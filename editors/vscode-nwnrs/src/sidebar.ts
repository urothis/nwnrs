import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import { resolveConfiguredPath } from './compiler';
import {
  VIEW_TYPE,
  type ResourceCustomEditorProvider,
} from './resource-custom-editor';
import type { ViewerWorkerClient } from './viewer-worker-client';
import type {
  NativeAreaObject,
  NativePackageDependency,
  NativePackageInfo,
  NativePackageSourceArea,
  NativePackageSourceFile,
  NativePackageSourceInfo,
  NativeResourceCatalogItem,
} from './native-types';

interface SidebarCompilerController {
  reindexCurrentPackage(): Promise<void>;
  restartLanguageService(): Promise<void>;
  clearDiagnostics(): void;
}

interface ResourceRequest {
  readonly session_key: string;
  readonly path: string;
  readonly project_root: string;
  readonly area: null;
  readonly root: string | null;
  readonly user: string | null;
  readonly language: string;
  readonly load_ovr: boolean;
  readonly archives: readonly string[];
  readonly include_project_resources: boolean;
}

interface ResourceQuery {
  readonly stage: string;
  readonly layer?: string;
  readonly family?: string;
  readonly extension?: string;
  readonly prefix?: string;
}

interface AreaObjectSelection {
  readonly manifestPath: string;
  readonly resref: string;
  readonly objectKey?: string;
}

interface SidebarTreeNode {
  kind: string;
  label: string;
  package?: NativePackageInfo;
  packageRoot?: string;
  dependencies?: readonly NativePackageDependency[];
  file?: NativePackageSourceFile;
  area?: NativePackageSourceArea;
  object?: NativeAreaObject;
  objectKind?: string;
  section?: string;
  resource?: string;
  origin?: string;
  filePath?: string;
  layer?: string;
  family?: string;
  extension?: string;
  prefix?: string;
  error?: string;
  count?: number;
  description?: string;
  tooltip?: string;
  icon?: string;
  uri?: vscode.Uri;
  command?: string;
  children?: SidebarTreeNode[];
  loadedChildren?: SidebarTreeNode[];
  parent?: SidebarTreeNode;
}

interface DirectoryEntry {
  readonly directory: string;
  readonly node: SidebarTreeNode;
}

interface DirectoryBranch {
  readonly directories: Map<string, DirectoryBranch>;
  readonly leaves: SidebarTreeNode[];
}

const PINNED_PACKAGE_KEY = 'nwnrs.sidebar.pinnedPackage';
const RESOURCE_LAYER_ORDER = [
  'Workspace',
  'Package Dependencies',
  'User Override',
  'Archives',
  'Vanilla',
];

export class NwnrsSidebarController {
  public readonly context: vscode.ExtensionContext;
  public readonly output: vscode.OutputChannel;
  public readonly viewerWorker: ViewerWorkerClient;
  public readonly resourceEditors: ResourceCustomEditorProvider;
  public readonly compilerController: SidebarCompilerController;
  public packages: NativePackageInfo[];
  public packageByRoot: Map<string, NativePackageInfo>;
  public activePackage?: NativePackageInfo;
  public pinnedRoot?: string;
  private packageRefresh?: Promise<void>;
  private packageRefreshRequested: boolean;
  private packageRefreshTimer?: NodeJS.Timeout;
  private resourceGeneration: number;
  private resourceQueries: Map<string, Promise<readonly NativeResourceCatalogItem[]>>;
  private sourceGeneration: number;
  private sourceQueries: Map<string, Promise<NativePackageSourceInfo>>;
  private resourceWatchers: vscode.Disposable[];
  private resourceRefreshTimer?: NodeJS.Timeout;
  private changedResourceSessions?: Set<string>;
  public readonly packageEmitter: vscode.EventEmitter<void>;
  public readonly resourceEmitter: vscode.EventEmitter<void>;
  private readonly packageProvider: PackageTreeProvider;
  private readonly resourceProvider: ResourceTreeProvider;
  private packageView?: vscode.TreeView<SidebarTreeNode>;
  private resourceView?: vscode.TreeView<SidebarTreeNode>;

  public constructor(
    context: vscode.ExtensionContext,
    output: vscode.OutputChannel,
    viewerWorker: ViewerWorkerClient,
    resourceEditors: ResourceCustomEditorProvider,
    compilerController: SidebarCompilerController,
  ) {
    this.context = context;
    this.output = output;
    this.viewerWorker = viewerWorker;
    this.resourceEditors = resourceEditors;
    this.compilerController = compilerController;
    this.packages = [];
    this.packageByRoot = new Map();
    this.activePackage = undefined;
    this.pinnedRoot = context.workspaceState.get(PINNED_PACKAGE_KEY);
    this.packageRefresh = undefined;
    this.packageRefreshRequested = false;
    this.resourceGeneration = 0;
    this.resourceQueries = new Map();
    this.sourceGeneration = 0;
    this.sourceQueries = new Map();
    this.resourceWatchers = [];
    this.resourceRefreshTimer = undefined;
    this.packageEmitter = new vscode.EventEmitter();
    this.resourceEmitter = new vscode.EventEmitter();
    this.packageProvider = new PackageTreeProvider(this);
    this.resourceProvider = new ResourceTreeProvider(this);
  }

  register(): void {
    const manifestWatcher = vscode.workspace.createFileSystemWatcher('**/nwpkg.toml');
    const manifestChanged = () => this.schedulePackageRefresh();
    this.packageView = vscode.window.createTreeView('nwnrs.packages', {
      treeDataProvider: this.packageProvider,
      showCollapseAll: true,
    });
    this.resourceView = vscode.window.createTreeView('nwnrs.resources', {
      treeDataProvider: this.resourceProvider,
      showCollapseAll: true,
    });
    this.context.subscriptions.push(
      this,
      this.packageEmitter,
      this.resourceEmitter,
      this.packageView,
      this.resourceView,
      vscode.commands.registerCommand('nwnrs.sidebar.refreshPackages', () => this.refreshPackages()),
      vscode.commands.registerCommand('nwnrs.sidebar.selectPackage', (root: string) =>
        this.pinPackage(root)),
      vscode.commands.registerCommand('nwnrs.sidebar.unpinPackage', () => this.unpinPackage()),
      vscode.commands.registerCommand('nwnrs.sidebar.refreshResources', () => this.refreshResources()),
      vscode.commands.registerCommand('nwnrs.sidebar.openResource', (node: SidebarTreeNode) =>
        this.openResource(node)),
      vscode.commands.registerCommand('nwnrs.sidebar.openSourceFile', (node: SidebarTreeNode) =>
        this.openSourceFile(node)),
      vscode.commands.registerCommand('nwnrs.sidebar.openSourceArea', (node: SidebarTreeNode) =>
        this.openSourceArea(node)),
      vscode.commands.registerCommand('nwnrs.sidebar.openSourceAreaObject', (node: SidebarTreeNode) =>
        this.openSourceArea(node)),
      vscode.commands.registerCommand('nwnrs.reindexCurrentPackage', () =>
        this.compilerController.reindexCurrentPackage()),
      vscode.commands.registerCommand('nwnrs.restartLanguageService', () =>
        this.compilerController.restartLanguageService()),
      vscode.commands.registerCommand('nwnrs.clearDiagnostics', () =>
        this.compilerController.clearDiagnostics()),
      vscode.commands.registerCommand('nwnrs.openSettings', () =>
        vscode.commands.executeCommand('workbench.action.openSettings', '@ext:nwnrs.nwnrs')),
      vscode.window.onDidChangeActiveTextEditor(() => this.followActiveEditor()),
      vscode.workspace.onDidChangeWorkspaceFolders(() => this.schedulePackageRefresh()),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('nwnrs.rootPath')
            || event.affectsConfiguration('nwnrs.userPath')
            || event.affectsConfiguration('nwnrs.language')
            || event.affectsConfiguration('nwnrs.loadOvr')) {
          this.refreshResources();
        }
      }),
      manifestWatcher,
      manifestWatcher.onDidCreate(manifestChanged),
      manifestWatcher.onDidChange(manifestChanged),
      manifestWatcher.onDidDelete(manifestChanged),
      this.resourceEditors.onDidSelectAreaObject((selection: AreaObjectSelection) => {
        void this.revealAreaObject(selection);
      }),
    );
    void this.refreshPackages();
  }

  dispose(): void {
    clearTimeout(this.packageRefreshTimer);
    clearTimeout(this.resourceRefreshTimer);
    this.disposeResourceWatchers();
  }

  schedulePackageRefresh(): void {
    clearTimeout(this.packageRefreshTimer);
    this.packageRefreshTimer = setTimeout(() => {
      this.packageRefreshTimer = undefined;
      void this.refreshPackages();
    }, 150);
  }

  async refreshPackages(): Promise<void> {
    if (this.packageRefresh) {
      this.packageRefreshRequested = true;
      return this.packageRefresh;
    }
    const refresh = (async () => {
      do {
        this.packageRefreshRequested = false;
        const packages = await this.discoverPackages();
        this.packages = packages;
        this.clearSourceCache();
        this.packageByRoot = new Map(packages.map((entry) => [normalizePath(entry.root), entry]));
        if (this.pinnedRoot && !this.packageByRoot.has(normalizePath(this.pinnedRoot))) {
          this.pinnedRoot = undefined;
          await this.context.workspaceState.update(PINNED_PACKAGE_KEY, undefined);
        }
        await this.selectEffectivePackage();
        this.rebuildResourceWatchers();
        this.packageEmitter.fire();
      } while (this.packageRefreshRequested);
    })().catch((error) => {
      const message = errorMessage(error);
      this.output.appendLine(`nwnrs package discovery failed: ${message}`);
      void vscode.window.showErrorMessage(`nwnrs package discovery failed: ${message}`);
    }).finally(() => {
      if (this.packageRefresh === refresh) this.packageRefresh = undefined;
    });
    this.packageRefresh = refresh;
    return refresh;
  }

  async discoverPackages(): Promise<NativePackageInfo[]> {
    const uris = await vscode.workspace.findFiles(
      '**/nwpkg.toml',
      '**/{.git,node_modules,target}/**',
    );
    const results = await Promise.allSettled(
      uris.map((uri) => this.viewerWorker.inspectPackage(uri.fsPath)),
    );
    const packages: NativePackageInfo[] = [];
    for (let index = 0; index < results.length; index += 1) {
      const result = results[index];
      const uri = uris[index];
      if (!result || !uri) {
        continue;
      }
      if (result.status === 'fulfilled') {
        packages.push(result.value);
      } else {
        this.output.appendLine(
          `Could not inspect ${uri.fsPath}: ${errorMessage(result.reason)}`,
        );
      }
    }
    return packages.sort(comparePackages);
  }

  async ensurePackages(): Promise<NativePackageInfo[]> {
    if (this.packageRefresh) await this.packageRefresh;
    return this.packages;
  }

  async pinPackage(root: string): Promise<void> {
    const selected = this.packageByRoot.get(normalizePath(root));
    if (!selected) {
      void vscode.window.showWarningMessage('That nwnrs package is no longer in this workspace.');
      return;
    }
    this.pinnedRoot = selected.root;
    await this.context.workspaceState.update(PINNED_PACKAGE_KEY, selected.root);
    await this.setActivePackage(selected);
  }

  async unpinPackage(): Promise<void> {
    this.pinnedRoot = undefined;
    await this.context.workspaceState.update(PINNED_PACKAGE_KEY, undefined);
    await this.selectEffectivePackage();
    this.packageEmitter.fire();
  }

  followActiveEditor(): void {
    if (!this.pinnedRoot) void this.selectEffectivePackage();
  }

  async selectEffectivePackage(): Promise<void> {
    const pinned = this.pinnedRoot
      ? this.packageByRoot.get(normalizePath(this.pinnedRoot))
      : undefined;
    const activePath = vscode.window.activeTextEditor?.document?.uri?.scheme === 'file'
      ? vscode.window.activeTextEditor.document.uri.fsPath
      : undefined;
    const selected = pinned || owningPackage(activePath, this.packages) || this.packages[0];
    await this.setActivePackage(selected);
  }

  async setActivePackage(selected: NativePackageInfo | undefined): Promise<void> {
    if (normalizePath(selected?.root) === normalizePath(this.activePackage?.root)) {
      await this.updateContexts();
      return;
    }
    this.activePackage = selected;
    this.clearResourceCache();
    await this.updateContexts();
    this.packageEmitter.fire();
    this.resourceEmitter.fire();
  }

  async updateContexts(): Promise<void> {
    if (this.packageView) {
      this.packageView.description = this.activePackage
        ? `${this.activePackage.name}${this.pinnedRoot ? ' • pinned' : ''}`
        : undefined;
    }
    if (this.resourceView) {
      this.resourceView.description = this.activePackage?.name || 'Vanilla';
    }
    await Promise.all([
      vscode.commands.executeCommand('setContext', 'nwnrs.hasPackages', this.packages.length > 0),
      vscode.commands.executeCommand('setContext', 'nwnrs.hasActivePackage', Boolean(this.activePackage)),
      vscode.commands.executeCommand('setContext', 'nwnrs.packagePinned', Boolean(this.pinnedRoot)),
    ]);
  }

  clearResourceCache(): void {
    this.resourceGeneration += 1;
    this.resourceQueries.clear();
  }

  clearSourceCache(): void {
    this.sourceGeneration += 1;
    this.sourceQueries.clear();
    this.packageProvider?.clearCache();
  }

  refreshResources(): void {
    this.viewerWorker.invalidate(this.activePackage?.manifestPath);
    this.clearResourceCache();
    this.resourceEmitter.fire();
  }

  scheduleResourceRefresh(sessionKey: string): void {
    if (!this.changedResourceSessions) this.changedResourceSessions = new Set();
    this.changedResourceSessions.add(sessionKey);
    clearTimeout(this.resourceRefreshTimer);
    this.resourceRefreshTimer = setTimeout(() => {
      this.resourceRefreshTimer = undefined;
      const changedSessions = this.changedResourceSessions;
      if (changedSessions) {
        for (const key of changedSessions) this.viewerWorker.invalidate(key);
        changedSessions.clear();
      }
      this.clearResourceCache();
      this.clearSourceCache();
      this.packageEmitter.fire();
      this.resourceEmitter.fire();
    }, 150);
  }

  disposeResourceWatchers(): void {
    for (const watcher of this.resourceWatchers) watcher.dispose();
    this.resourceWatchers = [];
  }

  rebuildResourceWatchers(): void {
    this.disposeResourceWatchers();
    const watched = new Set<string>();
    for (const packageInfo of this.packages) {
      const roots = packageInfo.resourcePaths || [packageInfo.sourcePath];
      for (const root of roots) {
        if (!root || watched.has(`${packageInfo.manifestPath}\0${normalizePath(root)}`)) continue;
        watched.add(`${packageInfo.manifestPath}\0${normalizePath(root)}`);
        if (!fs.existsSync(root)) continue;
        const watcher = vscode.workspace.createFileSystemWatcher(
          new vscode.RelativePattern(root, '**/*'),
        );
        const changed = () => this.scheduleResourceRefresh(packageInfo.manifestPath);
        watcher.onDidCreate(changed);
        watcher.onDidChange(changed);
        watcher.onDidDelete(changed);
        this.resourceWatchers.push(watcher);
      }
    }
  }

  resourceRequest(packageInfo: NativePackageInfo | undefined = this.activePackage): ResourceRequest {
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
      || this.context.extensionPath;
    const projectRoot = packageInfo?.root || workspaceRoot;
    const sourceRoot = packageInfo?.sourcePath || workspaceRoot;
    const scopeUri = packageInfo ? vscode.Uri.file(packageInfo.manifestPath) : undefined;
    const configuration = vscode.workspace.getConfiguration('nwnrs', scopeUri);
    const context = {
      projectRoot,
      workspaceFolder: vscode.workspace.getWorkspaceFolder(scopeUri || vscode.Uri.file(projectRoot))
        ?.uri.fsPath || workspaceRoot,
      fileDirname: projectRoot,
    };
    return {
      session_key: packageInfo?.manifestPath || `nwnrs-sidebar:${normalizePath(projectRoot)}`,
      path: path.join(sourceRoot, '.nwnrs-resource-catalog'),
      project_root: projectRoot,
      area: null,
      root: resolveConfiguredPath(configuration.get('rootPath', ''), context, projectRoot) || null,
      user: resolveConfiguredPath(configuration.get('userPath', ''), context, projectRoot) || null,
      language: configuration.get('language', 'english'),
      load_ovr: configuration.get('loadOvr', false),
      archives: [],
      include_project_resources: Boolean(packageInfo),
    };
  }

  async listResources(query: ResourceQuery): Promise<readonly NativeResourceCatalogItem[]> {
    const request = this.resourceRequest();
    const cacheKey = `${this.resourceGeneration}:${request.session_key}:${JSON.stringify(query)}`;
    let pending = this.resourceQueries.get(cacheKey);
    if (!pending) {
      pending = this.viewerWorker.listResources({ ...request, ...query })
        .then((response) => response?.items || [])
        .catch((error) => {
          this.resourceQueries.delete(cacheKey);
          throw error;
        });
      this.resourceQueries.set(cacheKey, pending);
    }
    return pending;
  }

  async packageSource(packageInfo: NativePackageInfo): Promise<NativePackageSourceInfo> {
    const key = `${this.sourceGeneration}:${packageInfo.manifestPath}`;
    let pending = this.sourceQueries.get(key);
    if (!pending) {
      pending = this.viewerWorker.inspectPackageSource(packageInfo.manifestPath)
        .then((catalog) => {
          for (const warning of catalog.warnings || []) {
            this.output.appendLine(`[source] ${packageInfo.name}: ${warning}`);
          }
          for (const area of catalog.areas || []) {
            if (area.objectError) {
              this.output.appendLine(
                `[source] ${packageInfo.name}/${area.resref}: ${area.objectError}`,
              );
            }
          }
          return catalog;
        })
        .catch((error) => {
          this.sourceQueries.delete(key);
          throw error;
        });
      this.sourceQueries.set(key, pending);
    }
    return pending;
  }

  async openSourceFile(node: SidebarTreeNode): Promise<void> {
    const file = node.file;
    if (!file?.path) return;
    const uri = vscode.Uri.file(file.path);
    try {
      if (file.kind === 'dlg' || file.kind === 'dlgJson') {
        await vscode.commands.executeCommand('vscode.openWith', uri, VIEW_TYPE);
      } else {
        await vscode.commands.executeCommand('vscode.open', uri);
      }
    } catch (error) {
      const message = errorMessage(error);
      this.output.appendLine(`Could not open ${file.path}: ${message}`);
      void vscode.window.showErrorMessage(
        `Could not open ${path.basename(file.path)}: ${message}`,
      );
    }
  }

  async openSourceArea(node: SidebarTreeNode): Promise<void> {
    const area = node.area;
    if (!area) return;
    const conflicts = area.conflicts || [];
    const missingRequired = (area.missing || [])
      .filter((kind) => kind === 'ARE' || kind === 'GIT');
    if (conflicts.length || missingRequired.length) {
      const problems = [
        conflicts.length ? `duplicate ${conflicts.join(', ')}` : '',
        missingRequired.length ? `missing ${missingRequired.join(', ')}` : '',
      ].filter(Boolean).join('; ');
      void vscode.window.showErrorMessage(
        `Cannot render area ${area.resref}: ${problems}.`,
      );
      return;
    }
    try {
      await this.resourceEditors.openAuthoredArea(
        this.resourceRequest(node.package),
        area,
        node.object?.key,
      );
    } catch (error) {
      const message = errorMessage(error);
      this.output.appendLine(
        `Could not render area ${area.resref}: ${message}`,
      );
      void vscode.window.showErrorMessage(
        `Could not render area ${area.resref}: ${message}`,
      );
    }
  }

  async revealAreaObject(selection: AreaObjectSelection): Promise<void> {
    if (!selection?.objectKey || !this.packageView) return;
    try {
      const node = await this.packageProvider.findAreaObject(selection);
      if (node) await this.packageView.reveal(node, { select: true, focus: false, expand: true });
    } catch (error) {
      this.output.appendLine(`Could not reveal selected area object: ${errorMessage(error)}`);
    }
  }

  async openResource(node: SidebarTreeNode): Promise<void> {
    if (!node?.resource) return;
    try {
      await this.resourceEditors.openResolvedResource(this.resourceRequest(), node.resource);
    } catch (error) {
      const message = errorMessage(error);
      this.output.appendLine(`Could not open ${node.resource}: ${message}`);
      void vscode.window.showErrorMessage(`Could not open ${node.resource}: ${message}`);
    }
  }
}

class PackageTreeProvider implements vscode.TreeDataProvider<SidebarTreeNode> {
  private readonly controller: NwnrsSidebarController;
  public readonly onDidChangeTreeData: vscode.Event<void>;
  private readonly packageNodes: Map<string, SidebarTreeNode>;

  constructor(controller: NwnrsSidebarController) {
    this.controller = controller;
    this.onDidChangeTreeData = controller.packageEmitter.event;
    this.packageNodes = new Map();
  }

  getTreeItem(node: SidebarTreeNode): vscode.TreeItem {
    const state = vscode.TreeItemCollapsibleState;
    if (node.kind === 'package' && node.package) {
      const selected = normalizePath(node.package.root)
        === normalizePath(this.controller.activePackage?.root);
      const item = new vscode.TreeItem(
        node.package.name,
        selected ? state.Expanded : state.Collapsed,
      );
      item.description = `${node.package.kind}${selected ? ' • active' : ''}`;
      item.tooltip = `${node.package.name}\n${node.package.root}${this.controller.pinnedRoot
        && selected ? '\nPinned' : ''}`;
      item.iconPath = new vscode.ThemeIcon('package');
      item.contextValue = selected ? 'nwnrsPackageActive' : 'nwnrsPackage';
      item.command = {
        command: 'nwnrs.sidebar.selectPackage',
        title: 'Select Package',
        arguments: [node.package.root],
      };
      return item;
    }
    if (node.kind === 'dependencyGroup') {
      const dependencies = node.dependencies || [];
      const item = new vscode.TreeItem(
        'Dependencies',
        dependencies.length ? state.Collapsed : state.None,
      );
      item.description = String(dependencies.length);
      item.iconPath = new vscode.ThemeIcon('references');
      return item;
    }
    if (node.kind === 'sourceRoot' && node.package) {
      const item = new vscode.TreeItem('Source', state.Collapsed);
      item.description = relativeDisplay(node.package.root, node.package.sourcePath);
      item.tooltip = node.package.sourcePath;
      item.iconPath = new vscode.ThemeIcon('folder-library');
      return item;
    }
    if (node.kind === 'sourceSection') {
      const item = new vscode.TreeItem(node.label, state.Collapsed);
      item.description = formatCount(node.count);
      item.iconPath = new vscode.ThemeIcon(sourceSectionIcon(node.section));
      return item;
    }
    if (node.kind === 'sourceFolder') {
      const item = new vscode.TreeItem(node.label, state.Collapsed);
      item.iconPath = new vscode.ThemeIcon('folder');
      return item;
    }
    if (node.kind === 'sourceUnregistered') {
      const item = new vscode.TreeItem('Unregistered Areas', state.Collapsed);
      item.description = formatCount(node.count);
      item.tooltip = 'Area sources that are not declared by module.ifo';
      item.iconPath = new vscode.ThemeIcon('warning');
      return item;
    }
    if (node.kind === 'sourceArea' && node.area) {
      const item = new vscode.TreeItem(
        node.area.resref,
        node.children?.length ? state.Collapsed : state.None,
      );
      const status = [];
      if (!node.area.registered) status.push('unregistered');
      if (node.area.missing.length) status.push(`missing ${node.area.missing.join(', ')}`);
      if (node.area.conflicts?.length) {
        status.push(`duplicate ${node.area.conflicts.join(', ')}`);
      }
      item.description = status.join(' • ') || undefined;
      const canRender = !node.area.conflicts?.length
        && !(node.area.missing || []).some((kind) => kind === 'ARE' || kind === 'GIT');
      item.tooltip = [
        `Area ${node.area.resref}`,
        node.area.registered ? '' : 'Not declared by module.ifo',
        node.area.missing?.includes('GIC') ? 'GIC is missing; visual preview remains available' : '',
        canRender ? 'Open read-only area preview' : 'ARE and GIT must be present and unambiguous',
        node.area.objectError ? `Objects unavailable: ${node.area.objectError}` : '',
      ].filter(Boolean).join('\n');
      item.iconPath = new vscode.ThemeIcon(status.length ? 'warning' : 'map');
      item.contextValue = canRender ? 'nwnrsSourceArea' : 'nwnrsSourceAreaInvalid';
      if (canRender) {
        item.command = {
          command: 'nwnrs.sidebar.openSourceArea',
          title: 'Open Area Preview',
          arguments: [node],
        };
      }
      return item;
    }
    if (node.kind === 'sourceAreaObjectGroup') {
      const item = new vscode.TreeItem(node.label, state.Collapsed);
      item.description = formatCount(node.children?.length);
      item.iconPath = new vscode.ThemeIcon(areaObjectIcon(node.objectKind));
      return item;
    }
    if (node.kind === 'sourceAreaObject' && node.object) {
      const item = new vscode.TreeItem(node.object.label, state.None);
      const metadata = [node.object.tag, node.object.templateResref].filter(Boolean);
      item.description = metadata.join(' · ') || undefined;
      item.tooltip = [
        node.object.label,
        node.object.tag ? `Tag: ${node.object.tag}` : '',
        node.object.templateResref ? `Blueprint: ${node.object.templateResref}` : '',
        `GIT ${node.objectKind} #${node.object.sourceIndex}`,
      ].filter(Boolean).join('\n');
      item.iconPath = new vscode.ThemeIcon(areaObjectIcon(node.objectKind));
      item.contextValue = 'nwnrsSourceAreaObject';
      item.command = {
        command: 'nwnrs.sidebar.openSourceAreaObject',
        title: 'Focus Area Object',
        arguments: [node],
      };
      return item;
    }
    if (node.kind === 'sourceFile' && node.file) {
      const item = new vscode.TreeItem(node.label, state.None);
      item.resourceUri = vscode.Uri.file(node.file.path);
      item.tooltip = node.file.path;
      item.description = sourceFileDescription(node.file.kind);
      item.iconPath = new vscode.ThemeIcon(sourceFileIcon(node.file.kind));
      item.command = {
        command: 'nwnrs.sidebar.openSourceFile',
        title: 'Open Source File',
        arguments: [node],
      };
      return item;
    }
    const item = new vscode.TreeItem(node.label, state.None);
    item.description = node.description;
    item.tooltip = node.tooltip;
    item.iconPath = new vscode.ThemeIcon(node.icon || 'file');
    if (node.uri && node.command) {
      item.resourceUri = node.uri;
      item.command = { command: node.command, title: node.label, arguments: [node.uri] };
    }
    return item;
  }

  clearCache(): void {
    this.packageNodes.clear();
  }

  async getChildren(node?: SidebarTreeNode): Promise<SidebarTreeNode[]> {
    await this.controller.ensurePackages();
    if (!node) {
      const current = new Set(this.controller.packages.map((packageInfo) => packageInfo.manifestPath));
      for (const key of this.packageNodes.keys()) if (!current.has(key)) this.packageNodes.delete(key);
      return this.controller.packages.map((packageInfo) => {
        let packageNode = this.packageNodes.get(packageInfo.manifestPath);
        if (!packageNode || packageNode.package !== packageInfo) {
          packageNode = { kind: 'package', label: packageInfo.name, package: packageInfo };
          this.packageNodes.set(packageInfo.manifestPath, packageNode);
        }
        return packageNode;
      });
    }
    if (node.kind === 'package') {
      if (node.loadedChildren) return node.loadedChildren;
      const packageInfo = node.package;
      if (!packageInfo) return [];
      const children: SidebarTreeNode[] = [{
        kind: 'file',
        label: 'Manifest',
        description: path.basename(packageInfo.manifestPath),
        tooltip: packageInfo.manifestPath,
        icon: 'settings-gear',
        uri: vscode.Uri.file(packageInfo.manifestPath),
        command: 'vscode.open',
      }];
      if (packageInfo.sourcePath) {
        children.push({
          kind: 'sourceRoot',
          label: 'Source',
          package: packageInfo,
        });
      }
      children.push({
        kind: 'dependencyGroup',
        label: 'Dependencies',
        packageRoot: packageInfo.root,
        dependencies: packageInfo.dependencies || [],
      });
      node.loadedChildren = attachParents(children, node);
      return node.loadedChildren;
    }
    if (node.kind === 'sourceRoot') {
      if (node.loadedChildren) return node.loadedChildren;
      try {
        const packageInfo = node.package;
        if (!packageInfo) return [];
        const catalog = await this.controller.packageSource(packageInfo);
        node.loadedChildren = attachParents(sourceSections(catalog, packageInfo), node);
        return node.loadedChildren;
      } catch (error) {
        this.controller.output.appendLine(
          `Could not inspect ${node.package?.sourcePath || node.label}: ${errorMessage(error)}`,
        );
        return [];
      }
    }
    if (node.kind === 'sourceSection' || node.kind === 'sourceFolder'
        || node.kind === 'sourceUnregistered' || node.kind === 'sourceArea'
        || node.kind === 'sourceAreaObjectGroup') {
      return attachParents(node.children || [], node);
    }
    if (node.kind === 'dependencyGroup') {
      return (node.dependencies || []).map((dependency) => ({
        kind: 'file',
        label: dependency.name,
        description: relativeDisplay(node.packageRoot, dependency.root),
        tooltip: dependency.root,
        icon: 'package',
        uri: vscode.Uri.file(dependency.manifestPath),
        command: 'vscode.open',
      }));
    }
    return [];
  }

  getParent(node: SidebarTreeNode): SidebarTreeNode | undefined {
    return node?.parent;
  }

  async findAreaObject(selection: AreaObjectSelection): Promise<SidebarTreeNode | undefined> {
    const packages = await this.getChildren();
    const packageNode = packages.find((node) => node.package
      && normalizePath(node.package.manifestPath)
      === normalizePath(selection.manifestPath));
    if (!packageNode) return undefined;
    const packageChildren = await this.getChildren(packageNode);
    const sourceRoot = packageChildren.find((node) => node.kind === 'sourceRoot');
    if (!sourceRoot) return undefined;
    const roots = await this.getChildren(sourceRoot);
    return findTreeNode(roots, (node) => node.kind === 'sourceAreaObject'
      && node.area?.resref.toLowerCase() === selection.resref.toLowerCase()
      && node.object?.key === selection.objectKey);
  }
}

class ResourceTreeProvider implements vscode.TreeDataProvider<SidebarTreeNode> {
  private readonly controller: NwnrsSidebarController;
  public readonly onDidChangeTreeData: vscode.Event<void>;

  constructor(controller: NwnrsSidebarController) {
    this.controller = controller;
    this.onDidChangeTreeData = controller.resourceEmitter.event;
  }

  getTreeItem(node: SidebarTreeNode): vscode.TreeItem {
    const expandable = node.kind !== 'resource' && node.kind !== 'error';
    const item = new vscode.TreeItem(
      node.label,
      expandable ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None,
    );
    if ((node.count || 0) > 1 || expandable) item.description = formatCount(node.count);
    item.contextValue = `nwnrsResource.${node.kind}`;
    item.iconPath = new vscode.ThemeIcon(resourceIcon(node));
    if (node.kind === 'error') {
      item.tooltip = node.error;
      item.description = 'See nwnrs Compiler output';
    }
    if (node.kind === 'resource') {
      item.tooltip = node.origin ? `${node.resource}\n${node.origin}` : node.resource;
      if (this.controller.resourceEditors.canOpenResource(node.resource, node.filePath)) {
        item.command = {
          command: 'nwnrs.sidebar.openResource',
          title: 'Open Resource',
          arguments: [node],
        };
      } else {
        item.description = 'unsupported';
      }
    }
    return item;
  }

  async getChildren(node?: SidebarTreeNode): Promise<SidebarTreeNode[]> {
    try {
      if (!node) {
        await this.controller.ensurePackages();
        let layers = await this.controller.listResources({ stage: 'layers' });
        if (!this.controller.activePackage) {
          layers = layers.filter((item) => item.layer === 'Vanilla');
        }
        return sortResourceLayers(layers).map(resourceNode);
      }
      const query = childResourceQuery(node);
      if (!query) return [];
      const items = await this.controller.listResources(query);
      return items.map((item) => resourceNode({
        ...item,
        layer: item.layer || node.layer,
        family: item.family || node.family,
        extension: item.extension || node.extension,
      }));
    } catch (error) {
      const message = errorMessage(error);
      this.controller.output.appendLine(`Could not list nwnrs resources: ${message}`);
      return [{
        kind: 'error',
        label: 'Resources unavailable',
        count: 0,
        error: message,
      }];
    }
  }
}

function comparePackages(left: NativePackageInfo, right: NativePackageInfo): number {
  return left.name.localeCompare(right.name, undefined, { sensitivity: 'base' })
    || left.root.localeCompare(right.root);
}

function sourceSections(
  catalog: NativePackageSourceInfo,
  packageInfo: NativePackageInfo,
): SidebarTreeNode[] {
  const sections: SidebarTreeNode[] = [];
  if (catalog.areas?.length) {
    const registered = catalog.areas.filter((area) => area.registered);
    const unregistered = catalog.areas.filter((area) => !area.registered);
    const children = buildAreaPathTree(registered, packageInfo);
    if (unregistered.length) {
      children.push({
        kind: 'sourceUnregistered',
        label: 'Unregistered Areas',
        count: unregistered.length,
        children: buildAreaPathTree(unregistered, packageInfo),
      });
    }
    sections.push({
      kind: 'sourceSection',
      section: 'areas',
      label: 'Areas',
      count: catalog.areas.length,
      children,
    });
  }
  if (catalog.dialogs?.length) {
    sections.push({
      kind: 'sourceSection',
      section: 'dialogs',
      label: 'Dialogs',
      count: catalog.dialogs.length,
      children: buildSourceFileTree(catalog.dialogs),
    });
  }
  if (catalog.code?.length) {
    sections.push({
      kind: 'sourceSection',
      section: 'code',
      label: 'Code',
      count: catalog.code.length,
      children: buildSourceFileTree(catalog.code),
    });
  }
  return sections;
}

function buildAreaPathTree(
  areas: readonly NativePackageSourceArea[],
  packageInfo: NativePackageInfo,
): SidebarTreeNode[] {
  const entries = areas.map((area) => {
    const baseDirectory = commonDirectory(
      area.files.map((file) => path.posix.dirname(normalizeRelativePath(file.relativePath))),
    );
    const objectGroups = areaObjectGroups(area, packageInfo);
    return {
      directory: baseDirectory,
      node: {
        kind: 'sourceArea',
        label: area.resref,
        area,
        package: packageInfo,
        ...(objectGroups.length ? { children: objectGroups } : {}),
      },
    };
  });
  return buildDirectoryNodes(entries);
}

const AREA_OBJECT_GROUPS = [
  ['creature', 'Creatures'],
  ['door', 'Doors'],
  ['placeable', 'Placeables'],
  ['encounter', 'Encounters'],
  ['sound', 'Sounds'],
  ['store', 'Stores'],
  ['trigger', 'Triggers'],
  ['waypoint', 'Waypoints'],
] as const;

function areaObjectGroups(
  area: NativePackageSourceArea,
  packageInfo: NativePackageInfo,
): SidebarTreeNode[] {
  return AREA_OBJECT_GROUPS.map(([objectKind, label]): SidebarTreeNode => {
    const objects = (area.objects || []).filter((object) => object.kind === objectKind);
    return {
      kind: 'sourceAreaObjectGroup',
      objectKind,
      label,
      children: objects.map((object) => ({
        kind: 'sourceAreaObject',
        label: object.label,
        objectKind,
        object,
        area,
        package: packageInfo,
      })),
    };
  }).filter((group) => (group.children?.length || 0) > 0);
}

function areaObjectIcon(kind: string | undefined): string {
  const icons: Readonly<Record<string, string>> = {
    creature: 'person',
    door: 'layout-sidebar-left',
    placeable: 'symbol-object',
    encounter: 'pulse',
    sound: 'unmute',
    store: 'store',
    trigger: 'symbol-event',
    waypoint: 'location',
  };
  return kind ? icons[kind] || 'symbol-object' : 'symbol-object';
}

function attachParents(
  children: SidebarTreeNode[],
  parent: SidebarTreeNode,
): SidebarTreeNode[] {
  for (const child of children) {
    child.parent = parent;
    if (child.children) attachParents(child.children, child);
  }
  return children;
}

function findTreeNode(
  nodes: readonly SidebarTreeNode[],
  predicate: (node: SidebarTreeNode) => boolean,
): SidebarTreeNode | undefined {
  for (const node of nodes) {
    if (predicate(node)) return node;
    const match: SidebarTreeNode | undefined = findTreeNode(node.children || [], predicate);
    if (match) return match;
  }
  return undefined;
}

function buildSourceFileTree(
  files: readonly NativePackageSourceFile[],
  baseDirectory = '',
): SidebarTreeNode[] {
  const normalizedBase = normalizeRelativePath(baseDirectory);
  const entries = files.map((file) => {
    const relative = normalizeRelativePath(file.relativePath);
    const scoped = normalizedBase && relative.startsWith(`${normalizedBase}/`)
      ? relative.slice(normalizedBase.length + 1)
      : relative;
    return {
      directory: path.posix.dirname(scoped) === '.' ? '' : path.posix.dirname(scoped),
      node: {
        kind: 'sourceFile',
        label: path.posix.basename(relative),
        file,
      },
    };
  });
  return buildDirectoryNodes(entries);
}

function buildDirectoryNodes(entries: readonly DirectoryEntry[]): SidebarTreeNode[] {
  const root: DirectoryBranch = { directories: new Map(), leaves: [] };
  for (const entry of entries) {
    let current = root;
    const segments = normalizeRelativePath(entry.directory).split('/').filter(Boolean);
    for (const segment of segments) {
      let next = current.directories.get(segment);
      if (!next) {
        next = { directories: new Map(), leaves: [] };
        current.directories.set(segment, next);
      }
      current = next;
    }
    current.leaves.push(entry.node);
  }
  const materialize = (branch: DirectoryBranch): SidebarTreeNode[] => [
    ...[...branch.directories.entries()]
      .sort(([left], [right]) => left.localeCompare(right, undefined, { sensitivity: 'base' }))
      .map(([label, child]) => ({
        kind: 'sourceFolder',
        label,
        children: materialize(child),
      })),
    ...branch.leaves.sort((left, right) =>
      left.label.localeCompare(right.label, undefined, { sensitivity: 'base' })),
  ];
  return materialize(root);
}

function commonDirectory(directories: readonly string[]): string {
  if (directories.length === 0) return '';
  const split = directories.map((directory) =>
    normalizeRelativePath(directory === '.' ? '' : directory).split('/').filter(Boolean));
  const shared = [];
  const first = split[0];
  if (!first) return '';
  for (let index = 0; index < first.length; index += 1) {
    const segment = first[index];
    if (segment === undefined) break;
    if (!split.every((parts) => parts[index] === segment)) break;
    shared.push(segment);
  }
  return shared.join('/');
}

function normalizeRelativePath(value: string): string {
  return String(value || '').replaceAll('\\', '/').replace(/^\.\//u, '').replace(/\/$/u, '');
}

function sourceSectionIcon(section: string | undefined): string {
  if (section === 'areas') return 'map';
  if (section === 'dialogs') return 'comment-discussion';
  return 'code';
}

function sourceFileIcon(kind: string): string {
  if (kind === 'nss') return 'code';
  if (kind === 'dlg' || kind === 'dlgJson') return 'comment-discussion';
  return 'file';
}

function sourceFileDescription(kind: string): string {
  if (kind === 'dlgJson') return 'DLG JSON';
  return kind.toUpperCase();
}

function normalizePath(value: string | undefined): string {
  if (!value) return '';
  let normalized = path.resolve(value);
  try {
    normalized = fs.realpathSync.native(normalized);
  } catch {
    // Missing paths still need stable lexical identity while manifests are edited.
  }
  return process.platform === 'win32' ? normalized.toLowerCase() : normalized;
}

function pathContains(root: string | undefined, candidate: string | undefined): boolean {
  if (!root || !candidate) return false;
  const relative = path.relative(normalizePath(root), normalizePath(candidate));
  return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

function owningPackage(
  filePath: string | undefined,
  packages: readonly NativePackageInfo[],
): NativePackageInfo | undefined {
  if (!filePath) return undefined;
  return packages
    .filter((packageInfo) => pathContains(packageInfo.root, filePath))
    .sort((left, right) => normalizePath(right.root).length - normalizePath(left.root).length)[0];
}

function relativeDisplay(root: string | undefined, target: string | undefined): string {
  if (!root || !target) return target || '';
  const relative = path.relative(root, target);
  return relative && !relative.startsWith('..') ? relative : target;
}

function sortResourceLayers(
  items: readonly NativeResourceCatalogItem[],
): NativeResourceCatalogItem[] {
  return [...items].sort((left, right) => {
    const leftIndex = RESOURCE_LAYER_ORDER.indexOf(left.layer || left.label);
    const rightIndex = RESOURCE_LAYER_ORDER.indexOf(right.layer || right.label);
    return (leftIndex < 0 ? RESOURCE_LAYER_ORDER.length : leftIndex)
      - (rightIndex < 0 ? RESOURCE_LAYER_ORDER.length : rightIndex)
      || left.label.localeCompare(right.label);
  });
}

function resourceNode(item: NativeResourceCatalogItem): SidebarTreeNode {
  return {
    ...item,
    filePath: item.filePath,
  };
}

function childResourceQuery(node: SidebarTreeNode): ResourceQuery | undefined {
  const shared = {
    layer: node.layer,
    family: node.family,
    extension: node.extension,
  };
  switch (node.kind) {
    case 'layer': return { stage: 'families', layer: node.layer };
    case 'family': return { stage: 'types', layer: node.layer, family: node.family };
    case 'extension': return { stage: 'names', ...shared, prefix: '' };
    case 'prefix': return { stage: 'names', ...shared, prefix: node.prefix };
    default: return undefined;
  }
}

function resourceIcon(node: SidebarTreeNode): string {
  switch (node.kind) {
    case 'layer': return node.layer === 'Vanilla' ? 'library' : 'layers';
    case 'family': return 'folder';
    case 'extension': return 'symbol-file';
    case 'prefix': return 'list-tree';
    case 'error': return 'warning';
    default: return node.filePath ? 'file' : 'lock';
  }
}

function formatCount(count: number | undefined): string {
  return Number(count || 0).toLocaleString();
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export {
  buildSourceFileTree,
  childResourceQuery,
  owningPackage,
  sourceSections,
  sortResourceLayers,
};
