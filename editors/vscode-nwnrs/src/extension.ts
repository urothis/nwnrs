import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import { LanguageWorkerClient } from './language-worker-client';
import { ResourceCustomEditorProvider } from './resource-custom-editor';
import { NwnrsSidebarController } from './sidebar';
import type {
  NativeCheckDiagnostic,
  NativeDefinition,
  NativeDocumentSymbol,
  NativeLanguageResponseMap,
  NativeReference,
} from './native-types';
import {
  buildCheckRequest,
  buildDefinitionRequest,
  buildDocumentSymbolsRequest,
  diagnosticRange,
  findProjectRoot,
  formatHoverDocumentation,
  isNssPath,
  nativeBindingPath,
  resolveConfiguredPath,
  selectHoverDefinition,
  type NativeCheckRequest,
  type NativeDefinitionRequest,
  type NativeDocumentSymbolsRequest,
  type SourceOverlay,
} from './compiler';

interface CompilerConfiguration {
  readonly includeDirectories: string[];
  readonly langspecPath: string;
  readonly rootPath: string;
  readonly userPath: string;
  readonly language: string;
  readonly loadOvr: boolean;
  readonly maxIncludeDepth: number;
  readonly maxDiagnosticsPerFile: number;
  readonly noEntrypointCheck: boolean;
}

interface WorkspaceRootCandidate {
  readonly root: string;
  readonly sourceUri: vscode.Uri;
}

interface CompilerRunRequest {
  readonly key: string;
  readonly targets: readonly string[];
  readonly cwd: string;
  readonly sourceUri: vscode.Uri;
  readonly recurse: boolean;
  readonly overlays: readonly SourceOverlay[];
  readonly revealFailure: boolean;
  readonly force: boolean;
}

interface ResolvedSymbol {
  readonly definitions: readonly NativeDefinition[];
  readonly wordRange: vscode.Range;
  readonly request: NativeDefinitionRequest;
}

interface ResolvedReferences {
  readonly records: readonly NativeReference[];
  readonly wordRange: vscode.Range;
  readonly request: NativeDefinitionRequest;
  readonly requests: readonly NativeDefinitionRequest[];
  readonly symbol: string;
}

interface DiagnosticRunEntry {
  readonly uri: vscode.Uri;
  readonly diagnostics: vscode.Diagnostic[];
}

interface IncludedSource {
  readonly path: string;
  readonly uri: string | null;
  readonly resource: string | null;
  readonly range: vscode.Range;
  readonly includeName: string;
}

interface UriOwner {
  readonly uri: vscode.Uri;
}

type NwnrsCallHierarchyItem = vscode.CallHierarchyItem & {
  readonly _nwnrsRequest?: NativeDefinitionRequest;
};

const DIAGNOSTIC_OWNER = 'nwnrs';
const SEMANTIC_TOKEN_TYPES = [
  'function', 'parameter', 'variable', 'property', 'type', 'enum', 'enumMember', 'macro',
];
const SEMANTIC_TOKEN_MODIFIERS = ['declaration', 'readonly', 'defaultLibrary'];
const SEMANTIC_LEGEND = new vscode.SemanticTokensLegend(
  SEMANTIC_TOKEN_TYPES,
  SEMANTIC_TOKEN_MODIFIERS,
);

class CompilerController {
  public readonly output: vscode.OutputChannel;
  private readonly context: vscode.ExtensionContext;
  private readonly diagnostics: vscode.DiagnosticCollection;
  private readonly manifestDiagnostics: vscode.DiagnosticCollection;
  private readonly status: vscode.StatusBarItem;
  private readonly languageWorker: LanguageWorkerClient;
  private sequence: number;
  private changeEpoch: number;
  private readonly activeRuns: Map<string, {
    cancellation: vscode.CancellationTokenSource;
    sequence: number;
    fingerprint: string;
  }>;
  private readonly runDiagnostics: Map<string, Map<string, DiagnosticRunEntry>>;
  private readonly timers: Map<string, NodeJS.Timeout>;
  private readonly manifestTimers: Map<string, NodeJS.Timeout>;
  private readonly manifestChecks: Map<string, unknown>;
  private readonly virtualDocumentRequests: Map<string, NativeDocumentSymbolsRequest>;
  private readonly physicalDocumentRequests: Map<string, NativeDocumentSymbolsRequest[]>;
  private readonly virtualDocuments: Map<string, string>;
  private readonly externalWatchers: Map<string, vscode.FileSystemWatcher[]>;
  private readonly watchRootRequests: Set<string>;

  public constructor(context: vscode.ExtensionContext) {
    this.context = context;
    this.diagnostics = vscode.languages.createDiagnosticCollection(DIAGNOSTIC_OWNER);
    this.manifestDiagnostics = vscode.languages.createDiagnosticCollection('nwnrs nwpkg');
    this.output = vscode.window.createOutputChannel('nwnrs Compiler');
    this.status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 20);
    this.status.command = 'nwnrs.showStatusMenu';
    this.status.text = '$(check) nwnrs';
    this.status.tooltip = 'nwnrs NWScript language service';
    this.status.show();
    this.sequence = 0;
    this.activeRuns = new Map();
    this.runDiagnostics = new Map();
    this.timers = new Map();
    this.manifestTimers = new Map();
    this.manifestChecks = new Map();
    this.changeEpoch = 0;
    this.virtualDocumentRequests = new Map();
    this.physicalDocumentRequests = new Map();
    this.virtualDocuments = new Map();
    this.externalWatchers = new Map();
    this.watchRootRequests = new Set();
    this.languageWorker = new LanguageWorkerClient(
      path.join(__dirname, 'compiler-worker.js'),
      nativeBindingPath(context.extensionPath),
      this.output,
    );

    context.subscriptions.push(
      this.diagnostics,
      this.manifestDiagnostics,
      this.output,
      this.status,
      this.languageWorker,
    );
  }

  register() {
    this.context.subscriptions.push(
      vscode.commands.registerCommand('nwnrs.checkCurrentFile', async () => {
        const document = vscode.window.activeTextEditor?.document;
        if (!document || !this.isNwScriptDocument(document)) {
          void vscode.window.showWarningMessage('Open an NSS file before running an nwnrs check.');
          return;
        }
        await this.checkDocument(document, true);
      }),
      vscode.commands.registerCommand('nwnrs.checkWorkspace', async () => {
        await this.checkWorkspace();
      }),
      vscode.commands.registerCommand('nwnrs.showStatusMenu', async () => {
        await this.showStatusMenu();
      }),
      vscode.commands.registerCommand('nwnrs.showCompilerOutput', () => this.output.show(true)),
      vscode.workspace.onDidSaveTextDocument((document) => {
        if (this.isNwpkgDocument(document)) {
          this.invalidateChecks(document);
          this.scheduleManifest(document);
          return;
        }
        if (!this.isNwScriptDocument(document)) {
          return;
        }
        this.invalidateChecks(document);
        const config = vscode.workspace.getConfiguration('nwnrs', document.uri);
        if (config.get('checkOnSave', true)) {
          this.scheduleDocument(document);
        }
      }),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (this.isNwpkgDocument(event.document)) {
          this.invalidateChecks(event.document);
          this.scheduleManifest(event.document);
          return;
        }
        if (!this.isNwScriptDocument(event.document)) {
          return;
        }
        this.invalidateChecks(event.document);
        const config = vscode.workspace.getConfiguration('nwnrs', event.document.uri);
        if (config.get('checkOnChange', true)) {
          this.scheduleDocument(event.document);
        }
      }),
      vscode.workspace.onDidOpenTextDocument((document) => {
        if (this.isNwpkgDocument(document)) {
          this.scheduleManifest(document);
          return;
        }
        if (!this.isNwScriptDocument(document)) {
          return;
        }
        this.invalidateChecks(document);
        const config = vscode.workspace.getConfiguration('nwnrs', document.uri);
        if (config.get('checkOnOpen', true)) {
          this.scheduleDocument(document);
        }
      }),
      vscode.workspace.onDidCloseTextDocument((document) => {
        if (this.isNwpkgDocument(document)) {
          const timer = this.manifestTimers.get(document.uri.toString());
          if (timer) {
            clearTimeout(timer);
            this.manifestTimers.delete(document.uri.toString());
          }
          this.manifestChecks.delete(document.uri.toString());
          this.manifestDiagnostics.delete(document.uri);
          return;
        }
        if (!this.isNwScriptDocument(document)) {
          return;
        }
        this.invalidateChecks(document);
        const key = this.documentKey(document);
        const timer = this.timers.get(key);
        if (timer) {
          clearTimeout(timer);
          this.timers.delete(key);
        }
      }),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('nwnrs')) {
          this.invalidateChecks();
          this.resetExternalWatchers();
          const document = vscode.window.activeTextEditor?.document;
          if (document && this.isNwScriptDocument(document)) {
            this.scheduleDocument(document);
          }
        }
      }),
      vscode.languages.registerDefinitionProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideDefinition: (document, position, token) =>
            this.provideDefinition(document, position, token),
        },
      ),
      vscode.languages.registerReferenceProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideReferences: (document, position, referenceContext, token) =>
            this.provideReferences(document, position, referenceContext, token),
        },
      ),
      vscode.languages.registerRenameProvider(
        { language: 'nwscript', scheme: 'file' },
        {
          prepareRename: (document, position, token) =>
            this.prepareRename(document, position, token),
          provideRenameEdits: (document, position, newName, token) =>
            this.provideRenameEdits(document, position, newName, token),
        },
      ),
      vscode.languages.registerCallHierarchyProvider(
        { language: 'nwscript', scheme: 'file' },
        {
          prepareCallHierarchy: (document, position, token) =>
            this.prepareCallHierarchy(document, position, token),
          provideCallHierarchyIncomingCalls: (item, token) =>
            this.provideIncomingCalls(item, token),
          provideCallHierarchyOutgoingCalls: (item, token) =>
            this.provideOutgoingCalls(item, token),
        },
      ),
      vscode.languages.registerWorkspaceSymbolProvider({
        provideWorkspaceSymbols: (query, token) => this.provideWorkspaceSymbols(query, token),
      }),
      vscode.languages.registerHoverProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideHover: (document, position, token) =>
            this.provideHover(document, position, token),
        },
      ),
      vscode.languages.registerDocumentSymbolProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideDocumentSymbols: (document, token) =>
            this.provideDocumentSymbols(document, token),
        },
      ),
      vscode.languages.registerCodeActionsProvider(
        { language: 'nwscript', scheme: 'file' },
        {
          provideCodeActions: (document, range, actionContext, token) =>
            this.provideCodeActions(document, range, actionContext, token),
        },
        { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix] },
      ),
      vscode.languages.registerDocumentSemanticTokensProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideDocumentSemanticTokens: (document, token) =>
            this.provideSemanticTokens(document, token),
        },
        SEMANTIC_LEGEND,
      ),
      vscode.languages.registerInlayHintsProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideInlayHints: (document, range, token) =>
            this.provideInlayHints(document, range, token),
        },
      ),
      vscode.languages.registerCompletionItemProvider(
        { language: 'nwpkg', scheme: 'file' },
        { provideCompletionItems: (document, position) => nwpkgCompletions(document, position) },
        '[', '.', '/', '"',
      ),
      vscode.languages.registerHoverProvider(
        { language: 'nwpkg', scheme: 'file' },
        { provideHover: (document, position) => nwpkgHover(document, position) },
      ),
      vscode.languages.registerDefinitionProvider(
        { language: 'nwpkg', scheme: 'file' },
        { provideDefinition: (document, position) => nwpkgDefinition(document, position) },
      ),
      vscode.languages.registerDocumentSymbolProvider(
        { language: 'nwpkg', scheme: 'file' },
        { provideDocumentSymbols: (document) => nwpkgDocumentSymbols(document) },
      ),
      vscode.workspace.registerTextDocumentContentProvider('nwnrs-game', {
        provideTextDocumentContent: (uri, token) => this.provideVirtualSource(uri, token),
      }),
    );

    this.registerWorkspaceWatchers();

    for (const document of vscode.workspace.textDocuments) {
      if (this.isNwpkgDocument(document)) {
        this.scheduleManifest(document);
        continue;
      }
      const config = vscode.workspace.getConfiguration('nwnrs', document.uri);
      if (config.get('checkOnOpen', true)) {
        this.scheduleDocument(document);
      }
    }
  }

  async showStatusMenu() {
    const selected = await vscode.window.showQuickPick([
      { label: '$(refresh) Reindex Current Package', action: 'reindex' },
      { label: '$(debug-restart) Restart Language Service', action: 'restart' },
      { label: '$(output) Show Compiler Output', action: 'output' },
      { label: '$(clear-all) Clear Diagnostics', action: 'clear' },
      { label: '$(gear) Open nwnrs Settings', action: 'settings' },
    ], {
      title: 'nwnrs',
      placeHolder: 'Choose an nwnrs action',
    });
    switch (selected?.action) {
      case 'reindex':
        await this.reindexCurrentPackage();
        break;
      case 'restart':
        await this.restartLanguageService();
        break;
      case 'output':
        this.output.show(true);
        break;
      case 'clear':
        this.clearDiagnostics();
        break;
      case 'settings':
        await vscode.commands.executeCommand('workbench.action.openSettings', '@ext:nwnrs.nwnrs');
        break;
      default:
        break;
    }
  }

  async reindexCurrentPackage() {
    const context = await this.currentPackageContext();
    if (!context) {
      void vscode.window.showWarningMessage(
        'Open a file inside an nwnrs package before reindexing.',
      );
      return;
    }
    const { projectRoot, representative } = context;
    const configuration = this.compilerConfiguration(representative, projectRoot);
    const request = buildDocumentSymbolsRequest(representative.fsPath, {
      projectRoot,
      overlays: this.sourceOverlays(projectRoot),
      ...configuration,
    });
    const previousText = this.status.text;
    const previousTooltip = this.status.tooltip;
    this.status.text = '$(sync~spin) nwnrs';
    this.status.tooltip = `Reindexing ${path.basename(projectRoot)}`;
    this.languageWorker.invalidate(projectRoot);
    this.physicalDocumentRequests.clear();
    let progressCancellation: vscode.CancellationToken | undefined;
    try {
      const index = await vscode.window.withProgress({
        location: vscode.ProgressLocation.Notification,
        title: `nwnrs: Reindexing ${path.basename(projectRoot)}`,
        cancellable: true,
      }, (_progress, token) => {
        progressCancellation = token;
        return this.invokeNative('indexProject', request, token);
      });
      for (const warning of index?.warnings || []) {
        this.output.appendLine(`nwnrs project-index warning: ${warning}`);
      }
      for (const document of index?.documents || []) {
        this.rememberPhysicalDocument(document.path, {
          ...request,
          source_path: document.path,
          overlays: [],
        });
      }
      const count = index?.documents?.length || 0;
      this.output.appendLine(`[index] ${projectRoot}: ${count} document(s)`);
      void vscode.window.showInformationMessage(
        `nwnrs reindexed ${count} document${count === 1 ? '' : 's'} in ${path.basename(projectRoot)}.`,
      );
    } catch (error) {
      if (progressCancellation?.isCancellationRequested) {
        this.output.appendLine(`[cancelled] Reindex ${projectRoot}`);
        return;
      }
      this.output.appendLine(`nwnrs reindex failure: ${String(error)}`);
      void vscode.window.showErrorMessage(`nwnrs reindex failed: ${String(error)}`);
    } finally {
      this.status.text = previousText;
      this.status.tooltip = previousTooltip;
    }
  }

  async currentPackageContext() {
    const document = vscode.window.activeTextEditor?.document;
    if (!document) {
      return undefined;
    }
    if (document.uri.scheme === 'nwnrs-game') {
      const request = this.virtualDocumentRequests.get(document.uri.toString());
      if (!request?.project_root || !request.source_path) {
        return undefined;
      }
      return {
        projectRoot: path.resolve(request.project_root),
        representative: vscode.Uri.file(request.source_path),
      };
    }
    if (document.uri.scheme !== 'file') {
      return undefined;
    }
    const projectRoot = this.isNwpkgDocument(document)
      ? path.dirname(document.uri.fsPath)
      : findProjectRoot(document.uri.fsPath);
    if (this.isNwScriptDocument(document)) {
      return { projectRoot, representative: document.uri };
    }
    const sources = await vscode.workspace.findFiles(
      new vscode.RelativePattern(projectRoot, '**/*.nss'),
      '**/{.git,node_modules,target}/**',
      1,
    );
    return sources[0] ? { projectRoot, representative: sources[0] } : undefined;
  }

  async restartLanguageService() {
    const previousText = this.status.text;
    const previousTooltip = this.status.tooltip;
    for (const active of this.activeRuns.values()) {
      active.cancellation.cancel();
    }
    this.activeRuns.clear();
    this.status.text = '$(sync~spin) nwnrs';
    this.status.tooltip = 'Restarting nwnrs language service';
    try {
      await this.languageWorker.restart();
      this.changeEpoch += 1;
      this.physicalDocumentRequests.clear();
      this.output.appendLine('[service] Language service restarted');
      this.status.text = '$(check) nwnrs';
      this.status.tooltip = 'nwnrs language service restarted';
      void vscode.window.showInformationMessage('nwnrs language service restarted.');
    } catch (error) {
      this.status.text = '$(warning) nwnrs';
      this.status.tooltip = String(error);
      this.output.appendLine(`nwnrs language-service restart failure: ${String(error)}`);
      void vscode.window.showErrorMessage(`nwnrs language service restart failed: ${String(error)}`);
      if (!this.languageWorker.isRunning) {
        return;
      }
      this.status.text = previousText;
      this.status.tooltip = previousTooltip;
    }
  }

  clearDiagnostics() {
    for (const timer of this.timers.values()) {
      clearTimeout(timer);
    }
    for (const timer of this.manifestTimers.values()) {
      clearTimeout(timer);
    }
    for (const active of this.activeRuns.values()) {
      active.cancellation.cancel();
    }
    this.timers.clear();
    this.manifestTimers.clear();
    this.manifestChecks.clear();
    this.activeRuns.clear();
    this.runDiagnostics.clear();
    this.diagnostics.clear();
    this.manifestDiagnostics.clear();
    this.status.text = '$(check) nwnrs';
    this.status.tooltip = 'nwnrs diagnostics cleared';
    this.output.appendLine('[clear] Diagnostics cleared');
  }

  isNwScriptDocument(document: vscode.TextDocument): boolean {
    return document.uri.scheme === 'file'
      && (document.languageId === 'nwscript' || isNssPath(document.uri.fsPath));
  }

  isNwpkgDocument(document: vscode.TextDocument): boolean {
    return document.uri.scheme === 'file'
      && (document.languageId === 'nwpkg' || path.basename(document.uri.fsPath) === 'nwpkg.toml');
  }

  scheduleManifest(document: vscode.TextDocument): void {
    const key = document.uri.toString();
    const previous = this.manifestTimers.get(key);
    if (previous) {
      clearTimeout(previous);
    }
    const config = vscode.workspace.getConfiguration('nwnrs', document.uri);
    const delay = Math.max(0, config.get('debounceMilliseconds', 250));
    this.manifestTimers.set(key, setTimeout(() => {
      this.manifestTimers.delete(key);
      void this.checkManifest(document);
    }, delay));
  }

  async checkManifest(document: vscode.TextDocument): Promise<void> {
    const key = document.uri.toString();
    const sequence = ++this.sequence;
    this.manifestChecks.set(key, sequence);
    try {
      const response = await this.invokeNative('checkNwpkg', {
        path: document.uri.fsPath,
        contents: document.getText(),
      });
      if (this.manifestChecks.get(key) !== sequence) {
        return;
      }
      const diagnostics = (response?.diagnostics || []).map((record) => {
        const range = diagnosticRange(record);
        const diagnostic = new vscode.Diagnostic(
          new vscode.Range(
            range.startLine,
            range.startColumn,
            range.endLine,
            range.endColumn,
          ),
          record.message,
          severity(record.severity),
        );
        diagnostic.source = 'nwnrs nwpkg';
        return diagnostic;
      });
      this.manifestDiagnostics.set(document.uri, diagnostics);
      this.manifestChecks.delete(key);
    } catch (error) {
      if (this.manifestChecks.get(key) !== sequence) {
        return;
      }
      this.manifestChecks.delete(key);
      this.output.appendLine(`nwnrs nwpkg validation failure: ${String(error)}`);
    }
  }

  documentKey(document: vscode.TextDocument): string {
    return `file:${document.uri.toString()}`;
  }

  invalidateChecks(document?: UriOwner): void {
    this.changeEpoch += 1;
    if (!document || path.basename(document.uri?.fsPath || '') === 'nwpkg.toml') {
      this.physicalDocumentRequests.clear();
    }
    if (document?.uri?.scheme === 'file') {
      this.languageWorker.invalidate('', document.uri.fsPath);
    } else {
      this.languageWorker.invalidate();
    }
  }

  registerWorkspaceWatchers() {
    const watchers = [
      vscode.workspace.createFileSystemWatcher('**/*.nss'),
      vscode.workspace.createFileSystemWatcher('**/nwpkg.toml'),
    ];
    for (const watcher of watchers) {
      const changed = (uri: vscode.Uri): void => this.handleWorkspaceFileChange(uri);
      this.context.subscriptions.push(
        watcher,
        watcher.onDidCreate(changed),
        watcher.onDidChange(changed),
        watcher.onDidDelete(changed),
      );
    }
  }

  handleWorkspaceFileChange(uri: vscode.Uri): void {
    if (this.ignoredWorkspacePath(uri.fsPath)) {
      return;
    }
    this.invalidateChecks({ uri });
    if (path.basename(uri.fsPath) === 'nwpkg.toml') {
      this.resetExternalWatchers();
    }
    const changedFolder = vscode.workspace.getWorkspaceFolder(uri);
    for (const document of vscode.workspace.textDocuments) {
      if (!this.isNwScriptDocument(document)) {
        continue;
      }
      const documentFolder = vscode.workspace.getWorkspaceFolder(document.uri);
      if (!changedFolder || !documentFolder
          || changedFolder.uri.toString() === documentFolder.uri.toString()) {
        this.scheduleDocument(document);
      }
    }
  }

  ignoredWorkspacePath(filePath: string): boolean {
    return filePath.split(path.sep).some((segment) =>
      segment === '.git' || segment === 'node_modules' || segment === 'target');
  }

  resetExternalWatchers() {
    for (const watchers of this.externalWatchers.values()) {
      for (const watcher of watchers) {
        watcher.dispose();
      }
    }
    this.externalWatchers.clear();
    this.watchRootRequests.clear();
  }

  async ensureExternalWatchRoots(
    inputs: readonly string[],
    includeDirectories: readonly string[],
  ): Promise<void> {
    const requestKey = JSON.stringify({ inputs, includeDirectories });
    if (this.watchRootRequests.has(requestKey)) {
      return;
    }
    this.watchRootRequests.add(requestKey);
    try {
      const resolved = await this.invokeNative('resolveWatchRoots', { roots: inputs });
      const roots = [...new Set([
        ...(Array.isArray(resolved) ? resolved : []),
        ...includeDirectories,
      ].map((root) => path.resolve(root)))];
      for (const root of roots) {
        if (this.externalWatchers.has(root) || this.rootIsInsideWorkspace(root)) {
          continue;
        }
        const sourceWatcher = vscode.workspace.createFileSystemWatcher(
          new vscode.RelativePattern(root, '**/*.nss'),
        );
        const manifestWatcher = vscode.workspace.createFileSystemWatcher(
          new vscode.RelativePattern(root, '**/nwpkg.toml'),
        );
        for (const watcher of [sourceWatcher, manifestWatcher]) {
          const changed = (uri: vscode.Uri): void => this.handleWorkspaceFileChange(uri);
          watcher.onDidCreate(changed, undefined, this.context.subscriptions);
          watcher.onDidChange(changed, undefined, this.context.subscriptions);
          watcher.onDidDelete(changed, undefined, this.context.subscriptions);
        }
        this.externalWatchers.set(root, [sourceWatcher, manifestWatcher]);
        this.context.subscriptions.push(sourceWatcher, manifestWatcher);
      }
    } catch (error) {
      this.watchRootRequests.delete(requestKey);
      this.output.appendLine(`nwnrs watch-root warning: ${String(error)}`);
    }
  }

  rootIsInsideWorkspace(root: string): boolean {
    return (vscode.workspace.workspaceFolders || []).some((folder) => {
      const relative = path.relative(folder.uri.fsPath, root);
      return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
    });
  }

  scheduleDocument(document: vscode.TextDocument): void {
    if (!this.isNwScriptDocument(document)) {
      return;
    }
    const key = this.documentKey(document);
    const previous = this.timers.get(key);
    if (previous) {
      clearTimeout(previous);
    }
    const config = vscode.workspace.getConfiguration('nwnrs', document.uri);
    const delay = Math.max(0, config.get('debounceMilliseconds', 250));
    const timer = setTimeout(() => {
      this.timers.delete(key);
      void this.checkDocument(document, false);
    }, delay);
    this.timers.set(key, timer);
  }

  async checkDocument(document: vscode.TextDocument, revealFailure: boolean): Promise<void> {
    if (!this.isNwScriptDocument(document)) {
      return;
    }
    const filePath = document.uri.fsPath;
    const projectRoot = findProjectRoot(filePath);
    await this.runCompiler({
      key: this.documentKey(document),
      targets: [filePath],
      cwd: projectRoot,
      sourceUri: document.uri,
      recurse: false,
      overlays: this.sourceOverlays(projectRoot),
      revealFailure,
      force: revealFailure,
    });
  }

  async checkWorkspace() {
    const folders = vscode.workspace.workspaceFolders || [];
    if (folders.length === 0) {
      void vscode.window.showWarningMessage('Open a workspace before running an nwnrs check.');
      return;
    }
    const candidates = [];
    for (const folder of folders) {
      const manifests = await vscode.workspace.findFiles(
        new vscode.RelativePattern(folder, '**/nwpkg.toml'),
        '**/{.git,node_modules,target}/**',
      );
      const roots = manifests.length > 0
        ? manifests.map((uri) => path.dirname(uri.fsPath))
        : [folder.uri.fsPath];
      for (const root of [...new Set(roots)]) {
        candidates.push({ root, sourceUri: folder.uri });
      }
    }
    const roots = await this.deduplicateWorkspaceRoots(candidates);
    await vscode.window.withProgress({
      location: vscode.ProgressLocation.Notification,
      title: 'Checking NWScript workspace',
      cancellable: true,
    }, async (progress, token) => {
      if (roots.length === 0) {
        progress.report({ increment: 100, message: 'No NWScript projects found' });
        return;
      }
      const increment = 100 / roots.length;
      await Promise.all(roots.map(async ({ root, sourceUri }) => {
        if (token.isCancellationRequested) {
          return;
        }
        progress.report({ message: path.basename(root) });
        await this.runCompiler({
          key: `workspace:${root}`,
          targets: [root],
          cwd: root,
          sourceUri,
          recurse: true,
          overlays: this.sourceOverlays(root),
          revealFailure: true,
          force: true,
        }, token);
        progress.report({ increment, message: path.basename(root) });
      }));
    });
  }

  async deduplicateWorkspaceRoots(
    candidates: readonly WorkspaceRootCandidate[],
  ): Promise<WorkspaceRootCandidate[]> {
    const unique: WorkspaceRootCandidate[] = [
      ...new Map<string, WorkspaceRootCandidate>(
        candidates.map((candidate) => [candidate.root, candidate]),
      ).values(),
    ];
    if (unique.length < 2) {
      return unique;
    }
    try {
      const response = await this.invokeNative('deduplicateProjectRoots', {
        roots: unique.map((candidate) => candidate.root),
      });
      if (!Array.isArray(response)) {
        throw new Error('native project deduplication returned an invalid response');
      }
      const selected = new Set(response.map((root) => path.resolve(root)));
      return unique.filter((candidate) => {
        let canonical = path.resolve(candidate.root);
        try {
          canonical = fs.realpathSync.native(candidate.root);
        } catch {
          // The compiler will report an inaccessible root with full context.
        }
        return selected.has(canonical);
      });
    } catch (error) {
      this.output.appendLine(`nwnrs project deduplication warning: ${String(error)}`);
      return unique;
    }
  }

  sourceOverlays(_root: string | null | undefined): SourceOverlay[] {
    return vscode.workspace.textDocuments
      .filter((document) => this.isNwScriptDocument(document) && document.isDirty)
      .map((document) => ({ path: document.uri.fsPath, contents: document.getText() }));
  }

  compilerConfiguration(sourceUri: vscode.Uri, cwd: string): CompilerConfiguration {
    const config = vscode.workspace.getConfiguration('nwnrs', sourceUri);
    const workspaceFolder = vscode.workspace.getWorkspaceFolder(sourceUri)?.uri.fsPath || cwd;
    const context = {
      workspaceFolder,
      projectRoot: cwd,
      fileDirname: sourceUri.scheme === 'file' ? path.dirname(sourceUri.fsPath) : cwd,
    };
    const includeDirectories = config.get('includeDirectories', []).map((entry) =>
      resolveConfiguredPath(entry, context, cwd));
    const langspecPath = resolveConfiguredPath(config.get('langspecPath', ''), context, cwd);
    const rootPath = resolveConfiguredPath(config.get('rootPath', ''), context, cwd);
    const userPath = resolveConfiguredPath(config.get('userPath', ''), context, cwd);
    return {
      includeDirectories,
      langspecPath,
      rootPath,
      userPath,
      language: config.get('language', 'english'),
      loadOvr: config.get('loadOvr', false),
      maxIncludeDepth: config.get('maxIncludeDepth', 16),
      maxDiagnosticsPerFile: config.get('maxDiagnosticsPerFile', 50),
      noEntrypointCheck: config.get('noEntrypointCheck', true),
    };
  }

  async invokeNative<K extends keyof NativeLanguageResponseMap>(
    method: K,
    request: unknown,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<NativeLanguageResponseMap[K]> {
    return this.languageWorker.requestTyped(
      method,
      request,
      cancellationToken,
      this.languageSessionKey(request),
    );
  }

  languageSessionKey(request: unknown): string {
    const requestPath = recordString(request, 'path');
    if (requestPath && path.basename(requestPath) === 'nwpkg.toml') {
      return path.dirname(path.resolve(requestPath));
    }
    const projectRoot = recordString(request, 'project_root');
    if (projectRoot) {
      return path.resolve(projectRoot);
    }
    const sourcePath = recordString(request, 'source_path');
    if (sourcePath) {
      return findProjectRoot(sourcePath);
    }
    const paths = recordStringArray(request, 'paths');
    if (paths?.length === 1 && paths[0]) {
      return findProjectRoot(paths[0]);
    }
    return '__workspace__';
  }

  async resolveSymbol(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
    operation: string,
  ): Promise<ResolvedSymbol | undefined> {
    const wordRange = document.getWordRangeAtPosition(
      position,
      /[A-Za-z_][A-Za-z0-9_]*/u,
    );
    if (!wordRange || cancellationToken.isCancellationRequested) {
      return undefined;
    }
    const symbol = document.getText(wordRange);
    const qualifier = this.symbolQualifier(document, wordRange);
    let request: NativeDefinitionRequest;
    if (document.uri.scheme === 'nwnrs-game') {
      const context = this.virtualDocumentRequests.get(document.uri.toString());
      if (!context) {
        return undefined;
      }
      request = definitionRequestFromSourceContext(
        context,
        symbol,
        qualifier,
        this.sourceOverlays(context.project_root),
      );
    } else {
      const sourcePath = document.uri.fsPath;
      const origin = this.physicalDocumentRequests.get(path.resolve(sourcePath))?.[0];
      if (origin) {
        request = definitionRequestFromSourceContext(
          { ...origin, source_path: sourcePath },
          symbol,
          qualifier,
          this.sourceOverlays(origin.project_root),
        );
      } else {
        const projectRoot = findProjectRoot(sourcePath);
        const configuration = this.compilerConfiguration(document.uri, projectRoot);
        request = buildDefinitionRequest(sourcePath, symbol, {
          projectRoot,
          qualifier,
          overlays: this.sourceOverlays(projectRoot),
          ...configuration,
        });
      }
    }
    try {
      const response = await this.invokeNative('findDefinitions', request, cancellationToken);
      const definitions = Array.isArray(response) ? response : [];
      this.rememberVirtualDocuments(definitions, request);
      return {
        definitions,
        wordRange,
        request,
      };
    } catch (error) {
      if (cancellationToken.isCancellationRequested) {
        return undefined;
      }
      this.output.appendLine(`nwnrs ${operation} failure: ${String(error)}`);
      return undefined;
    }
  }

  rememberVirtualDocuments(
    definitions: readonly NativeDefinition[],
    request: NativeDefinitionRequest,
  ): void {
    const reusableRequest = { ...request, overlays: [] };
    for (const definition of definitions) {
      if (!definition.uri && path.isAbsolute(definition.path)) {
        this.rememberPhysicalDocument(definition.path, {
          ...reusableRequest,
          source_path: definition.path,
        });
      }
      if (typeof definition.uri === 'string'
          && definition.uri.startsWith('nwnrs-game:')
          && typeof definition.resource === 'string') {
        this.virtualDocumentRequests.set(
          definition.uri,
          sourceContextFromRequest(reusableRequest, definition.path, definition.resource),
        );
      }
    }
  }

  rememberPhysicalDocument(
    sourcePath: string,
    request: NativeDefinitionRequest | NativeDocumentSymbolsRequest,
  ): void {
    const sourceContext = sourceContextFromRequest(request, sourcePath);
    const key = path.resolve(sourcePath);
    const contexts = [...(this.physicalDocumentRequests.get(key) || [])];
    const owningRoot = findProjectRoot(sourcePath);
    const contextKey = path.resolve(sourceContext.project_root || owningRoot);
    const existing = contexts.findIndex((context) =>
      path.resolve(context.project_root || owningRoot) === contextKey);
    if (existing >= 0) {
      contexts.splice(existing, 1);
    }
    contexts.push(sourceContext);
    contexts.sort((left, right) => {
      const score = (context: NativeDocumentSymbolsRequest): number => context.project_root
        && path.resolve(context.project_root) !== path.resolve(owningRoot) ? 1 : 0;
      return score(right) - score(left);
    });
    this.physicalDocumentRequests.set(key, contexts);
  }

  async provideVirtualSource(
    uri: vscode.Uri,
    cancellationToken: vscode.CancellationToken,
  ): Promise<string> {
    const key = uri.toString();
    const cached = this.virtualDocuments.get(key);
    if (cached !== undefined) {
      return cached;
    }
    const request = this.virtualDocumentRequests.get(key);
    if (!request) {
      throw new Error('The nwnrs game-source context expired; invoke Go to Definition again.');
    }
    const response = await this.invokeNative('readVirtualSource', request, cancellationToken);
    if (!response || response.uri !== key || typeof response.contents !== 'string') {
      throw new Error('The compiler returned a different game script than requested.');
    }
    this.virtualDocuments.set(key, response.contents);
    return response.contents;
  }

  async provideDocumentSymbols(
    document: vscode.TextDocument,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.DocumentSymbol[]> {
    let request;
    if (document.uri.scheme === 'nwnrs-game') {
      const context = this.virtualDocumentRequests.get(document.uri.toString());
      if (!context) {
        return [];
      }
      request = {
        ...context,
        overlays: this.sourceOverlays(context.project_root),
      };
    } else {
      const sourcePath = document.uri.fsPath;
      const projectRoot = findProjectRoot(sourcePath);
      const configuration = this.compilerConfiguration(document.uri, projectRoot);
      request = buildDocumentSymbolsRequest(sourcePath, {
        projectRoot,
        overlays: this.sourceOverlays(projectRoot),
        ...configuration,
      });
    }

    let records;
    try {
      const response = await this.invokeNative(
        'listDocumentSymbols',
        request,
        cancellationToken,
      );
      records = Array.isArray(response) ? response : [];
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs Outline failure: ${String(error)}`);
      }
      return [];
    }
    if (cancellationToken.isCancellationRequested) {
      return [];
    }
    return records.map((record) => documentSymbol(record));
  }

  async provideCodeActions(
    document: vscode.TextDocument,
    _range: vscode.Range,
    actionContext: vscode.CodeActionContext,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.CodeAction[]> {
    const actions = [];
    for (const diagnostic of actionContext.diagnostics) {
      if (diagnostic.source !== DIAGNOSTIC_OWNER) {
        continue;
      }
      if (diagnostic.code === -573) {
        const action = new vscode.CodeAction(
          'Insert missing semicolon',
          vscode.CodeActionKind.QuickFix,
        );
        action.diagnostics = [diagnostic];
        action.isPreferred = true;
        action.edit = new vscode.WorkspaceEdit();
        action.edit.insert(document.uri, diagnostic.range.end, ';');
        actions.push(action);
      }
      if (diagnostic.code === -622) {
        const match = diagnostic.message.match(/undefined identifier\s+"([A-Za-z_][A-Za-z0-9_]*)"/u);
        const missingSymbol = match?.[1];
        if (!missingSymbol) {
          continue;
        }
        const sourcePath = document.uri.fsPath;
        const projectRoot = findProjectRoot(sourcePath);
        const configuration = this.compilerConfiguration(document.uri, projectRoot);
        const request = buildDefinitionRequest(sourcePath, missingSymbol, {
          projectRoot,
          overlays: this.sourceOverlays(projectRoot),
          ...configuration,
        });
        try {
          const candidates = await this.invokeNative(
            'findIncludeCandidates',
            request,
            cancellationToken,
          );
          if (!Array.isArray(candidates) || candidates.length !== 1) {
            continue;
          }
          const includeName = candidates[0].include_name;
          if (document.getText().match(new RegExp(
            `^\\s*#include\\s+"${escapeRegExp(includeName)}"`,
            'imu',
          ))) {
            continue;
          }
          const action = new vscode.CodeAction(
            `Add #include "${includeName}"`,
            vscode.CodeActionKind.QuickFix,
          );
          action.diagnostics = [diagnostic];
          action.isPreferred = true;
          action.edit = new vscode.WorkspaceEdit();
          const insertion = includeInsertion(document, includeName);
          action.edit.insert(document.uri, insertion.position, insertion.text);
          actions.push(action);
        } catch (error) {
          if (!cancellationToken.isCancellationRequested) {
            this.output.appendLine(`nwnrs include quick-fix failure: ${String(error)}`);
          }
        }
      }
    }
    return actions;
  }

  async semanticDocument(
    document: vscode.TextDocument,
    cancellationToken: vscode.CancellationToken,
  ) {
    let request;
    if (document.uri.scheme === 'nwnrs-game') {
      const context = this.virtualDocumentRequests.get(document.uri.toString());
      if (!context) {
        return { tokens: [], hints: [] };
      }
      request = {
        ...context,
        overlays: this.sourceOverlays(context.project_root),
      };
    } else {
      const sourcePath = document.uri.fsPath;
      const projectRoot = findProjectRoot(sourcePath);
      const configuration = this.compilerConfiguration(document.uri, projectRoot);
      request = buildDocumentSymbolsRequest(sourcePath, {
        projectRoot,
        overlays: this.sourceOverlays(projectRoot),
        ...configuration,
      });
    }
    const response = await this.invokeNative('analyzeDocument', request, cancellationToken);
    return response && typeof response === 'object'
      ? response
      : { tokens: [], hints: [] };
  }

  async provideSemanticTokens(
    document: vscode.TextDocument,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.SemanticTokens> {
    try {
      const response = await this.semanticDocument(document, cancellationToken);
      const builder = new vscode.SemanticTokensBuilder(SEMANTIC_LEGEND);
      for (const record of response.tokens || []) {
        const range = diagnosticRange(record.range || {});
        if (range.startLine !== range.endLine) {
          continue;
        }
        const length = Math.max(1, range.endColumn - range.startColumn);
        const tokenType = SEMANTIC_TOKEN_TYPES.indexOf(record.kind);
        if (tokenType < 0) {
          continue;
        }
        let modifiers = 0;
        if (record.is_declaration) {
          modifiers |= 1 << SEMANTIC_TOKEN_MODIFIERS.indexOf('declaration');
        }
        if (record.is_readonly) {
          modifiers |= 1 << SEMANTIC_TOKEN_MODIFIERS.indexOf('readonly');
        }
        if (record.is_default_library) {
          modifiers |= 1 << SEMANTIC_TOKEN_MODIFIERS.indexOf('defaultLibrary');
        }
        builder.push(range.startLine, range.startColumn, length, tokenType, modifiers);
      }
      return builder.build();
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs semantic highlighting failure: ${String(error)}`);
      }
      return new vscode.SemanticTokens(new Uint32Array());
    }
  }

  async provideInlayHints(
    document: vscode.TextDocument,
    range: vscode.Range,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.InlayHint[]> {
    const config = vscode.workspace.getConfiguration('nwnrs.inlayHints', document.uri);
    const enumValues = config.get('enumValues', true);
    const parameterNames = config.get<string>('parameterNames', 'literals');
    if (!enumValues && parameterNames === 'off') {
      return [];
    }
    try {
      const response = await this.semanticDocument(document, cancellationToken);
      return (response.hints || [])
        .filter((record) => (record.kind === 'enumValue' && enumValues)
          || (record.kind === 'parameterLiteral' && parameterNames !== 'off')
          || (record.kind === 'parameter' && parameterNames === 'all'))
        .map((record) => {
          const hint = new vscode.InlayHint(
            new vscode.Position(record.line - 1, record.column - 1),
            record.label,
            record.kind === 'enumValue'
              ? vscode.InlayHintKind.Type
              : vscode.InlayHintKind.Parameter,
          );
          hint.paddingRight = record.kind !== 'enumValue';
          return hint;
        })
        .filter((hint) => range.contains(hint.position));
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs inlay-hint failure: ${String(error)}`);
      }
      return [];
    }
  }

  symbolQualifier(document: vscode.TextDocument, wordRange: vscode.Range): string | null {
    const line = document.lineAt(wordRange.start.line).text;
    const prefix = line.slice(0, wordRange.start.character);
    const match = prefix.match(/((?:[A-Za-z_][A-Za-z0-9_]*::)+)$/u);
    return match?.[1] ? match[1].slice(0, -2) : null;
  }

  async provideDefinition(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.Definition | undefined> {
    const included = await this.resolveIncludedSource(document, position, cancellationToken);
    if (included) {
      return new vscode.Location(
        included.uri ? vscode.Uri.parse(included.uri) : vscode.Uri.file(included.path),
        new vscode.Position(0, 0),
      );
    }
    try {
      const references = await this.resolveReferences(document, position, cancellationToken);
      const declarations = references?.records.filter((record) => record.is_declaration) || [];
      if (declarations.length > 0) {
        return declarations.map((record) => referenceLocation(record));
      }
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs exact-definition warning: ${String(error)}`);
      }
    }
    const resolved = await this.resolveSymbol(
      document,
      position,
      cancellationToken,
      'definition',
    );
    if (!resolved) {
      return undefined;
    }
    const { definitions } = resolved;
    const implementations = definitions.filter((definition) => definition.is_implementation);
    const preferred = implementations.length > 0 ? implementations : definitions;
    return preferred.map((definition) => {
      const range = diagnosticRange(definition);
      return new vscode.Location(
        definition.uri ? vscode.Uri.parse(definition.uri) : vscode.Uri.file(definition.path),
        new vscode.Range(
          range.startLine,
          range.startColumn,
          range.endLine,
          range.endColumn,
        ),
      );
    });
  }

  async provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.Hover | undefined> {
    const included = await this.resolveIncludedSource(document, position, cancellationToken);
    if (included) {
      const source = included.uri ? 'packed read-only game source' : 'editable source';
      const markdown = new vscode.MarkdownString();
      markdown.appendCodeblock(`#include "${included.includeName}"`, 'nwscript');
      markdown.appendMarkdown(`${source}: \`${included.path}\``);
      return new vscode.Hover(markdown, included.range);
    }
    const resolved = await this.resolveSymbol(document, position, cancellationToken, 'hover');
    if (!resolved) {
      return undefined;
    }
    let exactDefinitions = resolved.definitions;
    try {
      const references = await this.resolveReferences(document, position, cancellationToken);
      const declarations = references?.records.filter((record) => record.is_declaration) || [];
      const matched = resolved.definitions.filter((definition) =>
        declarations.some((reference) => definitionMatchesReference(definition, reference)));
      if (matched.length > 0) {
        exactDefinitions = matched;
      }
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs exact-hover warning: ${String(error)}`);
      }
    }
    const definition = selectHoverDefinition(exactDefinitions);
    if (!definition) {
      return undefined;
    }

    const contents = [];
    if (definition.signature) {
      const signature = new vscode.MarkdownString();
      signature.appendCodeblock(definition.signature, 'nwscript');
      contents.push(signature);
    }
    const formattedDocumentation = formatHoverDocumentation(definition.documentation);
    if (formattedDocumentation) {
      const documentation = new vscode.MarkdownString(formattedDocumentation);
      documentation.isTrusted = false;
      contents.push(documentation);
    }
    return contents.length > 0 ? new vscode.Hover(contents, resolved.wordRange) : undefined;
  }

  async resolveIncludedSource(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<IncludedSource | undefined> {
    const line = document.lineAt(position.line).text;
    const match = line.match(/^\s*#include\s+"([^"\r\n]+)"/u);
    if (!match) {
      return undefined;
    }
    const includeName = match[1];
    if (!includeName) return undefined;
    const valueStart = line.indexOf(includeName);
    const range = new vscode.Range(
      position.line,
      valueStart,
      position.line,
      valueStart + includeName.length,
    );
    if (!range.contains(position)) {
      return undefined;
    }
    let request: NativeDefinitionRequest;
    if (document.uri.scheme === 'nwnrs-game') {
      const context = this.virtualDocumentRequests.get(document.uri.toString());
      if (!context) {
        return undefined;
      }
      request = definitionRequestFromSourceContext(context, '', null, []);
    } else {
      const sourcePath = document.uri.fsPath;
      const projectRoot = findProjectRoot(sourcePath);
      const configuration = this.compilerConfiguration(document.uri, projectRoot);
      request = buildDefinitionRequest(sourcePath, '', {
        projectRoot,
        overlays: this.sourceOverlays(projectRoot),
        ...configuration,
      });
    }
    const source = await this.invokeNative('resolveSource', {
      ...request,
      resource: includeName,
    }, cancellationToken);
    if (!source) {
      return undefined;
    }
    if (source.uri && source.resource) {
      this.virtualDocumentRequests.set(
        source.uri,
        sourceContextFromRequest(request, source.path, source.resource),
      );
    }
    return { ...source, range, includeName };
  }

  async resolveReferences(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<ResolvedReferences | undefined> {
    const wordRange = document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/u);
    if (!wordRange) {
      return undefined;
    }
    const symbol = document.getText(wordRange);
    const qualifier = this.symbolQualifier(document, wordRange);
    let requests: NativeDefinitionRequest[];
    if (document.uri.scheme === 'nwnrs-game') {
      const context = this.virtualDocumentRequests.get(document.uri.toString());
      if (!context) {
        return undefined;
      }
      requests = [definitionRequestFromSourceContext(context, symbol, qualifier, [])];
    } else {
      const sourcePath = document.uri.fsPath;
      const origins = this.physicalDocumentRequests.get(path.resolve(sourcePath));
      if (origins?.length) {
        requests = origins.map((origin) => definitionRequestFromSourceContext(
          { ...origin, source_path: sourcePath },
          symbol,
          qualifier,
          this.sourceOverlays(origin.project_root),
        ));
      } else {
        const projectRoot = findProjectRoot(sourcePath);
        const configuration = this.compilerConfiguration(document.uri, projectRoot);
        requests = [buildDefinitionRequest(sourcePath, symbol, {
          projectRoot,
          qualifier,
          overlays: this.sourceOverlays(projectRoot),
          ...configuration,
        })];
      }
    }
    const responses = await Promise.all(requests.map((request) => this.invokeNative(
      'findReferences',
      {
        ...request,
        line: position.line + 1,
        column: position.character + 1,
      },
      cancellationToken,
    )));
    const request = requests[0];
    if (!request) {
      return undefined;
    }
    const records: NativeReference[] = [];
    const seen = new Set<string>();
    const recordContexts = new Map<string, NativeDefinitionRequest>();
    for (const [index, response] of responses.entries()) {
      for (const record of Array.isArray(response) ? response : []) {
        const key = `${record.uri || path.resolve(record.path)}:${record.range?.start_line}:${record.range?.start_column}`;
        if (!seen.has(key)) {
          seen.add(key);
          records.push(record);
          const recordRequest = requests[index];
          if (recordRequest) {
            recordContexts.set(key, recordRequest);
          }
        }
      }
    }
    for (const record of records) {
      const key = `${record.uri || path.resolve(record.path)}:${record.range?.start_line}:${record.range?.start_column}`;
      const recordRequest = recordContexts.get(key) || request;
      if (record.uri && record.resource) {
        this.virtualDocumentRequests.set(
          record.uri,
          sourceContextFromRequest(recordRequest, record.path, record.resource),
        );
      }
      if (!record.uri && path.isAbsolute(record.path)) {
        this.rememberPhysicalDocument(record.path, {
          ...recordRequest,
          source_path: record.path,
          overlays: [],
        });
      }
    }
    return { records, wordRange, request, requests, symbol };
  }

  async provideReferences(
    document: vscode.TextDocument,
    position: vscode.Position,
    referenceContext: vscode.ReferenceContext,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.Location[]> {
    try {
      const resolved = await this.resolveReferences(document, position, cancellationToken);
      if (!resolved) {
        return [];
      }
      return resolved.records
        .filter((record) => referenceContext.includeDeclaration || !record.is_declaration)
        .map((record) => referenceLocation(record));
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs references failure: ${String(error)}`);
      }
      return [];
    }
  }

  async prepareRename(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<{ range: vscode.Range; placeholder: string }> {
    const resolved = await this.resolveReferences(document, position, cancellationToken);
    if (!resolved || resolved.records.length === 0) {
      throw new Error('The symbol at this position cannot be renamed.');
    }
    if (resolved.records.some((record) => record.uri || !path.isAbsolute(record.path))) {
      throw new Error('Packed game and generated symbols are read-only and cannot be renamed.');
    }
    return { range: resolved.wordRange, placeholder: resolved.symbol };
  }

  async provideRenameEdits(
    document: vscode.TextDocument,
    position: vscode.Position,
    newName: string,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.WorkspaceEdit> {
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/u.test(newName)) {
      throw new Error(`${JSON.stringify(newName)} is not a valid NWScript identifier.`);
    }
    const resolved = await this.resolveReferences(document, position, cancellationToken);
    if (!resolved || resolved.records.length === 0) {
      throw new Error('The symbol at this position cannot be renamed.');
    }
    if (resolved.records.some((record) => record.uri || !path.isAbsolute(record.path))) {
      throw new Error('Packed game and generated symbols are read-only and cannot be renamed.');
    }
    const collisionResponses = await Promise.all(
      (resolved.requests || [resolved.request]).map((request) => this.invokeNative(
        'findDefinitions',
        { ...request, symbol: newName, qualifier: null },
        cancellationToken,
      )),
    );
    const collisions = collisionResponses.flatMap((response) =>
      Array.isArray(response) ? response : []);
    const relevantCollisions = await this.renameCollisions(
      resolved,
      collisions,
      cancellationToken,
    );
    if (relevantCollisions.length > 0) {
      throw new Error(`Rename would collide with the existing symbol ${newName}.`);
    }
    const edit = new vscode.WorkspaceEdit();
    for (const record of resolved.records) {
      const location = referenceLocation(record);
      edit.replace(location.uri, location.range, newName);
    }
    return edit;
  }

  async renameCollisions(
    resolved: ResolvedReferences,
    collisions: readonly NativeDefinition[],
    cancellationToken: vscode.CancellationToken,
  ): Promise<readonly NativeDefinition[]> {
    const local = resolved.records.find((record) => record.container && record.is_declaration)
      || resolved.records.find((record) => record.container);
    if (!local) {
      return collisions;
    }
    const sourcePath = local.path;
    const projectRoot = findProjectRoot(sourcePath);
    const configuration = this.compilerConfiguration(vscode.Uri.file(sourcePath), projectRoot);
    const request = buildDocumentSymbolsRequest(sourcePath, {
      projectRoot,
      overlays: this.sourceOverlays(projectRoot),
      ...configuration,
    });
    const symbols = await this.invokeNative('listDocumentSymbols', request, cancellationToken);
    if (!local.container) {
      return collisions.filter((collision) => path.resolve(collision.path) === path.resolve(sourcePath));
    }
    const container = findDocumentSymbol(symbols, local.container, 'function');
    if (!container) {
      return collisions.filter((collision) => path.resolve(collision.path) === path.resolve(sourcePath));
    }
    const range = diagnosticRange(container.range || {});
    return collisions.filter((collision) =>
      path.resolve(collision.path) === path.resolve(sourcePath)
      && collision.start_line - 1 >= range.startLine
      && collision.start_line - 1 <= range.endLine);
  }

  async provideWorkspaceSymbols(
    search: string,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.SymbolInformation[]> {
    const files = await vscode.workspace.findFiles(
      '**/*.nss',
      '**/{.git,node_modules,target}/**',
    );
    const results: vscode.SymbolInformation[] = [];
    const projects = new Map<string, vscode.Uri>();
    for (const uri of files) {
      const projectRoot = findProjectRoot(uri.fsPath);
      if (!projects.has(projectRoot)) {
        projects.set(projectRoot, uri);
      }
    }
    for (const [projectRoot, representative] of projects) {
      if (cancellationToken.isCancellationRequested) {
        return results;
      }
      const configuration = this.compilerConfiguration(representative, projectRoot);
      const request = buildDocumentSymbolsRequest(representative.fsPath, {
        projectRoot,
        overlays: this.sourceOverlays(projectRoot),
        ...configuration,
      });
      try {
        const index = await this.invokeNative('indexProject', request, cancellationToken);
        for (const warning of index?.warnings || []) {
          this.output.appendLine(`nwnrs project-index warning: ${warning}`);
        }
        for (const document of index?.documents || []) {
          this.rememberPhysicalDocument(document.path, {
            ...request,
            source_path: document.path,
            overlays: [],
          });
          appendWorkspaceSymbols(
            results,
            document.symbols,
            vscode.Uri.file(document.path),
            search,
            '',
          );
        }
      } catch (error) {
        if (!cancellationToken.isCancellationRequested) {
          this.output.appendLine(`nwnrs workspace-symbol warning: ${String(error)}`);
        }
      }
    }
    return results;
  }

  async prepareCallHierarchy(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.CallHierarchyItem[]> {
    const resolved = await this.resolveSymbol(document, position, cancellationToken, 'call hierarchy');
    if (!resolved) {
      return [];
    }
    const definition = resolved.definitions.find((candidate) =>
      candidate.kind === 'function' || candidate.kind === 'builtinFunction');
    if (!definition || definition.uri) {
      return [];
    }
    return [callHierarchyItem(definition, {
      ...resolved.request,
      source_path: definition.path,
      overlays: this.sourceOverlays(resolved.request.project_root),
    })];
  }

  async provideIncomingCalls(
    item: NwnrsCallHierarchyItem,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.CallHierarchyIncomingCall[]> {
    const request = item._nwnrsRequest;
    if (!request) {
      return [];
    }
    const references = await this.invokeNative('findReferences', {
      ...request,
      symbol: item.name,
      qualifier: null,
      line: item.selectionRange.start.line + 1,
      column: item.selectionRange.start.character + 1,
    }, cancellationToken);
    const groups = new Map();
    for (const reference of references || []) {
      if (reference.is_declaration || !reference.container || reference.uri) {
        continue;
      }
      const key = `${reference.path}:${reference.container}`;
      const group = groups.get(key) || { reference, ranges: [] };
      group.ranges.push(referenceLocation(reference).range);
      groups.set(key, group);
    }
    const incoming = [];
    for (const group of groups.values()) {
      const callerRoot = request.project_root || findProjectRoot(group.reference.path);
      const callerRequest = {
        ...request,
        source_path: group.reference.path,
        project_root: callerRoot,
        symbol: group.reference.container,
        qualifier: null,
        overlays: this.sourceOverlays(callerRoot),
      };
      const definitions = await this.invokeNative('findDefinitions', callerRequest, cancellationToken);
      const caller = definitions.find((definition) => definition.kind === 'function');
      if (caller) {
        incoming.push(new vscode.CallHierarchyIncomingCall(
          callHierarchyItem(caller, callerRequest),
          group.ranges,
        ));
      }
    }
    return incoming;
  }

  async provideOutgoingCalls(
    item: NwnrsCallHierarchyItem,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.CallHierarchyOutgoingCall[]> {
    const request = item._nwnrsRequest;
    if (!request) {
      return [];
    }
    const calls = await this.invokeNative('findOutgoingCalls', {
      ...request,
      symbol: item.name,
      qualifier: null,
      line: item.selectionRange.start.line + 1,
      column: item.selectionRange.start.character + 1,
    }, cancellationToken);
    return (calls || []).map((call) => {
      const targetRoot = request.project_root;
      const targetRequest = {
        ...request,
        source_path: call.target.path,
        project_root: targetRoot,
        symbol: call.target.name,
        overlays: path.isAbsolute(call.target.path) ? this.sourceOverlays(targetRoot) : [],
      };
      return new vscode.CallHierarchyOutgoingCall(
        callHierarchyItem(call.target, targetRequest),
        call.ranges.map((range) => {
          const mapped = diagnosticRange(range);
          return new vscode.Range(
            mapped.startLine,
            mapped.startColumn,
            mapped.endLine,
            mapped.endColumn,
          );
        }),
      );
    });
  }

  async runCompiler(
    request: CompilerRunRequest,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<void> {
    const configuration = this.compilerConfiguration(request.sourceUri, request.cwd);
    void this.ensureExternalWatchRoots(request.targets, configuration.includeDirectories);
    const checkRequest = buildCheckRequest(request.targets, {
      ...configuration,
      recurse: request.recurse,
      overlays: request.overlays,
    });
    const fingerprint = JSON.stringify({ epoch: this.changeEpoch, request: checkRequest });
    const sequence = ++this.sequence;
    const previous = this.activeRuns.get(request.key);
    if (!request.force && previous?.fingerprint === fingerprint) {
      return;
    }
    if (previous) {
      previous.cancellation.cancel();
      previous.cancellation.dispose();
    }

    this.status.text = '$(sync~spin) nwnrs';
    this.status.tooltip = `Checking ${request.targets.join(', ')}`;
    this.output.appendLine(`[check] ${request.targets.join(', ')}`);

    const runCancellation = new vscode.CancellationTokenSource();
    const linkedCancellation = cancellationToken?.onCancellationRequested(() => {
      runCancellation.cancel();
    });
    this.activeRuns.set(request.key, {
      cancellation: runCancellation,
      sequence,
      fingerprint,
    });

    let response;
    try {
      response = await this.invokeNative('checkNss', checkRequest, runCancellation.token);
    } catch (error) {
      const active = this.activeRuns.get(request.key);
      if (!active || active.sequence !== sequence || runCancellation.token.isCancellationRequested) {
        return;
      }
      this.activeRuns.delete(request.key);
      this.reportLaunchFailure(request, String(error), request.revealFailure);
      return;
    } finally {
      linkedCancellation?.dispose();
      runCancellation.dispose();
    }
    const active = this.activeRuns.get(request.key);
    if (!active || active.sequence !== sequence) {
      return;
    }
    this.activeRuns.delete(request.key);

    if (cancellationToken?.isCancellationRequested) {
      this.output.appendLine(`[cancelled] ${request.targets.join(', ')}`);
      if (this.activeRuns.size === 0) {
        this.status.text = '$(circle-slash) nwnrs';
        this.status.tooltip = 'NWScript check cancelled';
      }
      return;
    }

    response ||= { diagnostics: [], summary: undefined };
    const records = Array.isArray(response.diagnostics) ? response.diagnostics : [];
    const mapped = await this.mapDiagnostics(records, request);
    this.runDiagnostics.set(request.key, mapped);
    this.publishDiagnostics();

    const failed = response.summary?.failed ?? records.length;
    const compiled = response.summary?.compiled ?? 0;
    const skipped = response.summary?.skipped ?? 0;
    this.output.appendLine(
      `[done] ${compiled} compiled, ${skipped} skipped, ${failed} failed`,
    );
    this.status.text = failed > 0 ? `$(error) nwnrs ${failed}` : '$(check) nwnrs';
    this.status.tooltip = failed > 0
      ? `${failed} NWScript compilation(s) failed`
      : 'NWScript check passed';
  }

  async mapDiagnostics(
    records: readonly NativeCheckDiagnostic[],
    request: CompilerRunRequest,
  ): Promise<Map<string, DiagnosticRunEntry>> {
    const mapped = new Map<string, DiagnosticRunEntry>();
    for (const record of records) {
      const uri = await this.resolveDiagnosticUri(record, request);
      const range = diagnosticRange(record);
      const diagnostic = new vscode.Diagnostic(
        new vscode.Range(
          range.startLine,
          range.startColumn,
          range.endLine,
          range.endColumn,
        ),
        String(record.message || 'NWScript compilation failed'),
        severity(record.severity),
      );
      diagnostic.source = DIAGNOSTIC_OWNER;
      if (record.code !== null && Number.isInteger(record.code)) {
        diagnostic.code = record.code;
      }
      const key = uri.toString();
      const entry = mapped.get(key) || { uri, diagnostics: [] };
      entry.diagnostics.push(diagnostic);
      mapped.set(key, entry);
    }
    return mapped;
  }

  async resolveDiagnosticUri(
    record: NativeCheckDiagnostic,
    request: CompilerRunRequest,
  ): Promise<vscode.Uri> {
    const raw = String(record.file || record.input || request.sourceUri.fsPath);
    if (path.isAbsolute(raw)) {
      return vscode.Uri.file(raw);
    }
    const input = String(record.input || request.sourceUri.fsPath);
    const candidates = [
      path.resolve(request.cwd, raw),
      path.resolve(path.dirname(input), raw),
    ];
    const existing = candidates.find((candidate) => fs.existsSync(candidate));
    if (existing) {
      return vscode.Uri.file(existing);
    }
    const matches = await vscode.workspace.findFiles(
      `**/${path.basename(raw)}`,
      '**/{.git,node_modules,target}/**',
      2,
    );
    return matches[0] || request.sourceUri;
  }

  publishDiagnostics() {
    const combined = new Map();
    for (const run of this.runDiagnostics.values()) {
      for (const [key, entry] of run) {
        const aggregate = combined.get(key) || { uri: entry.uri, diagnostics: [] };
        aggregate.diagnostics.push(...entry.diagnostics);
        combined.set(key, aggregate);
      }
    }
    this.diagnostics.clear();
    for (const entry of combined.values()) {
      this.diagnostics.set(entry.uri, entry.diagnostics);
    }
  }

  reportLaunchFailure(
    request: CompilerRunRequest,
    message: string,
    reveal: boolean,
  ): void {
    this.output.appendLine(`nwnrs compiler failure: ${message}`);
    this.status.text = '$(warning) nwnrs';
    this.status.tooltip = message;
    if (reveal) {
      this.output.show(true);
      void vscode.window.showErrorMessage(`nwnrs compiler failure: ${message}`);
    }
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(0, 0, 0, 1),
      message,
      vscode.DiagnosticSeverity.Error,
    );
    diagnostic.source = DIAGNOSTIC_OWNER;
    this.runDiagnostics.set(request.key, new Map([
      [request.sourceUri.toString(), { uri: request.sourceUri, diagnostics: [diagnostic] }],
    ]));
    this.publishDiagnostics();
  }
}

function recordString(value: unknown, key: string): string | undefined {
  if (typeof value !== 'object' || value === null) {
    return undefined;
  }
  const field = (value as Record<string, unknown>)[key];
  return typeof field === 'string' ? field : undefined;
}

function recordStringArray(value: unknown, key: string): string[] | undefined {
  if (typeof value !== 'object' || value === null) {
    return undefined;
  }
  const field = (value as Record<string, unknown>)[key];
  return Array.isArray(field) && field.every((entry) => typeof entry === 'string')
    ? field
    : undefined;
}

function sourceContextFromRequest(
  request: NativeDefinitionRequest | NativeDocumentSymbolsRequest,
  sourcePath: string = request.source_path,
  resource: string | null = 'resource' in request ? request.resource : null,
): NativeDocumentSymbolsRequest {
  return {
    source_path: sourcePath,
    resource,
    project_root: request.project_root,
    include_dirs: [...request.include_dirs],
    overlays: [...request.overlays],
    langspec: request.langspec,
    max_include_depth: request.max_include_depth,
    root: request.root,
    user: request.user,
    language: request.language,
    load_ovr: request.load_ovr,
  };
}

function definitionRequestFromSourceContext(
  context: NativeDocumentSymbolsRequest,
  symbol: string,
  qualifier: string | null,
  overlays: readonly SourceOverlay[],
): NativeDefinitionRequest {
  return {
    source_path: context.source_path,
    symbol,
    qualifier,
    project_root: context.project_root,
    include_dirs: [...context.include_dirs],
    overlays: [...overlays],
    langspec: context.langspec,
    max_include_depth: context.max_include_depth,
    root: context.root,
    user: context.user,
    language: context.language,
    load_ovr: context.load_ovr,
  };
}

function severity(value: string): vscode.DiagnosticSeverity {
  switch (value) {
    case 'warning':
      return vscode.DiagnosticSeverity.Warning;
    case 'information':
      return vscode.DiagnosticSeverity.Information;
    case 'hint':
      return vscode.DiagnosticSeverity.Hint;
    default:
      return vscode.DiagnosticSeverity.Error;
  }
}

const NWPKG_SECTIONS: Readonly<Record<string, string>> = {
  project: 'Project identity and output kind.',
  source: 'Source directory for authored package contents.',
  dependencies: 'Local include-package dependencies keyed by package name.',
};

const NWPKG_FIELDS: Readonly<Record<string, string>> = {
  'project.name': 'Stable project name used in package metadata and diagnostics.',
  'project.kind': 'Package layout and output resource type.',
  'source.path': 'Source directory, resolved relative to this nwpkg.toml.',
  'dependencies.path': 'Local include-package root, resolved relative to this nwpkg.toml.',
};

const NWPKG_KINDS = [
  '2da', 'are', 'bic', 'dds', 'dlg', 'erf', 'git', 'hak', 'ifo', 'include',
  'itp', 'jrl', 'key', 'mdl', 'mod', 'ncs', 'nwm', 'plt', 'ssf', 'tga',
  'tlk', 'utc', 'utd', 'ute', 'uti', 'utm', 'utp', 'uts', 'utt', 'utw',
];

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
}

function includeInsertion(
  document: vscode.TextDocument,
  includeName: string,
): { readonly position: vscode.Position; readonly text: string } {
  let lastInclude = -1;
  for (let line = 0; line < document.lineCount; line += 1) {
    if (/^\s*#include\s+"[^"]+"/u.test(document.lineAt(line).text)) {
      lastInclude = line;
    }
  }
  if (lastInclude >= 0) {
    return {
      position: document.lineAt(lastInclude).range.end,
      text: `\n#include "${includeName}"`,
    };
  }
  return {
    position: new vscode.Position(0, 0),
    text: `#include "${includeName}"\n\n`,
  };
}

function nwpkgSection(document: vscode.TextDocument, line: number): string {
  for (let index = line; index >= 0; index -= 1) {
    const match = document.lineAt(index).text.match(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/u);
    if (match?.[1]) {
      return match[1];
    }
  }
  return '';
}

function nwpkgCompletions(
  document: vscode.TextDocument,
  position: vscode.Position,
): vscode.CompletionItem[] {
  const line = document.lineAt(position.line).text;
  const prefix = line.slice(0, position.character);
  const section = nwpkgSection(document, position.line);
  const pathValue = prefix.match(/^\s*(?:[A-Za-z0-9_-]+\s*=\s*\{\s*)?path\s*=\s*"([^"\r\n]*)$/u);
  if (pathValue?.[1] !== undefined && (section === 'source' || section === 'dependencies')) {
    return nwpkgPathCompletions(document, position, pathValue[1]);
  }
  if (/^\s*\[[A-Za-z]*$/u.test(prefix)) {
    return Object.entries(NWPKG_SECTIONS).map(([name, documentation]) => {
      const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Module);
      item.detail = `[${name}]`;
      item.documentation = documentation;
      item.insertText = `${name}]`;
      return item;
    });
  }
  const kindValue = prefix.match(/^\s*kind\s*=\s*("?)[A-Za-z0-9]*$/u);
  if (section === 'project' && kindValue) {
    return NWPKG_KINDS.map((kind) => {
      const item = new vscode.CompletionItem(kind, vscode.CompletionItemKind.EnumMember);
      item.insertText = kindValue[1] ? kind : `"${kind}"`;
      item.detail = 'nwpkg project kind';
      return item;
    });
  }
  const fields: readonly (readonly [string, string])[] = section === 'project'
    ? [['name', '"${1:project}"'], ['kind', '"${1:mod}"']]
    : section === 'source'
      ? [['path', '"${1:.}"']]
      : section === 'dependencies'
        ? [['package', '{ path = "${1:../include/package}" }']]
        : [];
  return fields.map(([name, value]) => {
    const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Property);
    item.insertText = new vscode.SnippetString(section === 'dependencies'
      ? '${1:package} = { path = "${2:../include/package}" }'
      : `${name} = ${value}`);
    item.detail = section === 'dependencies'
      ? 'Local include dependency'
      : NWPKG_FIELDS[`${section}.${name}`];
    return item;
  });
}

function nwpkgPathCompletions(
  document: vscode.TextDocument,
  position: vscode.Position,
  value: string,
): vscode.CompletionItem[] {
  const slash = Math.max(value.lastIndexOf('/'), value.lastIndexOf(path.sep));
  const directoryPart = slash >= 0 ? value.slice(0, slash + 1) : '';
  const namePart = slash >= 0 ? value.slice(slash + 1) : value;
  const directory = path.resolve(path.dirname(document.uri.fsPath), directoryPart || '.');
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(directory, { withFileTypes: true });
  } catch {
    return [];
  }
  const replaceStart = position.character - namePart.length;
  return entries
    .filter((entry) => entry.isDirectory() && entry.name.startsWith(namePart))
    .slice(0, 200)
    .map((entry) => {
      const item = new vscode.CompletionItem(entry.name, vscode.CompletionItemKind.Folder);
      item.range = new vscode.Range(
        position.line,
        replaceStart,
        position.line,
        position.character,
      );
      item.insertText = `${entry.name}/`;
      item.command = { command: 'editor.action.triggerSuggest', title: 'Continue path completion' };
      return item;
    });
}

function nwpkgHover(
  document: vscode.TextDocument,
  position: vscode.Position,
): vscode.Hover | undefined {
  const line = document.lineAt(position.line).text;
  const sectionMatch = line.match(/^\s*\[([^\]]+)\]/u);
  const sectionName = sectionMatch?.[1];
  if (sectionName) {
    const documentation = NWPKG_SECTIONS[sectionName];
    if (documentation) {
      return new vscode.Hover(documentation);
    }
  }
  const keyMatch = line.match(/^\s*([A-Za-z0-9_-]+)\s*=/u);
  if (!keyMatch) {
    return undefined;
  }
  const section = nwpkgSection(document, position.line);
  const key = keyMatch[1];
  if (!key) {
    return undefined;
  }
  const schemaKey = section === 'dependencies' ? 'dependencies.path' : `${section}.${key}`;
  const documentation = NWPKG_FIELDS[schemaKey];
  return documentation ? new vscode.Hover(documentation) : undefined;
}

function nwpkgDefinition(
  document: vscode.TextDocument,
  position: vscode.Position,
): vscode.Location | undefined {
  const line = document.lineAt(position.line).text;
  const section = nwpkgSection(document, position.line);
  const quotedValues = [...line.matchAll(/"([^"\r\n]+)"/gu)];
  const value = quotedValues.find((match) => {
    const matchedValue = match[1];
    if (matchedValue === undefined) {
      return false;
    }
    const start = (match.index ?? 0) + 1;
    const end = start + matchedValue.length;
    return position.character >= start && position.character <= end;
  });
  if (!value || (section !== 'source' && section !== 'dependencies')) {
    return undefined;
  }
  const matchedValue = value[1];
  if (!matchedValue) {
    return undefined;
  }
  const resolved = path.resolve(path.dirname(document.uri.fsPath), matchedValue);
  if (!fs.existsSync(resolved)) {
    return undefined;
  }
  const target = section === 'dependencies' && fs.statSync(resolved).isDirectory()
    ? path.join(resolved, 'nwpkg.toml')
    : resolved;
  if (!fs.existsSync(target) || fs.statSync(target).isDirectory()) {
    return undefined;
  }
  return new vscode.Location(vscode.Uri.file(target), new vscode.Position(0, 0));
}

function nwpkgDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
  const symbols: vscode.DocumentSymbol[] = [];
  let current: vscode.DocumentSymbol | undefined;
  for (let line = 0; line < document.lineCount; line += 1) {
    const text = document.lineAt(line).text;
    const section = text.match(/^\s*\[([^\]]+)\]/u);
    const sectionName = section?.[1];
    if (sectionName) {
      const start = text.indexOf(sectionName);
      current = new vscode.DocumentSymbol(
        sectionName,
        'manifest section',
        vscode.SymbolKind.Namespace,
        document.lineAt(line).range,
        new vscode.Range(line, start, line, start + sectionName.length),
      );
      current.children = [];
      symbols.push(current);
      continue;
    }
    const field = text.match(/^\s*([A-Za-z0-9_-]+)\s*=/u);
    if (!field) {
      continue;
    }
    const fieldName = field[1];
    if (!fieldName) {
      continue;
    }
    const start = text.indexOf(fieldName);
    const symbol = new vscode.DocumentSymbol(
      fieldName,
      text.slice(text.indexOf('=') + 1).trim(),
      vscode.SymbolKind.Property,
      document.lineAt(line).range,
      new vscode.Range(line, start, line, start + fieldName.length),
    );
    if (current) {
      current.children.push(symbol);
      current.range = new vscode.Range(current.range.start, document.lineAt(line).range.end);
    } else {
      symbols.push(symbol);
    }
  }
  return symbols;
}

function documentSymbol(record: NativeDocumentSymbol): vscode.DocumentSymbol {
  const full = diagnosticRange(record.range);
  const selection = diagnosticRange(record.selection_range);
  const symbol = new vscode.DocumentSymbol(
    String(record.name || ''),
    typeof record.detail === 'string' ? record.detail : '',
    documentSymbolKind(record.kind),
    new vscode.Range(full.startLine, full.startColumn, full.endLine, full.endColumn),
    new vscode.Range(
      selection.startLine,
      selection.startColumn,
      selection.endLine,
      selection.endColumn,
    ),
  );
  symbol.children = record.children.map((child) => documentSymbol(child));
  return symbol;
}

function referenceLocation(record: NativeReference): vscode.Location {
  const range = diagnosticRange(record.range);
  return new vscode.Location(
    record.uri ? vscode.Uri.parse(record.uri) : vscode.Uri.file(record.path),
    new vscode.Range(range.startLine, range.startColumn, range.endLine, range.endColumn),
  );
}

function definitionMatchesReference(
  definition: NativeDefinition,
  reference: NativeReference,
): boolean {
  const sameSource = definition.uri && reference.uri
    ? definition.uri === reference.uri
    : path.resolve(definition.path) === path.resolve(reference.path);
  return sameSource
    && definition.start_line === reference.range?.start_line
    && definition.start_column === reference.range?.start_column;
}

function callHierarchyItem(
  definition: NativeDefinition,
  request: NativeDefinitionRequest,
): NwnrsCallHierarchyItem {
  const range = diagnosticRange(definition);
  const selection = new vscode.Range(
    range.startLine,
    range.startColumn,
    range.endLine,
    range.endColumn,
  );
  const item = new vscode.CallHierarchyItem(
    vscode.SymbolKind.Function,
    definition.name,
    definition.signature || '',
    definition.uri ? vscode.Uri.parse(definition.uri) : vscode.Uri.file(definition.path),
    selection,
    selection,
  );
  return Object.assign(item, { _nwnrsRequest: request });
}

function appendWorkspaceSymbols(
  results: vscode.SymbolInformation[],
  records: readonly NativeDocumentSymbol[],
  uri: vscode.Uri,
  search: string,
  container: string,
): void {
  const normalizedSearch = search.trim().toLocaleLowerCase();
  for (const record of records) {
    const range = diagnosticRange(record.selection_range);
    if (!normalizedSearch || String(record.name).toLocaleLowerCase().includes(normalizedSearch)) {
      results.push(new vscode.SymbolInformation(
        record.name,
        documentSymbolKind(record.kind),
        container,
        new vscode.Location(
          uri,
          new vscode.Range(range.startLine, range.startColumn, range.endLine, range.endColumn),
        ),
      ));
    }
    appendWorkspaceSymbols(results, record.children, uri, search, record.name);
  }
}

function findDocumentSymbol(
  records: readonly NativeDocumentSymbol[],
  name: string,
  kind: string,
): NativeDocumentSymbol | undefined {
  for (const record of records) {
    if (record.name === name && record.kind === kind) {
      return record;
    }
    const nested = findDocumentSymbol(record.children, name, kind);
    if (nested) {
      return nested;
    }
  }
  return undefined;
}

function documentSymbolKind(kind: string): vscode.SymbolKind {
  switch (kind) {
    case 'function':
      return vscode.SymbolKind.Function;
    case 'variable':
      return vscode.SymbolKind.Variable;
    case 'struct':
      return vscode.SymbolKind.Struct;
    case 'field':
      return vscode.SymbolKind.Field;
    case 'enum':
      return vscode.SymbolKind.Enum;
    case 'enumVariant':
      return vscode.SymbolKind.EnumMember;
    case 'typeAlias':
      return vscode.SymbolKind.TypeParameter;
    case 'constant':
      return vscode.SymbolKind.Constant;
    case 'macro':
      return vscode.SymbolKind.Namespace;
    default:
      return vscode.SymbolKind.Object;
  }
}

export function activate(context: vscode.ExtensionContext): void {
  const controller = new CompilerController(context);
  controller.register();
  const resourceEditors = new ResourceCustomEditorProvider(context, controller.output);
  resourceEditors.register();
  const sidebar = new NwnrsSidebarController(
    context,
    controller.output,
    resourceEditors.viewerWorker,
    resourceEditors,
    controller,
  );
  sidebar.register();
}

export function deactivate(): void {}

export { CompilerController };
