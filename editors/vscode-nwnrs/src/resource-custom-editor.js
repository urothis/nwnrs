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
      '**/*.{mdl,wok,dwk,pwk,utc,utd,utp,uti,are,git,ifo,set,2da,mtr,txi,shd,dds,tga,plt,mod,erf,hak,nwm}',
    );
    const changed = (uri) => this.scheduleViewerRefresh(uri);
    this.context.subscriptions.push(
      this.worker,
      this.viewerWorker,
      this._onDidChangeCustomDocument,
      vscode.window.registerCustomEditorProvider(VIEW_TYPE, this, {
        webviewOptions: { retainContextWhenHidden: false },
        supportsMultipleEditorsPerDocument: true,
      }),
      vscode.workspace.registerTextDocumentContentProvider(VIEWER_TEXT_SCHEME, {
        provideTextDocumentContent: (uri) => {
          const id = new URLSearchParams(uri.query).get('id');
          const contents = this.viewerTextResources.get(id);
          if (contents === undefined) {
            throw new Error('The virtual nwnrs text dependency is no longer available.');
          }
          return contents;
        },
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
    );
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
    const viewer = VIEWER_EXTENSIONS.has(path.extname(uri.path).toLowerCase());
    if (uri.scheme === VIEWER_RESOURCE_SCHEME) {
      const virtualResourceId = new URLSearchParams(uri.query).get('id');
      const virtual = this.viewerResources.get(virtualResourceId);
      if (!virtual) throw new Error('The virtual nwnrs dependency is no longer available.');
      viewerContents = virtual.contents;
      viewerRequestOverride = virtual.request;
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
    document.scenePacket = undefined;
    document.scenePacketPromise = undefined;
    document.sceneGeneration = 0;
    document.sceneArea = undefined;
    document.viewerDependencyResources = new Set();
    document.viewerDependencyOrigins = new Set();
    document.virtualResourceId = uri.scheme === VIEWER_RESOURCE_SCHEME
      ? new URLSearchParams(uri.query).get('id')
      : undefined;
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
    if (sourceUri?.scheme !== 'file' || !sourceUri.fsPath) return;
    const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(
      path.dirname(sourceUri.fsPath),
      path.basename(sourceUri.fsPath),
    ));
    const changed = (uri) => this.scheduleViewerRefresh(uri);
    const disposable = vscode.Disposable.from(
      watcher,
      watcher.onDidCreate(changed),
      watcher.onDidChange(changed),
      watcher.onDidDelete(changed),
    );
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
        case 'importTexture':
          await this.selectTexture(view.webview);
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
        if (!destination) throw new Error('Save as Override was cancelled.');
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
      return view.webview.postMessage({ type: 'scene', packet });
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
      .map((view) => view.webview.postMessage({ type: 'scene', packet: recovered.packet })));
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

  async openDependency(document, resource) {
    if (!resource || path.basename(resource) !== resource) return;
    const request = this.viewerRequest(document);
    request.path = path.join(request.project_root, resource);
    request.area = null;
    const resolved = await this.viewerWorker.resolveResource(request);
    const extension = path.extname(resource).toLowerCase();
    if (resolved.file_path) {
      const uri = vscode.Uri.file(resolved.file_path);
      if (TEXT_DEPENDENCY_EXTENSIONS.has(extension)) {
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
    if (TEXT_DEPENDENCY_EXTENSIONS.has(extension)) {
      this.viewerTextResources.set(id, contents.toString('utf8'));
      const uri = vscode.Uri.from({
        scheme: VIEWER_TEXT_SCHEME,
        authority: 'game',
        path: `/${resource}`,
        query: new URLSearchParams({ id }).toString(),
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
    this.viewerResources.set(id, { contents, request });
    const uri = vscode.Uri.from({
      scheme: VIEWER_RESOURCE_SCHEME,
      authority: 'game',
      path: `/${resource}`,
      query: new URLSearchParams({ id }).toString(),
    });
    try {
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

  async selectTexture(webview) {
    const [uri] = await vscode.window.showOpenDialog({
      canSelectMany: false,
      openLabel: 'Import Texture Pixels',
      filters: { Images: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif'] },
    }) || [];
    if (!uri) return;
    const contents = await vscode.workspace.fs.readFile(uri);
    await webview.postMessage({
      type: 'textureFile',
      name: path.basename(uri.fsPath),
      contents: Buffer.from(contents).toString('base64'),
    });
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
      saveLabel: 'Save as Override',
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
  const directPaths = [document.uri?.fsPath, document.parent?.uri?.fsPath]
    .filter(Boolean)
    .map(normalizeFilePath);
  const projectRoot = normalizeFilePath(request.project_root);
  for (const changedPath of changedPaths) {
    if (directPaths.includes(changedPath)) return true;
    if ([...(document.viewerDependencyOrigins || [])].some((origin) => origin.includes(changedPath))) return true;
    if (changedPath.startsWith(`${projectRoot}${path.sep}`)
      && document.viewerDependencyResources?.has(path.basename(changedPath).toLowerCase())) return true;
  }
  return false;
}

function viewerAssetCacheMiss(error) {
  return /viewer (?:session is no longer available|scene assets were evicted)/iu.test(String(error?.message || error));
}

module.exports = {
  RESOURCE_SCHEME,
  VIEW_TYPE,
  ResourceCustomEditorProvider,
  decodeScenePacketManifest,
  viewerAssetCacheMiss,
  viewerAffectedByPaths,
};
