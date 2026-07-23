import * as crypto from 'node:crypto';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import { ResourceEditorWorkerClient } from './resource-editor-worker-client';
import { ViewerWorkerClient } from './viewer-worker-client';
import {
  attachResourceEditorView,
  postResourceSnapshot,
  type ResourceEditorView,
} from './resource-editor-view';
import {
  findProjectRoot,
  nativeBindingPath,
  resolveConfiguredPath,
} from './compiler';
import type { NativePackageSourceArea } from './native-types';
import {
  isCustomEditorResource,
  isTextResource,
  isViewerResource,
} from './resource-capabilities.generated';

export const VIEW_TYPE = 'nwnrs.resourceEditor';
export const RESOURCE_SCHEME = 'nwnrs-resource';
const VIEWER_RESOURCE_SCHEME = 'nwnrs-viewer-resource';
const VIEWER_TEXT_SCHEME = 'nwnrs-viewer-text';
export const RESOURCE_EDITOR_WEBVIEW_ASSETS = {
  roots: [
    ['media'],
    ['dist', 'media'],
  ],
  capabilitiesScript: ['dist', 'media', 'resource-capabilities.generated.js'],
  script: ['dist', 'media', 'resource-editor.js'],
  style: ['media', 'resource-editor.css'],
} as const;
interface ResourceSnapshotData {
  sourceFiles?: readonly { readonly name?: string }[];
  diagnostics?: string[];
  [key: string]: unknown;
}

interface ResourceSnapshot {
  readonly kind: string;
  readonly path?: string;
  readonly readOnlyOrigin?: boolean;
  readonly revision?: number;
  data: ResourceSnapshotData;
  [key: string]: unknown;
}

interface ResourceEntryPayload {
  readonly contents: string;
}

interface ResourceEditResult {
  readonly snapshot: ResourceSnapshot;
  readonly inverse: unknown;
  readonly label?: string;
}

interface AuthoredArea {
  readonly resref: string;
  readonly are: string;
  readonly git: string;
  readonly gic: string | null;
}

interface ViewerRequest {
  session_key: string;
  path: string;
  project_root: string;
  area: string | null;
  root?: string | null;
  user?: string | null;
  language?: string;
  load_ovr?: boolean;
  archives?: readonly string[];
  include_project_resources?: boolean;
  authored_area?: AuthoredArea;
}

interface ViewerResource {
  readonly contents?: Uint8Array;
  readonly request: ViewerRequest;
  readonly selectedObjectKey?: string;
}

interface VirtualResourceDescriptor {
  readonly resource: string;
  readonly request: ViewerRequest;
}

interface AreaObjectSelection {
  readonly manifestPath: string;
  readonly resref: string;
  readonly objectKey?: string;
}

interface ResourceEditorMessage {
  readonly type?: string;
  readonly area?: unknown;
  readonly objectKey?: unknown;
  readonly assetKey?: unknown;
  readonly modelIndex?: unknown;
  readonly animationIndex?: unknown;
  readonly textureIndex?: unknown;
  readonly preferCompressed?: unknown;
  readonly resource?: unknown;
  readonly file?: unknown;
  readonly line?: unknown;
  readonly edit?: unknown;
  readonly options?: unknown;
  readonly bifIndex?: unknown;
  readonly message?: unknown;
  readonly path?: unknown;
  readonly offset?: unknown;
  readonly limit?: unknown;
  readonly requestId?: unknown;
}

interface SceneManifest {
  readonly dependencies?: {
    readonly nodes?: readonly {
      readonly resource?: string;
      readonly origin?: string | null;
    }[];
  };
}

interface SaveAsPlan {
  readonly targets: readonly string[];
  readonly conflicts: readonly {
    readonly path: string;
  }[];
  readonly confirmationToken?: string;
}

type ResourceDocumentChangeEvent =
  | vscode.CustomDocumentEditEvent<ResourceCustomDocument>
  | vscode.CustomDocumentContentChangeEvent<ResourceCustomDocument>;

class ResourceCustomDocument implements vscode.CustomDocument {
  public readonly uri: vscode.Uri;
  public readonly worker?: ResourceEditorWorkerClient;
  public readonly id: string;
  public snapshot: ResourceSnapshot;
  public readonly parent?: ResourceCustomDocument;
  public readonly resource?: string;
  public readonly views: Set<ResourceEditorView>;
  public dirty: boolean;
  public disposed: boolean;
  public viewer?: boolean;
  public viewerContents?: Uint8Array;
  public viewerRequestOverride?: ViewerRequest;
  public selectedAreaObjectKey?: string;
  public viewerSourcePaths?: string[];
  public scenePacket?: Uint8Array;
  public scenePacketPromise?: Promise<{
    readonly generation: number;
    readonly packet: Uint8Array;
  }>;
  public sceneRecoveryPromise?: Promise<{ readonly generation: number; readonly packet: Uint8Array }>;
  public sceneGeneration?: number;
  public sceneArea?: string;
  public viewerDependencyResources?: Set<string>;
  public viewerDependencyOrigins?: Set<string>;
  public virtualResourceId?: string;
  private readonly _onDidDispose: vscode.EventEmitter<void>;
  public readonly onDidDispose: vscode.Event<void>;

  public constructor(
    uri: vscode.Uri,
    worker: ResourceEditorWorkerClient | undefined,
    id: string,
    snapshot: ResourceSnapshot,
    parent?: ResourceCustomDocument,
    resource?: string,
  ) {
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

  request<TResponse>(
    method: string,
    request: Readonly<Record<string, unknown>> = {},
    cancellationToken?: vscode.CancellationToken,
  ): Promise<TResponse> {
    if (!this.worker) return Promise.reject(new Error('3D viewer documents are read-only.'));
    return this.worker.request(method, { documentId: this.id, ...request }, cancellationToken);
  }

  readEntryBytes(
    resource: string,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<Uint8Array> {
    if (!this.worker) return Promise.reject(new Error('This document is not an archive.'));
    return this.worker.readEntryBytes(this.id, resource, cancellationToken);
  }

  async refresh(
    options: Readonly<Record<string, unknown>> = {},
    cancellationToken?: vscode.CancellationToken,
  ): Promise<ResourceSnapshot> {
    this.snapshot = await this.request<ResourceSnapshot>('snapshot', options, cancellationToken);
    return this.snapshot;
  }

  async apply(edit: unknown): Promise<ResourceEditResult> {
    const result = await this.request<ResourceEditResult>('applyEdit', { edit });
    this.snapshot = result.snapshot;
    this.dirty = true;
    return result;
  }

  async export(): Promise<ResourceEntryPayload> {
    return this.request<ResourceEntryPayload>('exportDocument');
  }

  async revert(cancellationToken?: vscode.CancellationToken): Promise<ResourceSnapshot> {
    if (this.parent) {
      if (!this.resource || !this.worker) {
        throw new Error('The nested resource no longer has an owning archive.');
      }
      const payload = await this.parent.request<ResourceEntryPayload>(
        'readEntry',
        { resource: this.resource },
      );
      await this.request<unknown>('closeDocument');
      this.snapshot = await this.worker.request<ResourceSnapshot>('openDocumentBytes', {
        documentId: this.id,
        path: this.uri.path,
        contents: payload.contents,
      }, cancellationToken);
    } else {
      this.snapshot = await this.request<ResourceSnapshot>(
        'revertDocument',
        {},
        cancellationToken,
      );
    }
    this.dirty = false;
    return this.snapshot;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    if (!this.viewer) void this.request<unknown>('closeDocument').catch(() => {});
    this._onDidDispose.fire();
    this._onDidDispose.dispose();
  }
}

export class ResourceCustomEditorProvider
implements vscode.CustomEditorProvider<ResourceCustomDocument> {
  public readonly viewerWorker: ViewerWorkerClient;
  private readonly context: vscode.ExtensionContext;
  private readonly output: vscode.OutputChannel;
  private readonly documents: Map<string, ResourceCustomDocument>;
  private readonly documentsById: Map<string, ResourceCustomDocument>;
  private readonly viewerResources: Map<string, ViewerResource>;
  private readonly viewerTextResources: Map<string, string>;
  private readonly viewerChangedPaths: Set<string>;
  private readonly _onDidChangeCustomDocument: vscode.EventEmitter<ResourceDocumentChangeEvent>;
  public readonly onDidChangeCustomDocument: vscode.Event<ResourceDocumentChangeEvent>;
  private readonly _onDidSelectAreaObject: vscode.EventEmitter<AreaObjectSelection>;
  public readonly onDidSelectAreaObject: vscode.Event<AreaObjectSelection>;
  private readonly worker: ResourceEditorWorkerClient;
  private viewerRefreshTimer?: NodeJS.Timeout;
  private viewerRefreshPromise?: Promise<void>;
  private scriptDebugRefreshTimer?: NodeJS.Timeout;
  private scriptDebugRefreshPromise?: Promise<void>;

  public constructor(context: vscode.ExtensionContext, output: vscode.OutputChannel) {
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

  register(): void {
    const viewerWatcher = vscode.workspace.createFileSystemWatcher(
      '**/*.{mdl,wok,dwk,pwk,utc,utd,utp,uti,are,git,ifo,set,2da,mtr,txi,shd,dds,tga,plt,mod,erf,hak,nwm,json}',
    );
    const scriptDebugWatcher = vscode.workspace.createFileSystemWatcher('**/*.{ncs,ndb,nss}');
    const changed = (uri: vscode.Uri): void => this.scheduleViewerRefresh(uri);
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
        provideTextDocumentContent: (uri: vscode.Uri) => this.virtualTextContents(uri),
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

  scheduleScriptDebugRefresh(): void {
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
            this.output.appendLine(
              `Could not refresh script workbench ${document.uri.fsPath}: ${errorMessage(error)}`,
            );
          }
        }
      });
      void this.scriptDebugRefreshPromise.catch((error) => {
        this.output.appendLine(`Could not refresh script workbenches: ${errorMessage(error)}`);
      });
    }, 100);
  }

  async virtualTextContents(uri: vscode.Uri): Promise<string> {
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

  async resolveVirtualResource(uri: vscode.Uri): Promise<ViewerResource> {
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
    const resolved: ViewerResource = { contents, request: descriptor.request };
    this.viewerResources.set(id, resolved);
    return resolved;
  }

  scheduleViewerRefresh(changedUri: vscode.Uri): void {
    if (changedUri?.fsPath) this.viewerChangedPaths.add(normalizeFilePath(changedUri.fsPath));
    clearTimeout(this.viewerRefreshTimer);
    this.viewerRefreshTimer = setTimeout(() => {
      this.viewerRefreshTimer = undefined;
      const changedPaths = new Set(this.viewerChangedPaths); this.viewerChangedPaths.clear();
      const previousRefresh = this.viewerRefreshPromise || Promise.resolve();
      this.viewerRefreshPromise = previousRefresh.catch(() => {}).then(() => this.refreshViewerDocuments(changedPaths));
      void this.viewerRefreshPromise.catch((error) => {
        this.output.appendLine(`Could not refresh 3D scenes: ${errorMessage(error)}`);
      });
    }, 100);
  }

  async refreshViewerDocuments(changedPaths: ReadonlySet<string> = new Set()): Promise<void> {
    const viewers = [...this.documents.values()].filter((document) => document.viewer);
    const affected = viewers.filter((document) => viewerAffectedByPaths(
      document,
      changedPaths,
      this.viewerRequest(document),
    ));
    if (affected.length === 0) return;
    const changedParents = new Set<ResourceCustomDocument>(
      affected
        .map((document) => document.parent)
        .filter((parent): parent is ResourceCustomDocument => Boolean(
          parent
          && !parent.dirty
          && parent.uri.scheme === 'file'
          && changedPaths.has(normalizeFilePath(parent.uri.fsPath)),
        )),
    );
    for (const parent of changedParents) {
      try {
        await parent.revert();
        await this.broadcast(parent);
      } catch (error) {
        this.output.appendLine(
          `Could not reload changed archive ${parent.uri.fsPath}: ${errorMessage(error)}`,
        );
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
        this.output.appendLine(
          `Could not refresh 3D scene ${document.uri.toString()}: ${errorMessage(error)}`,
        );
      }
    }
  }

  async openCustomDocument(
    uri: vscode.Uri,
    openContext: vscode.CustomDocumentOpenContext,
    cancellationToken: vscode.CancellationToken,
  ): Promise<ResourceCustomDocument> {
    const key = uri.toString();
    const existing = this.documents.get(key);
    if (existing) return existing;
    const id = crypto.randomUUID();
    let snapshot: ResourceSnapshot;
    let parent: ResourceCustomDocument | undefined;
    let resource: string | undefined;
    let viewerContents: Uint8Array | undefined;
    let viewerRequestOverride: ViewerRequest | undefined;
    let selectedAreaObjectKey: string | undefined;
    const viewer = isViewerResource(uri.path);
    if (uri.scheme === VIEWER_RESOURCE_SCHEME) {
      const virtualResourceId = new URLSearchParams(uri.query).get('id');
      const virtual = await this.resolveVirtualResource(uri);
      viewerContents = virtual.contents;
      viewerRequestOverride = virtual.request;
      selectedAreaObjectKey = virtual.selectedObjectKey;
      if (viewer) snapshot = viewerSnapshot(uri.path);
      else {
        if (!viewerContents) {
          throw new Error('The virtual resource did not provide document bytes.');
        }
        snapshot = await this.worker.request<ResourceSnapshot>('openDocumentBytes', {
          documentId: id,
          path: uri.path,
          contents: Buffer.from(viewerContents).toString('base64'),
        }, cancellationToken);
      }
    } else if (uri.scheme === RESOURCE_SCHEME) {
      const query = new URLSearchParams(uri.query);
      const parentId = query.get('parentId');
      parent = parentId ? this.documentsById.get(parentId) : undefined;
      resource = query.get('resource') || undefined;
      if (!parent || !resource) throw new Error('The owning nwnrs archive is no longer open.');
      if (viewer) {
        viewerContents = Buffer.from(await parent.readEntryBytes(resource, cancellationToken));
        snapshot = viewerSnapshot(uri.path);
      } else if (openContext.backupId) {
        snapshot = await this.worker.request<ResourceSnapshot>('openDocument', {
          documentId: id,
          path: uri.path,
          backupPath: vscode.Uri.parse(openContext.backupId).fsPath,
          readOnlyOrigin: true,
        }, cancellationToken);
      } else {
        const payload = await parent.request<ResourceEntryPayload>(
          'readEntry',
          { resource },
          cancellationToken,
        );
        snapshot = await this.worker.request<ResourceSnapshot>('openDocumentBytes', {
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
        : await this.worker.request<ResourceSnapshot>('openDocument', {
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
    ).filter((value): value is string => typeof value === 'string' && path.isAbsolute(value));
    document.scenePacket = undefined;
    document.scenePacketPromise = undefined;
    document.sceneGeneration = 0;
    document.sceneArea = undefined;
    document.viewerDependencyResources = new Set();
    document.viewerDependencyOrigins = new Set();
    document.virtualResourceId = uri.scheme === VIEWER_RESOURCE_SCHEME
      ? new URLSearchParams(uri.query).get('id') || undefined
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

  watchViewerSource(document: ResourceCustomDocument): void {
    if (!document.viewer) return;
    const sourceUri = document.parent?.uri || document.uri;
    const sourcePaths = new Set<string>(document.viewerSourcePaths || []);
    if (sourceUri?.scheme === 'file' && sourceUri.fsPath) sourcePaths.add(sourceUri.fsPath);
    if (sourcePaths.size === 0) return;
    const changed = (uri: vscode.Uri): void => this.scheduleViewerRefresh(uri);
    const watchers: vscode.Disposable[] = [];
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

  async resolveCustomEditor(
    document: ResourceCustomDocument,
    webviewPanel: vscode.WebviewPanel,
  ): Promise<void> {
    const view = attachResourceEditorView(
      document,
      webviewPanel,
      (message, owningView) => this.handleMessage(document, owningView, message),
    );
    try {
      view.webview.options = {
        enableScripts: true,
        localResourceRoots: RESOURCE_EDITOR_WEBVIEW_ASSETS.roots.map(
          (segments) => vscode.Uri.joinPath(this.context.extensionUri, ...segments),
        ),
      };
      view.webview.html = this.html(view.webview);
    } catch (error) {
      view.dispose?.();
      this.output.appendLine(
        `Could not resolve resource editor for ${document.uri.toString()}: ${errorStack(error)}`,
      );
      throw error;
    }
  }

  async handleMessage(
    document: ResourceCustomDocument,
    view: ResourceEditorView,
    rawMessage: unknown,
  ): Promise<void> {
    const message = resourceEditorMessage(rawMessage);
    if (!message) {
      this.output.appendLine('Ignored malformed resource editor webview message.');
      return;
    }
    try {
      switch (message?.type) {
        case 'ready':
          await this.postSnapshot(document, view);
          break;
        case 'selectArea':
          if (document.viewer) {
            document.sceneArea = typeof message.area === 'string' && message.area
              ? message.area
              : undefined;
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
            const authoredArea = request.authored_area;
            if (!authoredArea) {
              break;
            }
            this._onDidSelectAreaObject.fire({
              manifestPath: request.session_key,
              resref: authoredArea.resref,
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
                message: errorMessage(error),
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
          await document.refresh(isRecord(message.options) ? message.options : {});
          await this.broadcast(document);
          break;
        case 'gffNode':
          if (document.snapshot?.kind === 'gff'
            && Array.isArray(message.path)
            && typeof message.requestId === 'number'
            && Number.isInteger(message.requestId)) {
            const node = await document.request('gffNode', {
              path: message.path,
              offset: typeof message.offset === 'number' && Number.isInteger(message.offset)
                ? message.offset
                : 0,
              limit: typeof message.limit === 'number' && Number.isInteger(message.limit)
                ? message.limit
                : 200,
            });
            if (view.ready) {
              await view.webview.postMessage({
                type: 'gffNode',
                requestId: message.requestId,
                node,
              });
            }
          }
          break;
        case 'openEntry':
          if (typeof message.resource === 'string') {
            await this.openEntry(document, message.resource);
          }
          break;
        case 'replaceEntry':
          if (typeof message.resource === 'string') {
            await this.replaceEntryFromDisk(document, message.resource);
          }
          break;
        case 'addEntry':
          await this.addEntryFromDisk(document, message.bifIndex);
          break;
        case 'exportEntry':
          if (typeof message.resource === 'string') {
            await this.exportEntry(document, message.resource);
          }
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
      if (message.type === 'edit') {
        try {
          await this.broadcast(document);
        } catch (resyncError) {
          this.output.appendLine(
            `Resource editor resync failed after rejected edit: ${errorStack(resyncError)}`,
          );
        }
      }
      this.output.appendLine(`Resource editor error: ${errorStack(error)}`);
      void vscode.window.showErrorMessage(`nwnrs resource editor: ${errorMessage(error)}`);
    }
  }

  async recordEdit(document: ResourceCustomDocument, edit: unknown): Promise<void> {
    const result = await document.apply(edit);
    let undoEdit = result.inverse;
    let redoEdit: unknown;
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
        if (redoEdit === undefined) {
          throw new Error('Cannot redo a resource edit before its undo operation has completed.');
        }
        const redoResult = await document.apply(redoEdit);
        undoEdit = redoResult.inverse;
        await this.broadcast(document);
        await this.reloadNestedChildren(document);
      },
    });
    await this.broadcast(document);
    await this.reloadNestedChildren(document);
  }

  async saveCustomDocument(
    document: ResourceCustomDocument,
    cancellationToken: vscode.CancellationToken,
  ): Promise<void> {
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
      const message = errorMessage(error);
      if (message.includes('READ_ONLY_ORIGIN')) {
        const destination = await this.pickOverrideDestination(document);
        if (!destination) throw new Error('Save was cancelled.');
        await this.saveDocumentAsConfirmed(document, destination, cancellationToken);
      } else if (message.includes('EXTERNAL_CHANGE')) {
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

  async saveCustomDocumentAs(
    document: ResourceCustomDocument,
    destination: vscode.Uri,
    cancellationToken: vscode.CancellationToken,
  ): Promise<void> {
    if (document.viewer) throw new Error('3D viewer documents are read-only.');
    await this.saveDocumentAsConfirmed(document, destination, cancellationToken);
    document.dirty = false;
  }

  private async saveDocumentAsConfirmed(
    document: ResourceCustomDocument,
    destination: vscode.Uri,
    cancellationToken: vscode.CancellationToken,
  ): Promise<void> {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      if (cancellationToken.isCancellationRequested) {
        throw new Error('Save was cancelled.');
      }
      const plan = await document.request<SaveAsPlan>(
        'planSaveDocumentAs',
        { path: destination.fsPath },
        cancellationToken,
      );
      let confirmationToken: string | undefined;
      if (plan.conflicts.length > 0) {
        const displayed = plan.conflicts
          .slice(0, 12)
          .map((conflict) => `• ${conflict.path}`)
          .join('\n');
        const remaining = plan.conflicts.length - Math.min(plan.conflicts.length, 12);
        const suffix = remaining > 0 ? `\n• …and ${remaining} more` : '';
        const choice = await vscode.window.showWarningMessage(
          `Saving this KEY will replace ${plan.conflicts.length} files:\n${displayed}${suffix}`,
          { modal: true, detail: 'The KEY and every listed BIF are committed as one resource set.' },
          'Replace All',
        );
        if (choice !== 'Replace All') throw new Error('Save was cancelled.');
        confirmationToken = plan.confirmationToken;
      }
      try {
        await document.request(
          'saveDocumentAs',
          {
            path: destination.fsPath,
            ...(confirmationToken ? { confirmationToken } : {}),
          },
          cancellationToken,
        );
        return;
      } catch (error) {
        if (!errorMessage(error).includes('SAVE_AS_CONFIRMATION_REQUIRED') || attempt === 2) {
          throw error;
        }
      }
    }
    throw new Error('Save destination continued changing while confirmation was in progress.');
  }

  async revertCustomDocument(
    document: ResourceCustomDocument,
    cancellationToken: vscode.CancellationToken,
  ): Promise<void> {
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

  async backupCustomDocument(
    document: ResourceCustomDocument,
    context: vscode.CustomDocumentBackupContext,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.CustomDocumentBackup> {
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

  async postSnapshot(
    document: ResourceCustomDocument,
    view: ResourceEditorView,
  ): Promise<boolean> {
    if (document.viewer) {
      if (!view.ready) return false;
      const packet = await this.scenePacket(document);
      return view.webview.postMessage({
        type: 'scene',
        packet,
        selectedObjectKey: document.selectedAreaObjectKey || null,
      });
    }
    return postResourceSnapshot(document, view);
  }

  invalidateScene(document: ResourceCustomDocument): void {
    document.sceneGeneration = (document.sceneGeneration || 0) + 1;
    document.scenePacket = undefined;
    document.scenePacketPromise = undefined;
  }

  async recoverViewerScene(document: ResourceCustomDocument): Promise<void> {
    let recovery = document.sceneRecoveryPromise;
    if (!recovery) {
      this.invalidateScene(document);
      const generation = document.sceneGeneration || 0;
      recovery = this.scenePacket(document)
        .then((packet) => ({ generation, packet }))
        .finally(() => { document.sceneRecoveryPromise = undefined; });
      document.sceneRecoveryPromise = recovery;
    }
    const recovered = await recovery;
    if (recovered.generation !== document.sceneGeneration) return;
    await Promise.all([...document.views]
      .filter((view) => view.ready)
      .map((view) => view.webview.postMessage({
        type: 'scene',
        packet: recovered.packet,
        selectedObjectKey: document.selectedAreaObjectKey || null,
      })));
  }

  async scenePacket(document: ResourceCustomDocument): Promise<Uint8Array> {
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
          .filter((origin): origin is string => Boolean(origin)),
      );
    }
    return document.scenePacket;
  }

  viewerRequest(document: ResourceCustomDocument): ViewerRequest {
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
      archives: archivePath && ['.mod', '.erf', '.hak', '.nwm'].includes(archiveExtension)
        ? [archivePath]
        : [],
    };
  }

  async enrichScriptDebugDocument(
    document: ResourceCustomDocument,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<void> {
    const diagnostics: string[] = [];
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
        diagnostics.push(`Could not use ${companion}: ${errorMessage(error)}`);
      }
    }
    const langspec = await this.readScriptDebugResource(document, 'nwscript.nss', cancellationToken);
    if (langspec) {
      try {
        document.snapshot = await document.request('configureScriptDebug', {
          langspec: langspec.toString('base64'),
        }, cancellationToken);
      } catch (error) {
        diagnostics.push(`Could not use nwscript.nss: ${errorMessage(error)}`);
      }
    }
    const sourceFiles = document.snapshot.data?.sourceFiles || [];
    const sources: Record<string, string> = {};
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
        diagnostics.push(`Could not map NDB source files: ${errorMessage(error)}`);
      }
    }
    if (diagnostics.length > 0 && document.snapshot.data) {
      document.snapshot.data.diagnostics = [
        ...(document.snapshot.data.diagnostics || []),
        ...diagnostics,
      ];
    }
  }

  async readScriptDebugResource(
    document: ResourceCustomDocument,
    resource: string,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<Buffer | undefined> {
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
        if (!isNodeErrorCode(error, 'ENOENT')) throw error;
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

  async openScriptSource(
    document: ResourceCustomDocument,
    file: unknown,
    line: unknown,
  ): Promise<void> {
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
        if (!isNodeErrorCode(error, 'ENOENT')) throw error;
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

  async showVirtualScriptSource(
    request: ViewerRequest,
    resource: string,
    contents: Uint8Array,
    selection: vscode.Range,
  ): Promise<void> {
    const id = crypto.randomUUID();
    this.viewerTextResources.set(id, Buffer.from(contents).toString('utf8'));
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

  async openDependency(document: ResourceCustomDocument, resource: unknown): Promise<void> {
    if (typeof resource !== 'string' || !resource || path.basename(resource) !== resource) return;
    const request = this.viewerRequest(document);
    await this.openResolvedResource(request, resource);
  }

  canOpenResource(resource: string | undefined, filePath?: string): boolean {
    if (filePath) return true;
    if (!resource) return false;
    return isTextResource(resource) || isCustomEditorResource(resource);
  }

  async openResolvedResource(baseRequest: ViewerRequest, resource: string): Promise<void> {
    if (!resource || path.basename(resource) !== resource) return;
    const request = { ...baseRequest };
    request.path = path.join(request.project_root, resource);
    request.area = null;
    const resolved = await this.viewerWorker.resolveResource(request);
    const extension = path.extname(resource).toLowerCase();
    if (resolved.file_path) {
      const uri = vscode.Uri.file(resolved.file_path);
      if (isTextResource(resource) || !isCustomEditorResource(resource)) {
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
    if (isTextResource(resource)) {
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
    if (!isCustomEditorResource(resource)) {
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

  async openAuthoredArea(
    baseRequest: ViewerRequest,
    area: NativePackageSourceArea,
    selectedObjectKey?: string,
  ): Promise<void> {
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

  async broadcast(document: ResourceCustomDocument): Promise<void> {
    await Promise.all([...document.views].map((view) => this.postSnapshot(document, view)));
  }

  async reloadNestedChildren(parent: ResourceCustomDocument): Promise<void> {
    const children = [...this.documentsById.values()]
      .filter((document) => document.parent === parent && !document.dirty);
    for (const child of children) {
      try {
        if (child.viewer) {
          if (!child.resource) {
            throw new Error('Nested viewer resource lost its archive entry identity.');
          }
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
        this.output.appendLine(`Could not refresh ${child.resource}: ${errorMessage(error)}`);
      }
    }
  }

  async openEntry(document: ResourceCustomDocument, resource: string): Promise<void> {
    const uri = vscode.Uri.from({
      scheme: RESOURCE_SCHEME,
      authority: 'archive',
      path: `/${resource}`,
      query: new URLSearchParams({ parentId: document.id, resource }).toString(),
    });
    await vscode.commands.executeCommand('vscode.openWith', uri, VIEW_TYPE);
  }

  async replaceEntryFromDisk(
    document: ResourceCustomDocument,
    resource: string,
  ): Promise<void> {
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

  async addEntryFromDisk(document: ResourceCustomDocument, bifIndex: unknown): Promise<void> {
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

  async exportEntry(document: ResourceCustomDocument, resource: string): Promise<void> {
    const destination = await vscode.window.showSaveDialog({
      defaultUri: vscode.Uri.file(path.join(path.dirname(document.uri.fsPath), resource)),
      saveLabel: 'Export Resource',
    });
    if (!destination) return;
    const payload = await document.request<ResourceEntryPayload>('readEntry', { resource });
    await vscode.workspace.fs.writeFile(destination, Buffer.from(payload.contents, 'base64'));
  }

  async pickOverrideDestination(document: ResourceCustomDocument): Promise<vscode.Uri | undefined> {
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

  html(webview: vscode.Webview): string {
    const nonce = crypto.randomBytes(18).toString('base64');
    const script = webview.asWebviewUri(vscode.Uri.joinPath(
      this.context.extensionUri,
      ...RESOURCE_EDITOR_WEBVIEW_ASSETS.script,
    ));
    const capabilitiesScript = webview.asWebviewUri(vscode.Uri.joinPath(
      this.context.extensionUri,
      ...RESOURCE_EDITOR_WEBVIEW_ASSETS.capabilitiesScript,
    ));
    const style = webview.asWebviewUri(vscode.Uri.joinPath(
      this.context.extensionUri,
      ...RESOURCE_EDITOR_WEBVIEW_ASSETS.style,
    ));
    return `<!doctype html>
<html lang="en"><head><meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} blob: data:; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${style}"><title>nwnrs Resource Editor</title></head>
<body><main id="app" aria-live="polite"><div class="loading">Loading resource…</div></main>
<script nonce="${nonce}">
const bootApp = document.getElementById('app');
if (bootApp) {
  bootApp.dataset.bootTimer = String(window.setTimeout(() => {
    bootApp.innerHTML = '<div class="empty status-error"><strong>Could not start the resource editor.</strong><br>The packaged renderer script did not load.</div>';
  }, 10000));
}
</script>
<script nonce="${nonce}" src="${capabilitiesScript}"></script>
<script nonce="${nonce}" src="${script}"></script></body></html>`;
  }
}

function virtualResourceQuery(id: string, resource: string, request: ViewerRequest): string {
  const descriptor = Buffer.from(JSON.stringify({
    schema: 1,
    resource,
    request,
  }), 'utf8').toString('base64url');
  return new URLSearchParams({ id, context: descriptor }).toString();
}

function decodeVirtualResourceDescriptor(
  uri: vscode.Uri,
): VirtualResourceDescriptor | undefined {
  const encoded = new URLSearchParams(uri.query).get('context');
  if (!encoded || encoded.length > 65536) return undefined;
  let descriptor: unknown;
  try {
    descriptor = JSON.parse(Buffer.from(encoded, 'base64url').toString('utf8'));
  } catch {
    return undefined;
  }
  if (!isRecord(descriptor)
      || descriptor.schema !== 1
      || typeof descriptor.resource !== 'string') {
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

function validVirtualViewerRequest(request: unknown, resource: string): request is ViewerRequest {
  if (!isRecord(request)) return false;
  if (typeof request.session_key !== 'string' || request.session_key.length === 0) return false;
  if (typeof request.path !== 'string' || !path.isAbsolute(request.path)) return false;
  if (typeof request.project_root !== 'string' || !path.isAbsolute(request.project_root)) return false;
  if (path.basename(request.path).toLowerCase() !== resource.toLowerCase()) return false;
  if (request.area != null && typeof request.area !== 'string') return false;
  if (request.language != null && (typeof request.language !== 'string' || !request.language)) return false;
  if (request.load_ovr != null && typeof request.load_ovr !== 'boolean') return false;
  if (request.include_project_resources != null
      && typeof request.include_project_resources !== 'boolean') return false;
  for (const optionalPath of ['root', 'user'] as const) {
    const value = request[optionalPath];
    if (value != null && (typeof value !== 'string' || !path.isAbsolute(value))) return false;
  }
  if (request.archives != null && (!Array.isArray(request.archives)
      || request.archives.some((value) => typeof value !== 'string' || !path.isAbsolute(value)))) {
    return false;
  }
  if (request.authored_area != null) {
    const area = request.authored_area;
    if (!isRecord(area)
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

function authoredAreaVirtualId(request: ViewerRequest & { authored_area: AuthoredArea }): string {
  const identity = `${request.session_key}\0${request.authored_area.resref.toLowerCase()}`;
  return `area-${crypto.createHash('sha256').update(identity).digest('hex').slice(0, 24)}`;
}

function viewerSnapshot(resourcePath: string): ResourceSnapshot {
  return {
    path: resourcePath,
    kind: 'viewer',
    readOnlyOrigin: true,
    revision: 0,
    data: {},
  };
}

function normalizeFilePath(value: string): string {
  const normalized = path.normalize(path.resolve(value));
  return process.platform === 'win32' ? normalized.toLowerCase() : normalized;
}

function validScriptDebugResref(value: unknown): value is string {
  return typeof value === 'string'
    && value.length > 0
    && value.length <= 20
    && path.basename(value) === value
    && /^[A-Za-z0-9_]+(?:\.nss)?$/u.test(value);
}

function decodeScenePacketManifest(packetValue: Uint8Array): SceneManifest {
  const packet = Buffer.from(packetValue);
  if (packet.length < 12 || packet.subarray(0, 8).toString('binary') !== 'NWNRS3D\0') {
    throw new Error('The renderer returned an invalid scene packet.');
  }
  const manifestLength = packet.readUInt32LE(8);
  const manifestEnd = 12 + manifestLength;
  if (manifestEnd > packet.length) throw new Error('The renderer returned a truncated scene packet.');
  const decoded: unknown = JSON.parse(packet.subarray(12, manifestEnd).toString('utf8'));
  if (!isRecord(decoded)) {
    throw new Error('The renderer returned an invalid scene manifest.');
  }
  const dependencies = isRecord(decoded.dependencies) ? decoded.dependencies : undefined;
  const nodes = Array.isArray(dependencies?.nodes)
    ? dependencies.nodes
      .filter(isRecord)
      .map((node) => ({
        resource: typeof node.resource === 'string' ? node.resource : undefined,
        origin: typeof node.origin === 'string' ? node.origin : null,
      }))
    : [];
  return { dependencies: { nodes } };
}

function viewerAffectedByPaths(
  document: ResourceCustomDocument,
  changedPaths: ReadonlySet<string>,
  _request: ViewerRequest,
): boolean {
  if (changedPaths.size === 0) return true;
  const directPaths = [
    document.uri?.fsPath,
    document.parent?.uri?.fsPath,
    ...(document.viewerSourcePaths || []),
  ]
    .filter((entry): entry is string => Boolean(entry))
    .map(normalizeFilePath);
  for (const changedPath of changedPaths) {
    if (directPaths.includes(changedPath)) return true;
    if ([...(document.viewerDependencyOrigins || [])].some((origin) => origin.includes(changedPath))) return true;
    if (document.viewerDependencyResources?.has(resourceNameForChangedPath(changedPath))) return true;
  }
  return false;
}

function authoredAreaRequest(
  baseRequest: ViewerRequest,
  area: NativePackageSourceArea,
): ViewerRequest & { authored_area: AuthoredArea } {
  if (!area?.resref) throw new Error('Area preview is missing its resource name.');
  const byKind = new Map<string, string>();
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
  const are = byKind.get('are');
  const git = byKind.get('git');
  if (!are || !git) {
    throw new Error(`Area ${area.resref} requires both ARE and GIT sources.`);
  }
  return {
    ...baseRequest,
    path: path.join(path.dirname(baseRequest.path), `${area.resref}.are`),
    area: null,
    authored_area: {
      resref: area.resref,
      are,
      git,
      gic: byKind.get('gic') || null,
    },
  };
}

function resourceNameForChangedPath(changedPath: string): string {
  const basename = path.basename(changedPath).toLowerCase();
  return basename.endsWith('.json') ? basename.slice(0, -'.json'.length) : basename;
}

function viewerAssetCacheMiss(error: unknown): boolean {
  return /viewer (?:session is no longer available|scene assets were evicted)/iu.test(
    errorMessage(error),
  );
}

function resourceEditorMessage(value: unknown): ResourceEditorMessage | undefined {
  if (!isRecord(value) || typeof value.type !== 'string') {
    return undefined;
  }
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function errorStack(error: unknown): string {
  return error instanceof Error ? error.stack || error.message : String(error);
}

function isNodeErrorCode(error: unknown, code: string): boolean {
  return isRecord(error) && error.code === code;
}

export {
  authoredAreaVirtualId,
  authoredAreaRequest,
  decodeScenePacketManifest,
  decodeVirtualResourceDescriptor,
  virtualResourceQuery,
  viewerAssetCacheMiss,
  viewerAffectedByPaths,
};
