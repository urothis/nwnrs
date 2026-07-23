'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
const { ResourceEditorWorkerClient } = require('./resource-editor-worker-client');
const { ViewerWorkerClient } = require('./viewer-worker-client');
const { attachResourceEditorView, postResourceSnapshot } = require('./resource-editor-view');
const {
  findProjectRoot,
  nativeBindingPath,
  resolveConfiguredPath,
} = require('./compiler');

const VIEW_TYPE = 'nwnrs.resourceEditor';
const RESOURCE_SCHEME = 'nwnrs-resource';
const VIEWER_RESOURCE_SCHEME = 'nwnrs-viewer-resource';
const VIEWER_TEXT_SCHEME = 'nwnrs-viewer-text';
const VIEWER_EXTENSIONS = new Set([
  '.mdl', '.wok', '.dwk', '.pwk',
  '.utc', '.utd', '.utp', '.uti',
  '.are', '.git', '.ifo',
]);
const TEXT_DEPENDENCY_EXTENSIONS = new Set(['.mtr', '.txi', '.shd', '.set']);
const TEXT_RESOURCE_EXTENSIONS = new Set([
  ...TEXT_DEPENDENCY_EXTENSIONS,
  '.nss', '.lua', '.txt', '.ini', '.css',
]);
const CUSTOM_RESOURCE_EXTENSIONS = new Set([
  '.2da', '.tlk', '.dds', '.tga', '.plt', '.gff',
  '.utc', '.utd', '.ute', '.uti', '.utm', '.utp', '.uts', '.utt', '.utw',
  '.git', '.are', '.gic', '.ifo', '.fac', '.dlg', '.itp', '.bic', '.jrl', '.gui',
  '.erf', '.hak', '.mod', '.nwm', '.key', '.ncs', '.ndb',
  ...VIEWER_EXTENSIONS,
]);

class ResourceCustomDocument {
  constructor(uri, worker, id, snapshot, parent, resource) {
    this.uri = uri;
    this.worker = worker;
    this.id = id;
    this.snapshot = snapshot;
    this.parent = parent;
    this.resource = resource;
    this.views = new Set();
    this.dirty = false;
    this.disposed = false;
    this._onDidDispose = new vscode.EventEmitter();
    this.onDidDispose = this._onDidDispose.event;
  }

  request(method, request = {}, cancellationToken) {
    if (!this.worker) return Promise.reject(new Error('3D viewer documents are read-only.'));
    return this.worker.request(method, { documentId: this.id, ...request }, cancellationToken);
  }

  readEntryBytes(resource, cancellationToken) {
    if (!this.worker) return Promise.reject(new Error('This document is not an archive.'));
    return this.worker.readEntryBytes(this.id, resource, cancellationToken);
  }

  async refresh(options = {}, cancellationToken) {
    this.snapshot = await this.request('snapshot', options, cancellationToken);
    return this.snapshot;
  }

  async apply(edit) {
    const result = await this.request('applyEdit', { edit });
    this.snapshot = result.snapshot;
    this.dirty = true;
    return result;
  }

  async export() {
    return this.request('exportDocument');
  }

  async revert(cancellationToken) {
    if (this.parent) {
      const payload = await this.parent.request('readEntry', { resource: this.resource });
      await this.request('closeDocument');
      this.snapshot = await this.worker.request('openDocumentBytes', {
        documentId: this.id,
        path: this.uri.path,
        contents: payload.contents,
      }, cancellationToken);
    } else {
      this.snapshot = await this.request('revertDocument', {}, cancellationToken);
    }
    this.dirty = false;
    return this.snapshot;
  }

  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    if (!this.viewer) void this.request('closeDocument').catch(() => {});
    this._onDidDispose.fire();
    this._onDidDispose.dispose();
  }
}

class ResourceCustomEditorProvider {
  constructor(context, output) {
    this.context = context;
    this.output = output;
    this.documents = new Map();
    this.documentsById = new Map();
    this.viewerResources = new Map();
    this.viewerTextResources = new Map();
    this.viewerChangedPaths = new Set();
    this._onDidChangeCustomDocument = new vscode.EventEmitter();
    this.onDidChangeCustomDocument = this._onDidChangeCustomDocument.event;
    this._onDidSelectAreaObject = new vscode.EventEmitter();
    this.onDidSelectAreaObject = this._onDidSelectAreaObject.event;
    this.worker = new ResourceEditorWorkerClient(
      path.join(__dirname, 'resource-editor-worker.js'),
      nativeBindingPath(context.extensionPath),
      output,
    );
    this.viewerWorker = new ViewerWorkerClient(
      path.join(__dirname, 'viewer-worker.js'),
      nativeBindingPath(context.extensionPath),
      output,
    );
  }

  register() {
    const viewerWatcher = vscode.workspace.createFileSystemWatcher(
      '**/*.{mdl,wok,dwk,pwk,utc,utd,utp,uti,are,git,ifo,set,2da,mtr,txi,shd,dds,tga,plt,mod,erf,hak,nwm,json}',
    );
    const scriptDebugWatcher = vscode.workspace.createFileSystemWatcher('**/*.{ncs,ndb,nss}');
    const changed = (uri) => this.scheduleViewerRefresh(uri);
    const scriptDebugChanged = () => this.scheduleScriptDebugRefresh();
    this.context.subscriptions.push(
      this.worker,
      this.viewerWorker,
      this._onDidChangeCustomDocument,
      this._onDidSelectAreaObject,
      vscode.window.registerCustomEditorProvider(VIEW_TYPE, this, {
        webviewOptions: { retainContextWhenHidden: false },
        supportsMultipleEditorsPerDocument: true,
      }),
      vscode.workspace.registerTextDocumentContentProvider(VIEWER_TEXT_SCHEME, {
        provideTextDocumentContent: (uri) => this.virtualTextContents(uri),
      }),
      vscode.workspace.onDidCloseTextDocument((document) => {
        if (document.uri.scheme !== VIEWER_TEXT_SCHEME) return;
        const id = new URLSearchParams(document.uri.query).get('id');
        if (id) this.viewerTextResources.delete(id);
      }),
      viewerWatcher,
      viewerWatcher.onDidCreate(changed),
      viewerWatcher.onDidChange(changed),
      viewerWatcher.onDidDelete(changed),
      scriptDebugWatcher,
      scriptDebugWatcher.onDidCreate(scriptDebugChanged),
      scriptDebugWatcher.onDidChange(scriptDebugChanged),
      scriptDebugWatcher.onDidDelete(scriptDebugChanged),
    );
  }

  scheduleScriptDebugRefresh() {
    clearTimeout(this.scriptDebugRefreshTimer);
    this.scriptDebugRefreshTimer = setTimeout(() => {
      this.scriptDebugRefreshTimer = undefined;
      const previous = this.scriptDebugRefreshPromise || Promise.resolve();
      this.scriptDebugRefreshPromise = previous.catch(() => {}).then(async () => {
        const documents = [...this.documents.values()].filter((document) => document.snapshot?.kind === 'ncs' || document.snapshot?.kind === 'ndb');
        for (const document of documents) {
          if (document.parent || document.uri.scheme !== 'file' || document.dirty) continue;
          try {
            await document.revert();
            await this.enrichScriptDebugDocument(document);
            await this.broadcast(document);
          } catch (error) {
            this.output.appendLine(`Could not refresh script workbench ${document.uri.fsPath}: ${error.message || error}`);
          }
        }
      });
      void this.scriptDebugRefreshPromise.catch((error) => {
        this.output.appendLine(`Could not refresh script workbenches: ${error.message || error}`);
      });
    }, 100);
  }

  async virtualTextContents(uri) {
    const id = new URLSearchParams(uri.query).get('id');
    if (!id) throw new Error('The virtual nwnrs text URI is missing its resource identity.');
    const cached = this.viewerTextResources.get(id);
    if (cached !== undefined) return cached;
    const descriptor = decodeVirtualResourceDescriptor(uri);
    if (!descriptor) {
      throw new Error('This virtual nwnrs text tab predates restart-safe resources. Close it and open the dependency again.');
    }
    const resourceBytes = await this.viewerWorker.readResource(descriptor.request);
    const contents = Buffer.from(
      resourceBytes.buffer,
      resourceBytes.byteOffset,
      resourceBytes.byteLength,
    ).toString('utf8');
    this.viewerTextResources.set(id, contents);
    return contents;
  }

  async resolveVirtualResource(uri) {
    const id = new URLSearchParams(uri.query).get('id');
    if (!id) throw new Error('The virtual nwnrs resource URI is missing its resource identity.');
    const cached = this.viewerResources.get(id);
    if (cached) return cached;
    const descriptor = decodeVirtualResourceDescriptor(uri);
    if (!descriptor) {
      throw new Error('This virtual nwnrs tab predates restart-safe resources. Close it and open the resource again.');
    }
    let contents;
    if (!descriptor.request.authored_area) {
      const resourceBytes = await this.viewerWorker.readResource(descriptor.request);
      contents = Buffer.from(
        resourceBytes.buffer,
        resourceBytes.byteOffset,
        resourceBytes.byteLength,
      );
    }
    const resolved = { contents, request: descriptor.request };
    this.viewerResources.set(id, resolved);
    return resolved;
  }

  scheduleViewerRefresh(changedUri) {
    if (changedUri?.fsPath) this.viewerChangedPaths.add(normalizeFilePath(changedUri.fsPath));
    clearTimeout(this.viewerRefreshTimer);
    this.viewerRefreshTimer = setTimeout(() => {
      this.viewerRefreshTimer = undefined;
      const changedPaths = new Set(this.viewerChangedPaths); this.viewerChangedPaths.clear();
      const previousRefresh = this.viewerRefreshPromise || Promise.resolve();
      this.viewerRefreshPromise = previousRefresh.catch(() => {}).then(() => this.refreshViewerDocuments(changedPaths));
      void this.viewerRefreshPromise.catch((error) => {
        this.output.appendLine(`Could not refresh 3D scenes: ${error.message || error}`);
      });
    }, 100);
  }

  async refreshViewerDocuments(changedPaths = new Set()) {
    const viewers = [...this.documents.values()].filter((document) => document.viewer);
    const affected = viewers.filter((document) => viewerAffectedByPaths(
      document,
      changedPaths,
      this.viewerRequest(document),
    ));
    if (affected.length === 0) return;
    const changedParents = new Set(affected.map((document) => document.parent).filter((parent) => parent
      && !parent.dirty
      && parent.uri?.scheme === 'file'
      && changedPaths.has(normalizeFilePath(parent.uri.fsPath))));
    for (const parent of changedParents) {
      try {
        await parent.revert();
        await this.broadcast(parent);
      } catch (error) {
        this.output.appendLine(`Could not reload changed archive ${parent.uri.fsPath}: ${error.message || error}`);
      }
    }
    const sessions = new Set(affected.map((document) => this.viewerRequest(document).session_key));
    for (const sessionKey of sessions) this.viewerWorker.invalidate(sessionKey);
    for (const document of affected) {
      try {
        if (document.parent && document.resource) {
          document.viewerContents = Buffer.from(await document.parent.readEntryBytes(document.resource));
        }
        this.invalidateScene(document);
        await this.broadcast(document);
      } catch (error) {
        this.output.appendLine(`Could not refresh 3D scene ${document.uri.toString()}: ${error.message || error}`);
      }
    }
  }

  async openCustomDocument(uri, openContext, cancellationToken) {
    const key = uri.toString();
    const existing = this.documents.get(key);
    if (existing) return existing;
    const id = crypto.randomUUID();
    let snapshot;
    let parent;
    let resource;
    let viewerContents;
    let viewerRequestOverride;
    let selectedAreaObjectKey;
    const viewer = VIEWER_EXTENSIONS.has(path.extname(uri.path).toLowerCase());
    if (uri.scheme === VIEWER_RESOURCE_SCHEME) {
      const virtualResourceId = new URLSearchParams(uri.query).get('id');
      const virtual = await this.resolveVirtualResource(uri);
      viewerContents = virtual.contents;
      viewerRequestOverride = virtual.request;
      selectedAreaObjectKey = virtual.selectedObjectKey;
      if (viewer) snapshot = viewerSnapshot(uri.path);
      else snapshot = await this.worker.request('openDocumentBytes', {
        documentId: id,
        path: uri.path,
        contents: viewerContents.toString('base64'),
      }, cancellationToken);
    } else if (uri.scheme === RESOURCE_SCHEME) {
      const query = new URLSearchParams(uri.query);
      parent = this.documentsById.get(query.get('parentId'));
      resource = query.get('resource');
      if (!parent || !resource) throw new Error('The owning nwnrs archive is no longer open.');
      if (viewer) {
        viewerContents = Buffer.from(await parent.readEntryBytes(resource, cancellationToken));
        snapshot = viewerSnapshot(uri.path);
      } else if (openContext.backupId) {
        snapshot = await this.worker.request('openDocument', {
          documentId: id,
          path: uri.path,
          backupPath: vscode.Uri.parse(openContext.backupId).fsPath,
          readOnlyOrigin: true,
        }, cancellationToken);
      } else {
        const payload = await parent.request('readEntry', { resource }, cancellationToken);
        snapshot = await this.worker.request('openDocumentBytes', {
          documentId: id,
          path: uri.path,
          contents: payload.contents,
        }, cancellationToken);
      }
    } else {
      const backupPath = openContext.backupId
        ? vscode.Uri.parse(openContext.backupId).fsPath
        : undefined;
      snapshot = viewer
        ? viewerSnapshot(uri.fsPath)
        : await this.worker.request('openDocument', {
          documentId: id,
          path: uri.fsPath,
          backupPath,
          readOnlyOrigin: uri.scheme !== 'file',
        }, cancellationToken);
    }
    const document = new ResourceCustomDocument(
      uri,
      viewer ? undefined : this.worker,
      id,
      snapshot,
      parent,
      resource,
    );
    document.viewer = viewer;
    document.viewerContents = viewerContents;
    document.viewerRequestOverride = viewerRequestOverride;
    document.selectedAreaObjectKey = selectedAreaObjectKey;
    document.viewerSourcePaths = Object.values(
      viewerRequestOverride?.authored_area || {},
    ).filter((value) => typeof value === 'string' && path.isAbsolute(value));
    document.scenePacket = undefined;
    document.scenePacketPromise = undefined;
    document.sceneGeneration = 0;
    document.sceneArea = undefined;
    document.viewerDependencyResources = new Set();
    document.viewerDependencyOrigins = new Set();
    document.virtualResourceId = uri.scheme === VIEWER_RESOURCE_SCHEME
      ? new URLSearchParams(uri.query).get('id')
      : undefined;
    if (snapshot.kind === 'ncs' || snapshot.kind === 'ndb') {
      await this.enrichScriptDebugDocument(document, cancellationToken);
    }
    this.documents.set(key, document);
    this.documentsById.set(id, document);
    document.onDidDispose(() => {
      this.documents.delete(key);
      this.documentsById.delete(id);
      if (document.virtualResourceId) this.viewerResources.delete(document.virtualResourceId);
    });
    this.watchViewerSource(document);
    return document;
  }

  watchViewerSource(document) {
    if (!document.viewer) return;
    const sourceUri = document.parent?.uri || document.uri;
    const sourcePaths = new Set(document.viewerSourcePaths || []);
    if (sourceUri?.scheme === 'file' && sourceUri.fsPath) sourcePaths.add(sourceUri.fsPath);
    if (sourcePaths.size === 0) return;
    const changed = (uri) => this.scheduleViewerRefresh(uri);
    const watchers = [];
    for (const sourcePath of sourcePaths) {
      const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(
        path.dirname(sourcePath),
        path.basename(sourcePath),
      ));
      watchers.push(
        watcher,
        watcher.onDidCreate(changed),
        watcher.onDidChange(changed),
        watcher.onDidDelete(changed),
      );
    }
    const disposable = vscode.Disposable.from(...watchers);
    document.onDidDispose(() => disposable.dispose());
    this.context.subscriptions.push(disposable);
  }

  async resolveCustomEditor(document, webviewPanel) {
    const view = attachResourceEditorView(
      document,
      webviewPanel,
      (message, owningView) => this.handleMessage(document, owningView, message),
    );
    try {
      view.webview.options = {
        enableScripts: true,
        localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, 'media')],
      };
      view.webview.html = this.html(view.webview);
    } catch (error) {
      view.dispose();
      this.output.appendLine(
        `Could not resolve resource editor for ${document.uri.toString()}: ${error?.stack || String(error)}`,
      );
      throw error;
    }
  }

  async handleMessage(document, view, message) {
    try {
      switch (message?.type) {
        case 'ready':
          await this.postSnapshot(document, view);
          break;
        case 'selectArea':
          if (document.viewer) {
            document.sceneArea = message.area || undefined;
            this.invalidateScene(document);
            await this.broadcast(document);
          }
          break;
        case 'selectAreaObject':
          if (document.viewerRequestOverride?.authored_area) {
            document.selectedAreaObjectKey = typeof message.objectKey === 'string'
              ? message.objectKey
              : undefined;
            const request = this.viewerRequest(document);
            this._onDidSelectAreaObject.fire({
              manifestPath: request.session_key,
              resref: request.authored_area.resref,
              objectKey: document.selectedAreaObjectKey,
            });
            await Promise.all([...document.views]
              .filter((candidate) => candidate !== view && candidate.ready)
              .map((candidate) => candidate.webview.postMessage({
                type: 'selectAreaObject',
                objectKey: document.selectedAreaObjectKey || null,
                frame: false,
              })));
          }
          break;
        case 'inspectAreaObject':
          if (document.viewer) {
            const generation = document.sceneGeneration || 0;
            const assetKey = String(message.assetKey || '');
            const objectKey = String(message.objectKey || '');
            try {
              const inspection = await this.viewerWorker.inspectAreaObject({
                sessionKey: this.viewerRequest(document).session_key,
                assetKey,
                objectKey,
              });
              if (generation === (document.sceneGeneration || 0) && view.ready) {
                await view.webview.postMessage({
                  type: 'areaObjectInspection',
                  assetKey,
                  objectKey,
                  inspection,
                });
              }
            } catch (error) {
              if (generation !== (document.sceneGeneration || 0)) break;
              if (viewerAssetCacheMiss(error)) {
                await this.recoverViewerScene(document);
                break;
              }
              if (view.ready) await view.webview.postMessage({
                type: 'areaObjectInspectionError',
                assetKey,
                objectKey,
                message: error.message || String(error),
              });
            }
          }
          break;
        case 'loadAnimation':
          if (document.viewer) {
            const generation = document.sceneGeneration || 0;
            let packet;
            try {
              packet = await this.viewerWorker.loadAnimation({
                sessionKey: this.viewerRequest(document).session_key,
                assetKey: String(message.assetKey || ''),
                modelIndex: Number(message.modelIndex),
                animationIndex: Number(message.animationIndex),
              });
            } catch (error) {
              if (generation !== (document.sceneGeneration || 0)) break;
              if (viewerAssetCacheMiss(error)) {
                await this.recoverViewerScene(document);
                break;
              }
              throw error;
            }
            if (generation === (document.sceneGeneration || 0) && view.ready) {
              await view.webview.postMessage({ type: 'animationAsset', packet });
            }
          }
          break;
        case 'loadTexture':
          if (document.viewer) {
            const generation = document.sceneGeneration || 0;
            let packet;
            try {
              packet = await this.viewerWorker.loadTexture({
                sessionKey: this.viewerRequest(document).session_key,
                assetKey: String(message.assetKey || ''),
                textureIndex: Number(message.textureIndex),
                preferCompressed: message.preferCompressed === true,
              });
            } catch (error) {
              if (generation !== (document.sceneGeneration || 0)) break;
              if (viewerAssetCacheMiss(error)) {
                await this.recoverViewerScene(document);
                break;
              }
              throw error;
            }
            if (generation === (document.sceneGeneration || 0) && view.ready) {
              await view.webview.postMessage({ type: 'textureAsset', packet });
            }
          }
          break;
        case 'openDependency':
          if (document.viewer) await this.openDependency(document, message.resource);
          break;
        case 'openScriptSource':
          if (document.snapshot?.kind === 'ncs' || document.snapshot?.kind === 'ndb') {
            await this.openScriptSource(document, message.file, message.line);
          }
          break;
        case 'edit':
          await this.recordEdit(document, message.edit);
          break;
        case 'refresh':
          await document.refresh(message.options || {});
          await this.broadcast(document);
          break;
        case 'openEntry':
          await this.openEntry(document, message.resource);
          break;
        case 'replaceEntry':
          await this.replaceEntryFromDisk(document, message.resource);
          break;
        case 'addEntry':
          await this.addEntryFromDisk(document, message.bifIndex);
          break;
        case 'exportEntry':
          await this.exportEntry(document, message.resource);
          break;
        case 'showError':
          this.output.appendLine(
            `Resource editor webview error: ${String(message.message || 'Unknown resource editor error')}`,
          );
          void vscode.window.showErrorMessage(String(message.message || 'Unknown resource editor error'));
          break;
        default:
          break;
      }
    } catch (error) {
      this.output.appendLine(`Resource editor error: ${error?.stack || String(error)}`);
      void vscode.window.showErrorMessage(`nwnrs resource editor: ${error.message || String(error)}`);
    }
  }

  async recordEdit(document, edit) {
    const result = await document.apply(edit);
    let undoEdit = result.inverse;
    let redoEdit;
    this._onDidChangeCustomDocument.fire({
      document,
      label: result.label || 'Edit NWN resource',
      undo: async () => {
        const undoResult = await document.apply(undoEdit);
        redoEdit = undoResult.inverse;
        await this.broadcast(document);
        await this.reloadNestedChildren(document);
      },
      redo: async () => {
        const redoResult = await document.apply(redoEdit);
        undoEdit = redoResult.inverse;
        await this.broadcast(document);
        await this.reloadNestedChildren(document);
      },
    });
    await this.broadcast(document);
    await this.reloadNestedChildren(document);
  }

  async saveCustomDocument(document, cancellationToken) {
    if (document.viewer) throw new Error('3D viewer documents are read-only.');
    if (document.parent) {
      const payload = await document.export();
      await this.recordEdit(document.parent, {
        action: 'replaceEntry',
        resource: document.resource,
        contents: payload.contents,
      });
      document.dirty = false;
      return;
    }
    try {
      await document.request('saveDocument', {}, cancellationToken);
    } catch (error) {
      if (String(error.message).includes('READ_ONLY_ORIGIN')) {
        const destination = await this.pickOverrideDestination(document);
        if (!destination) throw new Error('Save was cancelled.');
        await document.request('saveDocumentAs', { path: destination.fsPath }, cancellationToken);
      } else if (String(error.message).includes('EXTERNAL_CHANGE')) {
        const choice = await vscode.window.showWarningMessage(
          `${path.basename(document.uri.fsPath)} changed on disk.`,
          { modal: true },
          'Overwrite',
          'Revert',
        );
        if (choice === 'Overwrite') {
          await document.request('saveDocument', { force: true }, cancellationToken);
        } else if (choice === 'Revert') {
          await document.revert(cancellationToken);
          await this.broadcast(document);
        } else {
          throw new Error('Save cancelled because the file changed on disk.');
        }
      } else {
        throw error;
      }
    }
    document.dirty = false;
  }

  async saveCustomDocumentAs(document, destination, cancellationToken) {
    if (document.viewer) throw new Error('3D viewer documents are read-only.');
    await document.request('saveDocumentAs', { path: destination.fsPath }, cancellationToken);
    document.dirty = false;
  }

  async revertCustomDocument(document, cancellationToken) {
    if (document.viewer) {
      this.viewerWorker.invalidate(this.viewerRequest(document).session_key);
      this.invalidateScene(document);
      await this.broadcast(document);
      return;
    }
    await document.revert(cancellationToken);
    if (document.snapshot?.kind === 'ncs' || document.snapshot?.kind === 'ndb') {
      await this.enrichScriptDebugDocument(document, cancellationToken);
    }
    await this.broadcast(document);
  }

  async backupCustomDocument(document, context, cancellationToken) {
    if (document.viewer) throw new Error('3D viewer documents do not require backups.');
    await document.request(
      'backupDocument',
      { path: context.destination.fsPath },
      cancellationToken,
    );
    return {
      id: context.destination.toString(),
      delete: () => vscode.workspace.fs.delete(context.destination).then(undefined, () => {}),
    };
  }

  async postSnapshot(document, view) {
    if (document.viewer) {
      if (!view.ready) return false;
      const packet = await this.scenePacket(document);
      return view.webview.postMessage({
        type: 'scene',
        packet,
        selectedObjectKey: document.selectedAreaObjectKey || null,
      });
    }
    await postResourceSnapshot(document, view);
  }

  invalidateScene(document) {
    document.sceneGeneration = (document.sceneGeneration || 0) + 1;
    document.scenePacket = undefined;
    document.scenePacketPromise = undefined;
  }

  async recoverViewerScene(document) {
    if (!document.sceneRecoveryPromise) {
      this.invalidateScene(document);
      const generation = document.sceneGeneration;
      document.sceneRecoveryPromise = this.scenePacket(document)
        .then((packet) => ({ generation, packet }))
        .finally(() => { document.sceneRecoveryPromise = undefined; });
    }
    const recovered = await document.sceneRecoveryPromise;
    if (recovered.generation !== document.sceneGeneration) return;
    await Promise.all([...document.views]
      .filter((view) => view.ready)
      .map((view) => view.webview.postMessage({
        type: 'scene',
        packet: recovered.packet,
        selectedObjectKey: document.selectedAreaObjectKey || null,
      })));
  }

  async scenePacket(document) {
    while (!document.scenePacket) {
      const generation = document.sceneGeneration || 0;
      if (!document.scenePacketPromise) {
        const loading = this.viewerWorker.loadScene(
          this.viewerRequest(document),
          document.viewerContents,
        ).then((packet) => ({ generation, packet }));
        document.scenePacketPromise = loading;
        void loading.finally(() => {
          if (document.scenePacketPromise === loading) document.scenePacketPromise = undefined;
        }).catch(() => {});
      }
      const loaded = await document.scenePacketPromise;
      if ((document.sceneGeneration || 0) !== loaded.generation) continue;
      document.scenePacket = loaded.packet;
      const manifest = decodeScenePacketManifest(loaded.packet);
      document.viewerDependencyResources = new Set(
        (manifest.dependencies?.nodes || []).map((node) => String(node.resource || '').toLowerCase()),
      );
      document.viewerDependencyOrigins = new Set(
        (manifest.dependencies?.nodes || [])
          .map((node) => node.origin && normalizeFilePath(String(node.origin)))
          .filter(Boolean),
      );
    }
    return document.scenePacket;
  }

  viewerRequest(document) {
    if (document.viewerRequestOverride) {
      return {
        ...document.viewerRequestOverride,
        area: document.sceneArea || null,
      };
    }
    const anchorPath = document.parent?.uri.fsPath || document.uri.fsPath;
    const projectRoot = findProjectRoot(anchorPath);
    const configuration = vscode.workspace.getConfiguration('nwnrs', document.uri);
    const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath || '';
    const context = {
      projectRoot,
      workspaceFolder,
      fileDirname: path.dirname(anchorPath),
    };
    const root = resolveConfiguredPath(configuration.get('rootPath', ''), context, projectRoot);
    const user = resolveConfiguredPath(configuration.get('userPath', ''), context, projectRoot);
    const archivePath = document.parent?.uri.fsPath;
    const archiveExtension = path.extname(archivePath || '').toLowerCase();
    return {
      session_key: path.join(projectRoot, 'nwpkg.toml'),
      path: document.resource ? path.join(projectRoot, document.resource) : document.uri.fsPath,
      project_root: projectRoot,
      area: document.sceneArea || null,
      root: root || null,
      user: user || null,
      language: configuration.get('language', 'english'),
      load_ovr: configuration.get('loadOvr', false),
      archives: ['.mod', '.erf', '.hak', '.nwm'].includes(archiveExtension)
        ? [archivePath]
        : [],
    };
  }

  async enrichScriptDebugDocument(document, cancellationToken) {
    const diagnostics = [];
    const primary = document.snapshot.kind;
    const base = path.basename(document.uri.path, path.extname(document.uri.path));
    const companion = primary === 'ncs' ? `${base}.ndb` : `${base}.ncs`;
    const companionBytes = await this.readScriptDebugResource(document, companion, cancellationToken);
    if (companionBytes) {
      try {
        document.snapshot = await document.request('configureScriptDebug', {
          [primary === 'ncs' ? 'ndb' : 'ncs']: companionBytes.toString('base64'),
        }, cancellationToken);
      } catch (error) {
        diagnostics.push(`Could not use ${companion}: ${error.message || error}`);
      }
    }
    const langspec = await this.readScriptDebugResource(document, 'nwscript.nss', cancellationToken);
    if (langspec) {
      try {
        document.snapshot = await document.request('configureScriptDebug', {
          langspec: langspec.toString('base64'),
        }, cancellationToken);
      } catch (error) {
        diagnostics.push(`Could not use nwscript.nss: ${error.message || error}`);
      }
    }
    const sourceFiles = document.snapshot.data?.sourceFiles || [];
    const sources = {};
    await Promise.all(sourceFiles.map(async (source) => {
      const name = String(source.name || '');
      if (!validScriptDebugResref(name)) return;
      const resource = name.toLowerCase().endsWith('.nss') ? name : `${name}.nss`;
      const bytes = await this.readScriptDebugResource(document, resource, cancellationToken);
      if (bytes) sources[name] = bytes.toString('base64');
    }));
    if (Object.keys(sources).length > 0) {
      try {
        document.snapshot = await document.request('configureScriptDebug', { sources }, cancellationToken);
      } catch (error) {
        diagnostics.push(`Could not map NDB source files: ${error.message || error}`);
      }
    }
    if (diagnostics.length > 0 && document.snapshot.data) {
      document.snapshot.data.diagnostics = [
        ...(document.snapshot.data.diagnostics || []),
        ...diagnostics,
      ];
    }
  }

  async readScriptDebugResource(document, resource, cancellationToken) {
    if (!resource || path.basename(resource) !== resource) return undefined;
    if (document.parent) {
      try {
        return Buffer.from(await document.parent.readEntryBytes(resource, cancellationToken));
      } catch {
        // The archive does not contain the companion; continue through normal
        // package/install precedence.
      }
    }
    if (document.uri.scheme === 'file') {
      const sibling = path.join(path.dirname(document.uri.fsPath), resource);
      try {
        return await fs.promises.readFile(sibling);
      } catch (error) {
        if (error?.code !== 'ENOENT') throw error;
      }
    }
    try {
      const request = this.viewerRequest(document);
      request.path = path.join(request.project_root, resource);
      request.area = null;
      const bytes = await this.viewerWorker.readResource(request);
      return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    } catch {
      return undefined;
    }
  }

  async openScriptSource(document, file, line) {
    const name = String(file || '');
    if (!validScriptDebugResref(name)) return;
    const resource = name.toLowerCase().endsWith('.nss') ? name : `${name}.nss`;
    const selection = new vscode.Range(
      Math.max(0, Number(line || 1) - 1),
      0,
      Math.max(0, Number(line || 1) - 1),
      0,
    );
    if (document.uri.scheme === 'file') {
      const sibling = path.join(path.dirname(document.uri.fsPath), resource);
      try {
        await fs.promises.access(sibling, fs.constants.R_OK);
        const textDocument = await vscode.workspace.openTextDocument(vscode.Uri.file(sibling));
        await vscode.window.showTextDocument(textDocument, { preview: true, selection });
        return;
      } catch (error) {
        if (error?.code !== 'ENOENT') throw error;
      }
    }
    const request = this.viewerRequest(document);
    request.path = path.join(request.project_root, resource);
    request.area = null;
    if (document.parent) {
      try {
        const bytes = Buffer.from(await document.parent.readEntryBytes(resource));
        await this.showVirtualScriptSource(request, resource, bytes, selection);
        return;
      } catch {
        // Continue through package/install precedence when the archive has no
        // matching source entry.
      }
    }
    const resolved = await this.viewerWorker.resolveResource(request);
    if (resolved.file_path) {
      const textDocument = await vscode.workspace.openTextDocument(vscode.Uri.file(resolved.file_path));
      await vscode.window.showTextDocument(textDocument, { preview: true, selection });
      return;
    }
    const resourceBytes = await this.viewerWorker.readResource(request);
    const contents = Buffer.from(
      resourceBytes.buffer,
      resourceBytes.byteOffset,
      resourceBytes.byteLength,
    );
    await this.showVirtualScriptSource(request, resource, contents, selection);
  }

  async showVirtualScriptSource(request, resource, contents, selection) {
    const id = crypto.randomUUID();
    this.viewerTextResources.set(id, contents.toString('utf8'));
    const uri = vscode.Uri.from({
      scheme: VIEWER_TEXT_SCHEME,
      authority: 'game',
      path: `/${resource}`,
      query: virtualResourceQuery(id, resource, request),
    });
    try {
      const textDocument = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(textDocument, { preview: true, selection });
    } catch (error) {
      this.viewerTextResources.delete(id);
      throw error;
    }
  }

  async openDependency(document, resource) {
    if (!resource || path.basename(resource) !== resource) return;
    const request = this.viewerRequest(document);
    await this.openResolvedResource(request, resource);
  }

  canOpenResource(resource, filePath) {
    if (filePath) return true;
    const extension = path.extname(resource).toLowerCase();
    return TEXT_RESOURCE_EXTENSIONS.has(extension) || CUSTOM_RESOURCE_EXTENSIONS.has(extension);
  }

  async openResolvedResource(baseRequest, resource) {
    if (!resource || path.basename(resource) !== resource) return;
    const request = { ...baseRequest };
    request.path = path.join(request.project_root, resource);
    request.area = null;
    const resolved = await this.viewerWorker.resolveResource(request);
    const extension = path.extname(resource).toLowerCase();
    if (resolved.file_path) {
      const uri = vscode.Uri.file(resolved.file_path);
      if (TEXT_RESOURCE_EXTENSIONS.has(extension) || !CUSTOM_RESOURCE_EXTENSIONS.has(extension)) {
        await vscode.commands.executeCommand('vscode.open', uri);
      } else {
        await vscode.commands.executeCommand('vscode.openWith', uri, VIEW_TYPE);
      }
      return;
    }
    const resourceBytes = await this.viewerWorker.readResource(request);
    // The worker has already transferred an independently owned ArrayBuffer.
    // Wrap that storage without another multi-megabyte copy; loadScene copies
    // it only when transferring a retained virtual resource back to the worker.
    const contents = Buffer.from(
      resourceBytes.buffer,
      resourceBytes.byteOffset,
      resourceBytes.byteLength,
    );
    const id = crypto.randomUUID();
    if (TEXT_RESOURCE_EXTENSIONS.has(extension)) {
      this.viewerTextResources.set(id, contents.toString('utf8'));
      const uri = vscode.Uri.from({
        scheme: VIEWER_TEXT_SCHEME,
        authority: 'game',
        path: `/${resource}`,
        query: virtualResourceQuery(id, resource, request),
      });
      try {
        const textDocument = await vscode.workspace.openTextDocument(uri);
        await vscode.window.showTextDocument(textDocument, { preview: true });
      } catch (error) {
        this.viewerTextResources.delete(id);
        throw error;
      }
      return;
    }
    if (!CUSTOM_RESOURCE_EXTENSIONS.has(extension)) {
      throw new Error(`No nwnrs editor is registered for packed ${extension || 'unknown'} resources.`);
    }
    this.viewerResources.set(id, { contents, request });
    const uri = vscode.Uri.from({
      scheme: VIEWER_RESOURCE_SCHEME,
      authority: 'game',
      path: `/${resource}`,
      query: virtualResourceQuery(id, resource, request),
    });
    try {
      await vscode.commands.executeCommand('vscode.openWith', uri, VIEW_TYPE);
    } catch (error) {
      this.viewerResources.delete(id);
      throw error;
    }
  }

  async openAuthoredArea(baseRequest, area, selectedObjectKey) {
    const request = authoredAreaRequest(baseRequest, area);
    const id = authoredAreaVirtualId(request);
    this.viewerResources.set(id, {
      contents: undefined,
      request,
      selectedObjectKey,
    });
    const uri = vscode.Uri.from({
      scheme: VIEWER_RESOURCE_SCHEME,
      authority: 'source',
      path: `/${area.resref}.are`,
      query: virtualResourceQuery(id, `${area.resref}.are`, request),
    });
    try {
      const existing = this.documents.get(uri.toString());
      if (existing) {
        existing.viewerRequestOverride = request;
        existing.selectedAreaObjectKey = selectedObjectKey;
        await Promise.all([...existing.views]
          .filter((view) => view.ready)
          .map((view) => view.webview.postMessage({
            type: 'selectAreaObject',
            objectKey: selectedObjectKey || null,
            frame: Boolean(selectedObjectKey),
          })));
      }
      await vscode.commands.executeCommand('vscode.openWith', uri, VIEW_TYPE);
    } catch (error) {
      this.viewerResources.delete(id);
      throw error;
    }
  }

  async broadcast(document) {
    await Promise.all([...document.views].map((view) => this.postSnapshot(document, view)));
  }

  async reloadNestedChildren(parent) {
    const children = [...this.documentsById.values()]
      .filter((document) => document.parent === parent && !document.dirty);
    for (const child of children) {
      try {
        if (child.viewer) {
          child.viewerContents = Buffer.from(await parent.readEntryBytes(child.resource));
          this.invalidateScene(child);
          this.viewerWorker.invalidate(this.viewerRequest(child).session_key);
        } else {
          await child.revert();
          if (child.snapshot?.kind === 'ncs' || child.snapshot?.kind === 'ndb') {
            await this.enrichScriptDebugDocument(child);
          }
        }
        await this.broadcast(child);
      } catch (error) {
        this.output.appendLine(`Could not refresh ${child.resource}: ${error.message || error}`);
      }
    }
  }

  async openEntry(document, resource) {
    const uri = vscode.Uri.from({
      scheme: RESOURCE_SCHEME,
      authority: 'archive',
      path: `/${resource}`,
      query: new URLSearchParams({ parentId: document.id, resource }).toString(),
    });
    await vscode.commands.executeCommand('vscode.openWith', uri, VIEW_TYPE);
  }

  async replaceEntryFromDisk(document, resource) {
    const [uri] = await vscode.window.showOpenDialog({
      canSelectMany: false,
      openLabel: `Replace ${resource}`,
    }) || [];
    if (!uri) return;
    const contents = await vscode.workspace.fs.readFile(uri);
    await this.recordEdit(document, {
      action: 'replaceEntry',
      resource,
      contents: Buffer.from(contents).toString('base64'),
    });
  }

  async addEntryFromDisk(document, bifIndex) {
    const [uri] = await vscode.window.showOpenDialog({ canSelectMany: false, openLabel: 'Add Resource' }) || [];
    if (!uri) return;
    const contents = await vscode.workspace.fs.readFile(uri);
    await this.recordEdit(document, {
      action: 'addEntry',
      resource: path.basename(uri.fsPath),
      contents: Buffer.from(contents).toString('base64'),
      bifIndex,
    });
  }

  async exportEntry(document, resource) {
    const destination = await vscode.window.showSaveDialog({
      defaultUri: vscode.Uri.file(path.join(path.dirname(document.uri.fsPath), resource)),
      saveLabel: 'Export Resource',
    });
    if (!destination) return;
    const payload = await document.request('readEntry', { resource });
    await vscode.workspace.fs.writeFile(destination, Buffer.from(payload.contents, 'base64'));
  }

  async pickOverrideDestination(document) {
    const configuration = vscode.workspace.getConfiguration('nwnrs', document.uri);
    const configured = configuration.get('overrideDirectory', '').trim();
    const userPath = configuration.get('userPath', '').trim();
    const directory = configured || (userPath ? path.join(userPath, 'override') : '');
    const defaultUri = directory
      ? vscode.Uri.file(path.join(directory, path.basename(document.uri.path)))
      : vscode.Uri.file(path.basename(document.uri.path));
    return vscode.window.showSaveDialog({
      defaultUri,
      saveLabel: 'Save',
    });
  }

  html(webview) {
    const nonce = crypto.randomBytes(18).toString('base64');
    const script = webview.asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, 'media', 'resource-editor.js'));
    const style = webview.asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, 'media', 'resource-editor.css'));
    return `<!doctype html>
<html lang="en"><head><meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} blob: data:; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${style}"><title>nwnrs Resource Editor</title></head>
<body><main id="app" aria-live="polite"><div class="loading">Loading resource…</div></main>
<script nonce="${nonce}" src="${script}"></script></body></html>`;
  }
}

function virtualResourceQuery(id, resource, request) {
  const descriptor = Buffer.from(JSON.stringify({
    schema: 1,
    resource,
    request,
  }), 'utf8').toString('base64url');
  return new URLSearchParams({ id, context: descriptor }).toString();
}

function decodeVirtualResourceDescriptor(uri) {
  const encoded = new URLSearchParams(uri.query).get('context');
  if (!encoded || encoded.length > 65536) return undefined;
  let descriptor;
  try {
    descriptor = JSON.parse(Buffer.from(encoded, 'base64url').toString('utf8'));
  } catch {
    return undefined;
  }
  if (!descriptor || descriptor.schema !== 1 || typeof descriptor.resource !== 'string') {
    return undefined;
  }
  const resource = descriptor.resource;
  if (!resource || path.basename(resource) !== resource
      || path.basename(uri.path).toLowerCase() !== resource.toLowerCase()
      || !validVirtualViewerRequest(descriptor.request, resource)) {
    return undefined;
  }
  return { resource, request: descriptor.request };
}

function validVirtualViewerRequest(request, resource) {
  if (!request || typeof request !== 'object' || Array.isArray(request)) return false;
  if (typeof request.session_key !== 'string' || request.session_key.length === 0) return false;
  if (typeof request.path !== 'string' || !path.isAbsolute(request.path)) return false;
  if (typeof request.project_root !== 'string' || !path.isAbsolute(request.project_root)) return false;
  if (path.basename(request.path).toLowerCase() !== resource.toLowerCase()) return false;
  if (request.area != null && typeof request.area !== 'string') return false;
  if (request.language != null && (typeof request.language !== 'string' || !request.language)) return false;
  if (request.load_ovr != null && typeof request.load_ovr !== 'boolean') return false;
  if (request.include_project_resources != null
      && typeof request.include_project_resources !== 'boolean') return false;
  for (const optionalPath of ['root', 'user']) {
    const value = request[optionalPath];
    if (value != null && (typeof value !== 'string' || !path.isAbsolute(value))) return false;
  }
  if (request.archives != null && (!Array.isArray(request.archives)
      || request.archives.some((value) => typeof value !== 'string' || !path.isAbsolute(value)))) {
    return false;
  }
  if (request.authored_area != null) {
    const area = request.authored_area;
    if (!area || typeof area !== 'object' || Array.isArray(area)
        || typeof area.resref !== 'string' || !area.resref
        || `${area.resref}.are`.toLowerCase() !== resource.toLowerCase()
        || typeof area.are !== 'string' || !path.isAbsolute(area.are)
        || typeof area.git !== 'string' || !path.isAbsolute(area.git)
        || (area.gic != null && (typeof area.gic !== 'string' || !path.isAbsolute(area.gic)))) {
      return false;
    }
  }
  return true;
}

function authoredAreaVirtualId(request) {
  const identity = `${request.session_key}\0${request.authored_area.resref.toLowerCase()}`;
  return `area-${crypto.createHash('sha256').update(identity).digest('hex').slice(0, 24)}`;
}

function viewerSnapshot(resourcePath) {
  return {
    path: resourcePath,
    kind: 'viewer',
    readOnlyOrigin: true,
    revision: 0,
    data: {},
  };
}

function normalizeFilePath(value) {
  const normalized = path.normalize(path.resolve(value));
  return process.platform === 'win32' ? normalized.toLowerCase() : normalized;
}

function validScriptDebugResref(value) {
  return typeof value === 'string'
    && value.length > 0
    && value.length <= 20
    && path.basename(value) === value
    && /^[A-Za-z0-9_]+(?:\.nss)?$/u.test(value);
}

function decodeScenePacketManifest(packetValue) {
  const packet = Buffer.from(packetValue);
  if (packet.length < 12 || packet.subarray(0, 8).toString('binary') !== 'NWNRS3D\0') {
    throw new Error('The renderer returned an invalid scene packet.');
  }
  const manifestLength = packet.readUInt32LE(8);
  const manifestEnd = 12 + manifestLength;
  if (manifestEnd > packet.length) throw new Error('The renderer returned a truncated scene packet.');
  return JSON.parse(packet.subarray(12, manifestEnd).toString('utf8'));
}

function viewerAffectedByPaths(document, changedPaths, request) {
  if (changedPaths.size === 0) return true;
  const directPaths = [
    document.uri?.fsPath,
    document.parent?.uri?.fsPath,
    ...(document.viewerSourcePaths || []),
  ]
    .filter(Boolean)
    .map(normalizeFilePath);
  for (const changedPath of changedPaths) {
    if (directPaths.includes(changedPath)) return true;
    if ([...(document.viewerDependencyOrigins || [])].some((origin) => origin.includes(changedPath))) return true;
    if (document.viewerDependencyResources?.has(resourceNameForChangedPath(changedPath))) return true;
  }
  return false;
}

function authoredAreaRequest(baseRequest, area) {
  if (!area?.resref) throw new Error('Area preview is missing its resource name.');
  const byKind = new Map();
  for (const file of area.files || []) {
    const kind = String(file.kind || '').toLowerCase();
    if (!['are', 'git', 'gic'].includes(kind)) continue;
    if (byKind.has(kind)) {
      throw new Error(`Area ${area.resref} has more than one ${kind.toUpperCase()} source.`);
    }
    byKind.set(kind, file.path);
  }
  if (!byKind.has('are') || !byKind.has('git')) {
    throw new Error(`Area ${area.resref} requires both ARE and GIT sources.`);
  }
  return {
    ...baseRequest,
    path: path.join(path.dirname(baseRequest.path), `${area.resref}.are`),
    area: null,
    authored_area: {
      resref: area.resref,
      are: byKind.get('are'),
      git: byKind.get('git'),
      gic: byKind.get('gic') || null,
    },
  };
}

function resourceNameForChangedPath(changedPath) {
  const basename = path.basename(changedPath).toLowerCase();
  return basename.endsWith('.json') ? basename.slice(0, -'.json'.length) : basename;
}

function viewerAssetCacheMiss(error) {
  return /viewer (?:session is no longer available|scene assets were evicted)/iu.test(String(error?.message || error));
}

module.exports = {
  RESOURCE_SCHEME,
  VIEW_TYPE,
  ResourceCustomEditorProvider,
  authoredAreaVirtualId,
  authoredAreaRequest,
  decodeScenePacketManifest,
  decodeVirtualResourceDescriptor,
  virtualResourceQuery,
  viewerAssetCacheMiss,
  viewerAffectedByPaths,
};
