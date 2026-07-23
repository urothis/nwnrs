'use strict';

const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
const { resolveConfiguredPath } = require('./compiler');
const { VIEW_TYPE } = require('./resource-custom-editor');

const PINNED_PACKAGE_KEY = 'nwnrs.sidebar.pinnedPackage';
const RESOURCE_LAYER_ORDER = [
  'Workspace',
  'Package Dependencies',
  'User Override',
  'Archives',
  'Vanilla',
];

class NwnrsSidebarController {
  constructor(context, output, viewerWorker, resourceEditors, compilerController) {
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

  register() {
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
      vscode.commands.registerCommand('nwnrs.sidebar.selectPackage', (root) => this.pinPackage(root)),
      vscode.commands.registerCommand('nwnrs.sidebar.unpinPackage', () => this.unpinPackage()),
      vscode.commands.registerCommand('nwnrs.sidebar.refreshResources', () => this.refreshResources()),
      vscode.commands.registerCommand('nwnrs.sidebar.openResource', (node) => this.openResource(node)),
      vscode.commands.registerCommand('nwnrs.sidebar.openSourceFile', (node) =>
        this.openSourceFile(node)),
      vscode.commands.registerCommand('nwnrs.sidebar.openSourceArea', (node) =>
        this.openSourceArea(node)),
      vscode.commands.registerCommand('nwnrs.sidebar.openSourceAreaObject', (node) =>
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
      this.resourceEditors.onDidSelectAreaObject((selection) => {
        void this.revealAreaObject(selection);
      }),
    );
    void this.refreshPackages();
  }

  dispose() {
    clearTimeout(this.packageRefreshTimer);
    clearTimeout(this.resourceRefreshTimer);
    this.disposeResourceWatchers();
  }

  schedulePackageRefresh() {
    clearTimeout(this.packageRefreshTimer);
    this.packageRefreshTimer = setTimeout(() => {
      this.packageRefreshTimer = undefined;
      void this.refreshPackages();
    }, 150);
  }

  async refreshPackages() {
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
      this.output.appendLine(`nwnrs package discovery failed: ${error.message || error}`);
      void vscode.window.showErrorMessage(`nwnrs package discovery failed: ${error.message || error}`);
    }).finally(() => {
      if (this.packageRefresh === refresh) this.packageRefresh = undefined;
    });
    this.packageRefresh = refresh;
    return refresh;
  }

  async discoverPackages() {
    const uris = await vscode.workspace.findFiles(
      '**/nwpkg.toml',
      '**/{.git,node_modules,target}/**',
    );
    const results = await Promise.allSettled(
      uris.map((uri) => this.viewerWorker.inspectPackage(uri.fsPath)),
    );
    const packages = [];
    for (let index = 0; index < results.length; index += 1) {
      const result = results[index];
      if (result.status === 'fulfilled') {
        packages.push(result.value);
      } else {
        this.output.appendLine(
          `Could not inspect ${uris[index].fsPath}: ${result.reason?.message || result.reason}`,
        );
      }
    }
    return packages.sort(comparePackages);
  }

  async ensurePackages() {
    if (this.packageRefresh) await this.packageRefresh;
    return this.packages;
  }

  async pinPackage(root) {
    const selected = this.packageByRoot.get(normalizePath(root));
    if (!selected) {
      void vscode.window.showWarningMessage('That nwnrs package is no longer in this workspace.');
      return;
    }
    this.pinnedRoot = selected.root;
    await this.context.workspaceState.update(PINNED_PACKAGE_KEY, selected.root);
    await this.setActivePackage(selected);
  }

  async unpinPackage() {
    this.pinnedRoot = undefined;
    await this.context.workspaceState.update(PINNED_PACKAGE_KEY, undefined);
    await this.selectEffectivePackage();
    this.packageEmitter.fire();
  }

  followActiveEditor() {
    if (!this.pinnedRoot) void this.selectEffectivePackage();
  }

  async selectEffectivePackage() {
    const pinned = this.pinnedRoot
      ? this.packageByRoot.get(normalizePath(this.pinnedRoot))
      : undefined;
    const activePath = vscode.window.activeTextEditor?.document?.uri?.scheme === 'file'
      ? vscode.window.activeTextEditor.document.uri.fsPath
      : undefined;
    const selected = pinned || owningPackage(activePath, this.packages) || this.packages[0];
    await this.setActivePackage(selected);
  }

  async setActivePackage(selected) {
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

  async updateContexts() {
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

  clearResourceCache() {
    this.resourceGeneration += 1;
    this.resourceQueries.clear();
  }

  clearSourceCache() {
    this.sourceGeneration += 1;
    this.sourceQueries.clear();
    this.packageProvider?.clearCache();
  }

  refreshResources() {
    this.viewerWorker.invalidate(this.activePackage?.manifestPath);
    this.clearResourceCache();
    this.resourceEmitter.fire();
  }

  scheduleResourceRefresh(sessionKey) {
    if (!this.changedResourceSessions) this.changedResourceSessions = new Set();
    this.changedResourceSessions.add(sessionKey);
    clearTimeout(this.resourceRefreshTimer);
    this.resourceRefreshTimer = setTimeout(() => {
      this.resourceRefreshTimer = undefined;
      for (const key of this.changedResourceSessions) this.viewerWorker.invalidate(key);
      this.changedResourceSessions.clear();
      this.clearResourceCache();
      this.clearSourceCache();
      this.packageEmitter.fire();
      this.resourceEmitter.fire();
    }, 150);
  }

  disposeResourceWatchers() {
    for (const watcher of this.resourceWatchers) watcher.dispose();
    this.resourceWatchers = [];
  }

  rebuildResourceWatchers() {
    this.disposeResourceWatchers();
    const watched = new Set();
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

  resourceRequest(packageInfo = this.activePackage) {
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

  async listResources(query) {
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

  async packageSource(packageInfo) {
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

  async openSourceFile(node) {
    if (!node?.file?.path) return;
    const uri = vscode.Uri.file(node.file.path);
    try {
      if (node.file.kind === 'dlg' || node.file.kind === 'dlgJson') {
        await vscode.commands.executeCommand('vscode.openWith', uri, VIEW_TYPE);
      } else {
        await vscode.commands.executeCommand('vscode.open', uri);
      }
    } catch (error) {
      this.output.appendLine(`Could not open ${node.file.path}: ${error.message || error}`);
      void vscode.window.showErrorMessage(
        `Could not open ${path.basename(node.file.path)}: ${error.message || error}`,
      );
    }
  }

  async openSourceArea(node) {
    if (!node?.area) return;
    const conflicts = node.area.conflicts || [];
    const missingRequired = (node.area.missing || [])
      .filter((kind) => kind === 'ARE' || kind === 'GIT');
    if (conflicts.length || missingRequired.length) {
      const problems = [
        conflicts.length ? `duplicate ${conflicts.join(', ')}` : '',
        missingRequired.length ? `missing ${missingRequired.join(', ')}` : '',
      ].filter(Boolean).join('; ');
      void vscode.window.showErrorMessage(
        `Cannot render area ${node.area.resref}: ${problems}.`,
      );
      return;
    }
    try {
      await this.resourceEditors.openAuthoredArea(
        this.resourceRequest(node.package),
        node.area,
        node.object?.key,
      );
    } catch (error) {
      this.output.appendLine(
        `Could not render area ${node.area.resref}: ${error.message || error}`,
      );
      void vscode.window.showErrorMessage(
        `Could not render area ${node.area.resref}: ${error.message || error}`,
      );
    }
  }

  async revealAreaObject(selection) {
    if (!selection?.objectKey || !this.packageView) return;
    try {
      const node = await this.packageProvider.findAreaObject(selection);
      if (node) await this.packageView.reveal(node, { select: true, focus: false, expand: true });
    } catch (error) {
      this.output.appendLine(`Could not reveal selected area object: ${error.message || error}`);
    }
  }

  async openResource(node) {
    if (!node?.resource) return;
    try {
      await this.resourceEditors.openResolvedResource(this.resourceRequest(), node.resource);
    } catch (error) {
      this.output.appendLine(`Could not open ${node.resource}: ${error.message || error}`);
      void vscode.window.showErrorMessage(`Could not open ${node.resource}: ${error.message || error}`);
    }
  }
}

class PackageTreeProvider {
  constructor(controller) {
    this.controller = controller;
    this.onDidChangeTreeData = controller.packageEmitter.event;
    this.packageNodes = new Map();
  }

  getTreeItem(node) {
    const state = vscode.TreeItemCollapsibleState;
    if (node.kind === 'package') {
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
      const item = new vscode.TreeItem(
        'Dependencies',
        node.dependencies.length ? state.Collapsed : state.None,
      );
      item.description = String(node.dependencies.length);
      item.iconPath = new vscode.ThemeIcon('references');
      return item;
    }
    if (node.kind === 'sourceRoot') {
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
    if (node.kind === 'sourceArea') {
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
      item.description = formatCount(node.children.length);
      item.iconPath = new vscode.ThemeIcon(areaObjectIcon(node.objectKind));
      return item;
    }
    if (node.kind === 'sourceAreaObject') {
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
    if (node.kind === 'sourceFile') {
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
    item.iconPath = new vscode.ThemeIcon(node.icon);
    if (node.uri) {
      item.resourceUri = node.uri;
      item.command = { command: node.command, title: node.label, arguments: [node.uri] };
    }
    return item;
  }

  clearCache() {
    this.packageNodes.clear();
  }

  async getChildren(node) {
    await this.controller.ensurePackages();
    if (!node) {
      const current = new Set(this.controller.packages.map((packageInfo) => packageInfo.manifestPath));
      for (const key of this.packageNodes.keys()) if (!current.has(key)) this.packageNodes.delete(key);
      return this.controller.packages.map((packageInfo) => {
        let packageNode = this.packageNodes.get(packageInfo.manifestPath);
        if (!packageNode || packageNode.package !== packageInfo) {
          packageNode = { kind: 'package', package: packageInfo };
          this.packageNodes.set(packageInfo.manifestPath, packageNode);
        }
        return packageNode;
      });
    }
    if (node.kind === 'package') {
      if (node.loadedChildren) return node.loadedChildren;
      const packageInfo = node.package;
      const children = [{
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
          package: packageInfo,
        });
      }
      children.push({
        kind: 'dependencyGroup',
        packageRoot: packageInfo.root,
        dependencies: packageInfo.dependencies || [],
      });
      node.loadedChildren = attachParents(children, node);
      return node.loadedChildren;
    }
    if (node.kind === 'sourceRoot') {
      if (node.loadedChildren) return node.loadedChildren;
      try {
        const catalog = await this.controller.packageSource(node.package);
        node.loadedChildren = attachParents(sourceSections(catalog, node.package), node);
        return node.loadedChildren;
      } catch (error) {
        this.controller.output.appendLine(
          `Could not inspect ${node.package.sourcePath}: ${error.message || error}`,
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
      return node.dependencies.map((dependency) => ({
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

  getParent(node) {
    return node?.parent;
  }

  async findAreaObject(selection) {
    const packages = await this.getChildren();
    const packageNode = packages.find((node) => normalizePath(node.package.manifestPath)
      === normalizePath(selection.manifestPath));
    if (!packageNode) return undefined;
    const packageChildren = await this.getChildren(packageNode);
    const sourceRoot = packageChildren.find((node) => node.kind === 'sourceRoot');
    if (!sourceRoot) return undefined;
    const roots = await this.getChildren(sourceRoot);
    return findTreeNode(roots, (node) => node.kind === 'sourceAreaObject'
      && node.area.resref.toLowerCase() === String(selection.resref).toLowerCase()
      && node.object.key === selection.objectKey);
  }
}

class ResourceTreeProvider {
  constructor(controller) {
    this.controller = controller;
    this.onDidChangeTreeData = controller.resourceEmitter.event;
  }

  getTreeItem(node) {
    const expandable = node.kind !== 'resource' && node.kind !== 'error';
    const item = new vscode.TreeItem(
      node.label,
      expandable ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None,
    );
    if (node.count > 1 || expandable) item.description = formatCount(node.count);
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

  async getChildren(node) {
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
      this.controller.output.appendLine(`Could not list nwnrs resources: ${error.message || error}`);
      return [{
        kind: 'error',
        label: 'Resources unavailable',
        count: 0,
        error: error.message || String(error),
      }];
    }
  }
}

function comparePackages(left, right) {
  return left.name.localeCompare(right.name, undefined, { sensitivity: 'base' })
    || left.root.localeCompare(right.root);
}

function sourceSections(catalog, packageInfo) {
  const sections = [];
  if (catalog.areas?.length) {
    const registered = catalog.areas.filter((area) => area.registered);
    const unregistered = catalog.areas.filter((area) => !area.registered);
    const children = buildAreaPathTree(registered, packageInfo);
    if (unregistered.length) {
      children.push({
        kind: 'sourceUnregistered',
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

function buildAreaPathTree(areas, packageInfo) {
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
];

function areaObjectGroups(area, packageInfo) {
  return AREA_OBJECT_GROUPS.map(([objectKind, label]) => {
    const objects = (area.objects || []).filter((object) => object.kind === objectKind);
    return {
      kind: 'sourceAreaObjectGroup',
      objectKind,
      label,
      children: objects.map((object) => ({
        kind: 'sourceAreaObject',
        objectKind,
        object,
        area,
        package: packageInfo,
      })),
    };
  }).filter((group) => group.children.length);
}

function areaObjectIcon(kind) {
  return ({
    creature: 'person',
    door: 'layout-sidebar-left',
    placeable: 'symbol-object',
    encounter: 'pulse',
    sound: 'unmute',
    store: 'store',
    trigger: 'symbol-event',
    waypoint: 'location',
  })[kind] || 'symbol-object';
}

function attachParents(children, parent) {
  for (const child of children) {
    child.parent = parent;
    if (child.children) attachParents(child.children, child);
  }
  return children;
}

function findTreeNode(nodes, predicate) {
  for (const node of nodes) {
    if (predicate(node)) return node;
    const match = findTreeNode(node.children || [], predicate);
    if (match) return match;
  }
  return undefined;
}

function buildSourceFileTree(files, baseDirectory = '') {
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

function buildDirectoryNodes(entries) {
  const root = { directories: new Map(), leaves: [] };
  for (const entry of entries) {
    let current = root;
    const segments = normalizeRelativePath(entry.directory).split('/').filter(Boolean);
    for (const segment of segments) {
      if (!current.directories.has(segment)) {
        current.directories.set(segment, { directories: new Map(), leaves: [] });
      }
      current = current.directories.get(segment);
    }
    current.leaves.push(entry.node);
  }
  const materialize = (branch) => [
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

function commonDirectory(directories) {
  if (directories.length === 0) return '';
  const split = directories.map((directory) =>
    normalizeRelativePath(directory === '.' ? '' : directory).split('/').filter(Boolean));
  const shared = [];
  for (let index = 0; index < split[0].length; index += 1) {
    const segment = split[0][index];
    if (!split.every((parts) => parts[index] === segment)) break;
    shared.push(segment);
  }
  return shared.join('/');
}

function normalizeRelativePath(value) {
  return String(value || '').replaceAll('\\', '/').replace(/^\.\//u, '').replace(/\/$/u, '');
}

function sourceSectionIcon(section) {
  if (section === 'areas') return 'map';
  if (section === 'dialogs') return 'comment-discussion';
  return 'code';
}

function sourceFileIcon(kind) {
  if (kind === 'nss') return 'code';
  if (kind === 'dlg' || kind === 'dlgJson') return 'comment-discussion';
  return 'file';
}

function sourceFileDescription(kind) {
  if (kind === 'dlgJson') return 'DLG JSON';
  return kind.toUpperCase();
}

function normalizePath(value) {
  if (!value) return '';
  let normalized = path.resolve(value);
  try {
    normalized = fs.realpathSync.native(normalized);
  } catch {
    // Missing paths still need stable lexical identity while manifests are edited.
  }
  return process.platform === 'win32' ? normalized.toLowerCase() : normalized;
}

function pathContains(root, candidate) {
  if (!root || !candidate) return false;
  const relative = path.relative(normalizePath(root), normalizePath(candidate));
  return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

function owningPackage(filePath, packages) {
  if (!filePath) return undefined;
  return packages
    .filter((packageInfo) => pathContains(packageInfo.root, filePath))
    .sort((left, right) => normalizePath(right.root).length - normalizePath(left.root).length)[0];
}

function relativeDisplay(root, target) {
  if (!root || !target) return target || '';
  const relative = path.relative(root, target);
  return relative && !relative.startsWith('..') ? relative : target;
}

function sortResourceLayers(items) {
  return [...items].sort((left, right) => {
    const leftIndex = RESOURCE_LAYER_ORDER.indexOf(left.layer || left.label);
    const rightIndex = RESOURCE_LAYER_ORDER.indexOf(right.layer || right.label);
    return (leftIndex < 0 ? RESOURCE_LAYER_ORDER.length : leftIndex)
      - (rightIndex < 0 ? RESOURCE_LAYER_ORDER.length : rightIndex)
      || left.label.localeCompare(right.label);
  });
}

function resourceNode(item) {
  return {
    ...item,
    filePath: item.filePath,
  };
}

function childResourceQuery(node) {
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

function resourceIcon(node) {
  switch (node.kind) {
    case 'layer': return node.layer === 'Vanilla' ? 'library' : 'layers';
    case 'family': return 'folder';
    case 'extension': return 'symbol-file';
    case 'prefix': return 'list-tree';
    case 'error': return 'warning';
    default: return node.filePath ? 'file' : 'lock';
  }
}

function formatCount(count) {
  return Number(count || 0).toLocaleString();
}

module.exports = {
  NwnrsSidebarController,
  buildSourceFileTree,
  childResourceQuery,
  owningPackage,
  sourceSections,
  sortResourceLayers,
};
