import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
  LanguageWorkerClient,
  type NativeLanguageResponse,
  type NativeLanguageRequest,
  type WorkerLanguageMethod,
} from './language-worker-client';
import type { WorkerProgressEvent } from './worker-client';
import { ResourceCustomEditorProvider } from './resource-custom-editor';
import { NwnrsSidebarController } from './sidebar';
import {
  buildEventCatalogDocument,
  eventHoverMarkdown,
  eventRouteKey,
  type EventCatalogDocument,
} from './event-catalog';
import type {
  NativeDefinition,
  NativeDocumentSymbol,
  NativeEventCatalogEntry,
  NativeEventPositionRequest,
  NativeEventSymbol,
  NativeDependencyPlan,
  NativeDependencySource,
  NativeDependencyStatus,
  NativeReference,
  NativeLanguageDiagnostic,
  NativePositionRequest,
  NativeRange,
} from './generated/native';
import {
  buildCheckRequest,
  buildDefinitionRequest,
  buildDocumentSymbolsRequest,
  diagnosticRange,
  formatHoverDocumentation,
  isNssPath,
  nativeBindingPath,
  resolveConfiguredPath,
  selectHoverDefinition,
  type NativeCheckRequest,
  type NativeDefinitionRequest,
  type NativeDocumentSymbolsRequest,
} from './compiler';
import { unrepresentableUtf16Ranges } from './nwscript-encoding';

interface CompilerConfiguration {
  readonly includeDirectories: string[];
  readonly langspecPath: string;
  readonly rootPath: string;
  readonly userPath: string;
  readonly language: string;
  readonly loadOvr: boolean;
  readonly gitExecutable: string;
  readonly packageCache: string;
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
const NWNSCRIPT_ENCODING = 'windows1252';
const EVENT_CATALOG_URI = vscode.Uri.parse('nwnrs-events:/catalog.md');
const DEPENDENCY_SOURCE_SCHEME = 'nwnrs-package';
const SEMANTIC_TOKEN_TYPES = [
  'function', 'parameter', 'variable', 'property', 'type', 'enum', 'enumMember', 'macro', 'event',
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
  private readonly encodingDiagnostics: vscode.DiagnosticCollection;
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
  private readonly encodingWarnings: Set<string>;
  private eventCatalogPromise: Promise<readonly NativeEventCatalogEntry[]> | undefined;
  private eventCatalogDocument: EventCatalogDocument | undefined;

  public constructor(context: vscode.ExtensionContext) {
    this.context = context;
    this.diagnostics = vscode.languages.createDiagnosticCollection(DIAGNOSTIC_OWNER);
    this.encodingDiagnostics = vscode.languages.createDiagnosticCollection('nwnrs encoding');
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
    this.encodingWarnings = new Set();
    this.languageWorker = new LanguageWorkerClient(
      path.join(__dirname, 'compiler-worker.js'),
      nativeBindingPath(context.extensionPath),
      this.output,
    );
    this.languageWorker.onReady(() => {
      void this.synchronizeAllOpenDocuments();
    });
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      this.languageWorker.addWorkspace(folder.uri.fsPath);
    }
    for (const document of vscode.workspace.textDocuments) {
      if (document.uri.scheme === 'file'
        && (this.isNwScriptDocument(document) || this.isNwpkgDocument(document))) {
        void this.synchronizeDocument(document, 'open');
      }
    }

    context.subscriptions.push(
      this.diagnostics,
      this.encodingDiagnostics,
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
      vscode.commands.registerCommand(
        'nwnrs.nwpkg.fetchDependencies',
        (manifestPath?: string, dependency?: string) =>
          this.fetchPackageDependencies(manifestPath, dependency),
      ),
      vscode.commands.registerCommand(
        'nwnrs.nwpkg.updateDependencies',
        (manifestPath?: string, dependency?: string) =>
          this.updatePackageDependencies(manifestPath, dependency),
      ),
      vscode.commands.registerCommand(
        'nwnrs.nwpkg.cleanCache',
        () => this.cleanPackageDependencyCache(),
      ),
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
        if (event.document.uri.scheme === 'file'
          && (this.isNwScriptDocument(event.document) || this.isNwpkgDocument(event.document))) {
          void this.synchronizeDocument(event.document, 'update').then((valid) => {
            if (!valid || !this.isNwScriptDocument(event.document)) return;
            this.invalidateChecks(event.document);
            const config = vscode.workspace.getConfiguration('nwnrs', event.document.uri);
            if (config.get('checkOnChange', true)) {
              this.scheduleDocument(event.document);
            }
          });
        }
        if (this.isNwpkgDocument(event.document)) {
          this.invalidateChecks(event.document);
          this.scheduleManifest(event.document);
          return;
        }
        if (!this.isNwScriptDocument(event.document)) {
          return;
        }
      }),
      vscode.workspace.onDidOpenTextDocument((document) => {
        if (document.uri.scheme === 'file'
          && (this.isNwScriptDocument(document) || this.isNwpkgDocument(document))) {
          void this.synchronizeDocument(document, 'open').then((valid) => {
            if (!valid || !this.isNwScriptDocument(document)) return;
            this.invalidateChecks(document);
            const config = vscode.workspace.getConfiguration('nwnrs', document.uri);
            if (config.get('checkOnOpen', true)) {
              this.scheduleDocument(document);
            }
          });
        }
        if (this.isNwpkgDocument(document)) {
          this.scheduleManifest(document);
          return;
        }
        if (!this.isNwScriptDocument(document)) {
          return;
        }
      }),
      vscode.workspace.onDidCloseTextDocument((document) => {
        if (document.uri.scheme === 'file'
          && (this.isNwScriptDocument(document) || this.isNwpkgDocument(document))) {
          this.languageWorker.closeDocument(document.uri.fsPath);
          this.encodingWarnings.delete(document.uri.toString());
          this.encodingDiagnostics.delete(document.uri);
        }
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
      vscode.workspace.onDidChangeWorkspaceFolders((event) => {
        for (const folder of event.removed) {
          this.languageWorker.removeWorkspace(folder.uri.fsPath);
        }
        for (const folder of event.added) {
          this.languageWorker.addWorkspace(folder.uri.fsPath);
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
      vscode.languages.registerCompletionItemProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideCompletionItems: (document, position, token) =>
            this.provideNwScriptCompletions(document, position, token),
        },
        '(',
        ',',
        ':',
        '.',
      ),
      vscode.languages.registerSignatureHelpProvider(
        [
          { language: 'nwscript', scheme: 'file' },
          { language: 'nwscript', scheme: 'nwnrs-game' },
        ],
        {
          provideSignatureHelp: (document, position, token) =>
            this.provideSignatureHelp(document, position, token),
        },
        '(',
        ',',
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
      vscode.languages.registerCodeActionsProvider(
        { language: 'nwpkg', scheme: 'file' },
        {
          provideCodeActions: (document, _range, actionContext) =>
            this.provideManifestCodeActions(document, actionContext),
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
        {
          provideCompletionItems: (document, position, token) =>
            this.provideManifestCompletions(document, position, token),
        },
        '[', '.', '/', '"',
      ),
      vscode.languages.registerHoverProvider(
        { language: 'nwpkg', scheme: 'file' },
        {
          provideHover: (document, position, token) =>
            this.provideManifestHover(document, position, token),
        },
      ),
      vscode.languages.registerDefinitionProvider(
        { language: 'nwpkg', scheme: 'file' },
        {
          provideDefinition: (document, position, token) =>
            this.provideManifestDefinition(document, position, token),
        },
      ),
      vscode.languages.registerDocumentSymbolProvider(
        { language: 'nwpkg', scheme: 'file' },
        {
          provideDocumentSymbols: (document, token) =>
            this.provideManifestSymbols(document, token),
        },
      ),
      vscode.workspace.registerTextDocumentContentProvider('nwnrs-game', {
        provideTextDocumentContent: (uri, token) => this.provideVirtualSource(uri, token),
      }),
      vscode.workspace.registerTextDocumentContentProvider('nwnrs-events', {
        provideTextDocumentContent: (_uri, token) => this.provideEventCatalogDocument(token),
      }),
      vscode.workspace.registerTextDocumentContentProvider(DEPENDENCY_SOURCE_SCHEME, {
        provideTextDocumentContent: (uri, token) => this.provideDependencySource(uri, token),
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
        this.rememberPhysicalDocument(document.path, request);
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
      if (!request?.projectRoot || !request.sourcePath) {
        return undefined;
      }
      return {
        projectRoot: path.resolve(request.projectRoot),
        representative: vscode.Uri.file(request.sourcePath),
      };
    }
    if (document.uri.scheme !== 'file') {
      return undefined;
    }
    const projectRoot = this.isNwpkgDocument(document)
      ? path.dirname(document.uri.fsPath)
      : await this.resolveProjectRoot(document.uri.fsPath);
    if (this.isNwScriptDocument(document)) {
      return { projectRoot, representative: document.uri };
    }
    return this.isNwpkgDocument(document)
      ? { projectRoot, representative: vscode.Uri.file(projectRoot) }
      : undefined;
  }

  private async synchronizeAllOpenDocuments(): Promise<void> {
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      this.languageWorker.addWorkspace(folder.uri.fsPath);
    }
    await Promise.all(vscode.workspace.textDocuments.map(async (document) => {
      if (document.uri.scheme === 'file'
        && (this.isNwScriptDocument(document) || this.isNwpkgDocument(document))) {
        await this.synchronizeDocument(document, 'open');
      }
    }));
  }

  private async synchronizeDocument(
    document: vscode.TextDocument,
    operation: 'open' | 'update',
  ): Promise<boolean> {
    if (document.uri.scheme !== 'file') return false;
    const version = document.version;
    if (this.isNwpkgDocument(document)) {
      const bytes = new TextEncoder().encode(document.getText());
      if (document.version !== version) return false;
      if (operation === 'open') {
        this.languageWorker.openDocument(document.uri.fsPath, version, bytes);
      } else {
        this.languageWorker.updateDocument(document.uri.fsPath, version, bytes);
      }
      return true;
    }
    if (!this.isNwScriptDocument(document)) return false;

    if (document.encoding !== NWNSCRIPT_ENCODING) {
      this.suspendDocumentAnalysis(document);
      this.languageWorker.invalidateDocument(document.uri.fsPath, version);
      await this.warnIncorrectNwScriptEncoding(document);
      return false;
    }

    const source = document.getText();
    const bytes = await vscode.workspace.encode(source, {
      encoding: NWNSCRIPT_ENCODING,
    });
    const decoded = await vscode.workspace.decode(bytes, {
      encoding: NWNSCRIPT_ENCODING,
    });
    if (document.version !== version) return false;

    if (decoded !== source) {
      this.suspendDocumentAnalysis(document);
      this.encodingDiagnostics.set(
        document.uri,
        unrepresentableEncodingDiagnostics(document, source, decoded),
      );
      this.languageWorker.invalidateDocument(document.uri.fsPath, version);
      return false;
    }

    this.encodingDiagnostics.delete(document.uri);
    this.encodingWarnings.delete(document.uri.toString());
    if (operation === 'open') {
      this.languageWorker.openDocument(document.uri.fsPath, version, bytes);
    } else {
      this.languageWorker.updateDocument(document.uri.fsPath, version, bytes);
    }
    return true;
  }

  private suspendDocumentAnalysis(document: vscode.TextDocument): void {
    const key = this.documentKey(document);
    const timer = this.timers.get(key);
    if (timer) {
      clearTimeout(timer);
      this.timers.delete(key);
    }
    const active = this.activeRuns.get(key);
    if (active) {
      active.cancellation.cancel();
      this.activeRuns.delete(key);
    }
    this.changeEpoch += 1;
    this.diagnostics.delete(document.uri);
    this.runDiagnostics.delete(key);
  }

  private async warnIncorrectNwScriptEncoding(document: vscode.TextDocument): Promise<void> {
    const key = document.uri.toString();
    if (this.encodingWarnings.has(key)) return;
    this.encodingWarnings.add(key);
    const action = 'Reopen as Windows-1252';
    const selected = await vscode.window.showWarningMessage(
      `NWScript files must use Windows-1252. ${path.basename(document.uri.fsPath)} is open as `
      + `${document.encoding}; nwnrs language features are suspended.`,
      action,
    );
    if (selected !== action) return;

    if (document.isDirty) {
      const confirmed = await vscode.window.showWarningMessage(
        'Reopening with Windows-1252 must discard the unsaved edits in this document.',
        { modal: true },
        'Discard Unsaved Edits',
      );
      if (confirmed !== 'Discard Unsaved Edits') return;
      await vscode.commands.executeCommand('workbench.action.files.revert', document.uri);
      const current = vscode.workspace.textDocuments.find(
        (candidate) => candidate.uri.toString() === key,
      );
      if (current?.isDirty) {
        void vscode.window.showErrorMessage(
          'Could not discard the unsaved NWScript edits; the encoding was not changed.',
        );
        return;
      }
    }

    try {
      await vscode.workspace.openTextDocument(document.uri, {
        encoding: NWNSCRIPT_ENCODING,
      });
    } catch (error) {
      void vscode.window.showErrorMessage(
        `Could not reopen ${path.basename(document.uri.fsPath)} as Windows-1252: ${String(error)}`,
      );
    }
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
      const configuration = this.compilerConfiguration(
        document.uri,
        path.dirname(document.uri.fsPath),
      );
      const response = await this.invokeNative('manifestDiagnostics', {
        path: document.uri.fsPath,
        gitExecutable: configuration.gitExecutable || undefined,
        packageCache: configuration.packageCache || undefined,
      });
      if (this.manifestChecks.get(key) !== sequence) {
        return;
      }
      const diagnostics = response.map((record) => {
        const diagnostic = new vscode.Diagnostic(
          new vscode.Range(
            record.range.start.line,
            record.range.start.character,
            record.range.end.line,
            record.range.end.character,
          ),
          record.message,
          severity(record.severity),
        );
        diagnostic.source = 'nwnrs nwpkg';
        if (record.code !== undefined) diagnostic.code = record.code;
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

  async listPackageDependencies(manifestPath: string): Promise<readonly NativeDependencyStatus[]> {
    return this.invokeNative('listPackageDependencies', this.dependencyRequest(manifestPath));
  }

  async listPackageDependencySources(
    manifestPath: string,
    route: string,
    token?: vscode.CancellationToken,
  ): Promise<readonly NativeDependencySource[]> {
    return this.invokeNative(
      'listPackageDependencySources',
      this.dependencySourceRequest(manifestPath, route),
      token,
    );
  }

  dependencySourceUri(
    manifestPath: string,
    route: string,
    relativePath: string,
  ): vscode.Uri {
    const query = new URLSearchParams({
      manifest: manifestPath,
      route,
      source: relativePath,
    });
    return vscode.Uri.from({
      scheme: DEPENDENCY_SOURCE_SCHEME,
      path: `/${route}/${relativePath}`,
      query: query.toString(),
    });
  }

  async provideDependencySource(
    uri: vscode.Uri,
    token: vscode.CancellationToken,
  ): Promise<string> {
    const query = new URLSearchParams(uri.query);
    const manifestPath = query.get('manifest');
    const route = query.get('route');
    const relativePath = query.get('source');
    if (!manifestPath || !route || !relativePath) {
      throw new Error('Invalid nwnrs dependency source URI.');
    }
    const response = await this.invokeNative('readPackageDependencySource', {
      ...this.dependencySourceRequest(manifestPath, route),
      relativePath,
    }, token);
    return vscode.workspace.decode(response.contents, {
      encoding: response.encoding,
    });
  }

  async fetchPackageDependencies(
    manifestPath?: string,
    dependency?: string,
  ): Promise<readonly NativeDependencyStatus[]> {
    const manifest = await this.resolveDependencyManifest(manifestPath);
    if (!manifest || !(await this.prepareDependencyOperation(manifest, 'Fetch'))) return [];
    const request = this.dependencyRequest(manifest, dependency);
    try {
      const result = await vscode.window.withProgress({
        location: vscode.ProgressLocation.Notification,
        title: dependency
          ? `nwnrs: Fetching ${dependency}`
          : `nwnrs: Fetching dependencies for ${path.basename(path.dirname(manifest))}`,
        cancellable: true,
      }, (progress, token) =>
        this.invokeNative(
          'fetchPackageDependencies',
          request,
          token,
          (event) => {
            progress.report({ message: event.message });
            this.output.appendLine(`[nwpkg:${event.phase}] ${event.message}`);
          },
        ));
      this.output.appendLine(
        `[nwpkg] fetched ${result.length} dependency package${result.length === 1 ? '' : 's'} for ${manifest}`,
      );
      for (const status of result) {
        const commit = status.commit ? ` @ ${shortCommit(status.commit)}` : '';
        this.output.appendLine(
          `  ${status.route}: ${status.sourceKind} ${status.state}${commit}`,
        );
      }
      await this.refreshAfterDependencyChange(manifest);
      return result;
    } catch (error) {
      this.output.appendLine(`[nwpkg] dependency fetch failed: ${String(error)}`);
      void vscode.window.showErrorMessage(`nwnrs dependency fetch failed: ${String(error)}`);
      return [];
    }
  }

  async updatePackageDependencies(
    manifestPath?: string,
    dependency?: string,
  ): Promise<NativeDependencyPlan | undefined> {
    const manifest = await this.resolveDependencyManifest(manifestPath);
    if (!manifest || !(await this.prepareDependencyOperation(manifest, 'Update'))) return undefined;
    const request = this.dependencyRequest(manifest, dependency);
    try {
      const plan = await vscode.window.withProgress({
        location: vscode.ProgressLocation.Notification,
        title: dependency
          ? `nwnrs: Resolving update for ${dependency}`
          : `nwnrs: Resolving dependency updates for ${path.basename(path.dirname(manifest))}`,
        cancellable: true,
      }, (progress, token) =>
        this.invokeNative(
          'planPackageDependencyUpdate',
          request,
          token,
          (event) => {
            progress.report({ message: event.message });
            this.output.appendLine(`[nwpkg:${event.phase}] ${event.message}`);
          },
        ));
      if (!plan.changes.length) {
        void vscode.window.showInformationMessage('nwnrs dependencies are already up to date.');
        return plan;
      }
      const lines = plan.changes.map((change) =>
        `${change.route}: ${shortCommit(change.before)} → ${shortCommit(change.after)}`);
      this.output.appendLine(`[nwpkg] update preview for ${manifest}`);
      for (const line of lines) this.output.appendLine(`  ${line}`);
      const choice = await vscode.window.showInformationMessage(
        `Apply ${plan.changes.length} dependency update${plan.changes.length === 1 ? '' : 's'}?\n\n`
          + lines.slice(0, 8).join('\n')
          + (lines.length > 8 ? `\n…and ${lines.length - 8} more (see nwnrs Compiler output)` : ''),
        { modal: true },
        'Apply Update',
      );
      if (choice !== 'Apply Update') return plan;
      await this.invokeNative('applyPackageDependencyUpdate', {
        manifestPath: manifest,
        token: plan.token,
      });
      this.output.appendLine(`[nwpkg] applied dependency update to ${manifest}`);
      await this.refreshAfterDependencyChange(manifest);
      return plan;
    } catch (error) {
      this.output.appendLine(`[nwpkg] dependency update failed: ${String(error)}`);
      void vscode.window.showErrorMessage(`nwnrs dependency update failed: ${String(error)}`);
      return undefined;
    }
  }

  async cleanPackageDependencyCache(): Promise<void> {
    if (!(await this.ensureTrustedWorkspace())) return;
    try {
      const result = await vscode.window.withProgress({
        location: vscode.ProgressLocation.Notification,
        title: 'nwnrs: Cleaning unused dependency cache entries',
        cancellable: true,
      }, (_progress, token) =>
        this.invokeNative('cleanPackageDependencyCache', this.dependencyRequest(''), token));
      this.output.appendLine(`[nwpkg] removed ${result.removed.length} unused cache entries`);
      void vscode.window.showInformationMessage(
        `nwnrs removed ${result.removed.length} unused dependency cache entr${result.removed.length === 1 ? 'y' : 'ies'}.`,
      );
    } catch (error) {
      this.output.appendLine(`[nwpkg] cache cleanup failed: ${String(error)}`);
      void vscode.window.showErrorMessage(`nwnrs cache cleanup failed: ${String(error)}`);
    }
  }

  provideManifestCodeActions(
    document: vscode.TextDocument,
    actionContext: vscode.CodeActionContext,
  ): vscode.CodeAction[] {
    const actions: vscode.CodeAction[] = [];
    for (const diagnostic of actionContext.diagnostics) {
      if (diagnostic.source !== 'nwnrs nwpkg' || typeof diagnostic.code !== 'string') continue;
      if (diagnostic.code === 'nwpkg.dependency.missing') {
        const action = new vscode.CodeAction(
          document.isDirty ? 'Save and Fetch nwpkg Dependencies' : 'Fetch nwpkg Dependencies',
          vscode.CodeActionKind.QuickFix,
        );
        action.diagnostics = [diagnostic];
        action.isPreferred = true;
        action.command = {
          command: 'nwnrs.nwpkg.fetchDependencies',
          title: action.title,
          arguments: [document.uri.fsPath],
        };
        actions.push(action);
      } else if (diagnostic.code === 'nwpkg.dependency.stale') {
        const action = new vscode.CodeAction(
          document.isDirty ? 'Save and Update nwpkg Dependencies' : 'Update nwpkg Dependencies',
          vscode.CodeActionKind.QuickFix,
        );
        action.diagnostics = [diagnostic];
        action.isPreferred = true;
        action.command = {
          command: 'nwnrs.nwpkg.updateDependencies',
          title: action.title,
          arguments: [document.uri.fsPath],
        };
        actions.push(action);
      }
    }
    return actions;
  }

  private dependencyRequest(manifestPath: string, dependency?: string) {
    const uri = manifestPath ? vscode.Uri.file(manifestPath) : undefined;
    const configuration = vscode.workspace.getConfiguration('nwnrs', uri);
    const gitPath = configuration.get<string>('git.path', '').trim();
    const cachePath = configuration.get<string>('packageCachePath', '').trim();
    return {
      manifestPath,
      gitPath: gitPath || undefined,
      cachePath: cachePath || undefined,
      dependency,
    };
  }

  private dependencySourceRequest(
    manifestPath: string,
    route: string,
  ) {
    const dependency = this.dependencyRequest(manifestPath);
    return {
      manifestPath,
      route,
      relativePath: undefined,
      gitPath: dependency.gitPath,
      cachePath: dependency.cachePath,
    };
  }

  private async resolveDependencyManifest(explicit?: string): Promise<string | undefined> {
    if (explicit) return explicit;
    const document = vscode.window.activeTextEditor?.document;
    if (document && this.isNwpkgDocument(document)) return document.uri.fsPath;
    const context = await this.currentPackageContext();
    if (context) return path.join(context.projectRoot, 'nwpkg.toml');
    void vscode.window.showWarningMessage(
      'Open a file inside an nwnrs package before managing dependencies.',
    );
    return undefined;
  }

  private async prepareDependencyOperation(
    manifestPath: string,
    operation: 'Fetch' | 'Update',
  ): Promise<boolean> {
    if (!(await this.ensureTrustedWorkspace())) return false;
    const document = vscode.workspace.textDocuments.find((candidate) =>
      candidate.uri.scheme === 'file'
      && normalizeFsPath(candidate.uri.fsPath) === normalizeFsPath(manifestPath));
    if (!document?.isDirty) return true;
    const choice = await vscode.window.showWarningMessage(
      `${path.basename(manifestPath)} must be saved before resolving dependencies.`,
      { modal: true },
      `Save and ${operation}`,
    );
    if (choice !== `Save and ${operation}`) return false;
    return document.save();
  }

  private async ensureTrustedWorkspace(): Promise<boolean> {
    if (vscode.workspace.isTrusted) return true;
    const choice = await vscode.window.showWarningMessage(
      'nwnrs requires a trusted workspace before invoking system Git.',
      'Manage Workspace Trust',
    );
    if (choice === 'Manage Workspace Trust') {
      await vscode.commands.executeCommand('workbench.trust.manage');
    }
    return false;
  }

  private async refreshAfterDependencyChange(manifestPath: string): Promise<void> {
    this.languageWorker.invalidate(path.dirname(manifestPath), path.join(
      path.dirname(manifestPath),
      'nwpkg.lock',
    ));
    const document = vscode.workspace.textDocuments.find((candidate) =>
      normalizeFsPath(candidate.uri.fsPath) === normalizeFsPath(manifestPath));
    if (document) {
      await this.checkManifest(document);
    }
  }

  async provideManifestCompletions(
    document: vscode.TextDocument,
    position: vscode.Position,
    token: vscode.CancellationToken,
  ): Promise<vscode.CompletionItem[]> {
    if (token.isCancellationRequested) return [];
    const records = await this.invokeNative('manifestCompletions', {
      path: document.uri.fsPath,
      line: position.line,
      character: position.character,
    }, token);
    return records.map((record) => {
      const item = new vscode.CompletionItem(
        record.label,
        record.kind === 'folder'
          ? vscode.CompletionItemKind.Folder
          : record.kind === 'projectKind'
            ? vscode.CompletionItemKind.EnumMember
            : record.kind === 'section'
              ? vscode.CompletionItemKind.Module
              : vscode.CompletionItemKind.Property,
      );
      item.detail = record.detail;
      item.documentation = new vscode.MarkdownString(record.documentation);
      item.insertText = new vscode.SnippetString(record.insertText);
      item.range = new vscode.Range(
        record.replacement.start.line,
        record.replacement.start.character,
        record.replacement.end.line,
        record.replacement.end.character,
      );
      if (record.retrigger) {
        item.command = {
          command: 'editor.action.triggerSuggest',
          title: 'Continue path completion',
        };
      }
      return item;
    });
  }

  async provideManifestHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    token: vscode.CancellationToken,
  ): Promise<vscode.Hover | undefined> {
    if (token.isCancellationRequested) return undefined;
    const record = await this.invokeNative('manifestHover', {
      path: document.uri.fsPath,
      line: position.line,
      character: position.character,
    }, token);
    if (!record) return undefined;
    return new vscode.Hover(
      new vscode.MarkdownString(record.documentation),
      new vscode.Range(
        record.range.start.line,
        record.range.start.character,
        record.range.end.line,
        record.range.end.character,
      ),
    );
  }

  async provideManifestDefinition(
    document: vscode.TextDocument,
    position: vscode.Position,
    token: vscode.CancellationToken,
  ): Promise<vscode.Location | undefined> {
    if (token.isCancellationRequested) return undefined;
    const record = await this.invokeNative('manifestDefinition', {
      path: document.uri.fsPath,
      line: position.line,
      character: position.character,
    }, token);
    return record
      ? new vscode.Location(
        vscode.Uri.file(record.path),
        new vscode.Range(
          record.range.start.line,
          record.range.start.character,
          record.range.end.line,
          record.range.end.character,
        ),
      )
      : undefined;
  }

  async provideManifestSymbols(
    document: vscode.TextDocument,
    token: vscode.CancellationToken,
  ): Promise<vscode.DocumentSymbol[]> {
    if (token.isCancellationRequested) return [];
    const records = await this.invokeNative('manifestSymbols', {
      path: document.uri.fsPath,
    }, token);
    const convert = (record: (typeof records)[number]): vscode.DocumentSymbol => {
      const symbol = new vscode.DocumentSymbol(
        record.name,
        record.detail,
        record.kind === 'section' ? vscode.SymbolKind.Namespace : vscode.SymbolKind.Property,
        new vscode.Range(
          record.range.start.line,
          record.range.start.character,
          record.range.end.line,
          record.range.end.character,
        ),
        new vscode.Range(
          record.selection.start.line,
          record.selection.start.character,
          record.selection.end.line,
          record.selection.end.character,
        ),
      );
      symbol.children = record.children.map(convert);
      return symbol;
    };
    return records.map(convert);
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
      vscode.workspace.createFileSystemWatcher('**/nwpkg.lock'),
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
    if (path.basename(uri.fsPath) === 'nwpkg.toml'
        || path.basename(uri.fsPath) === 'nwpkg.lock') {
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
      const resolved = await this.invokeNative('resolveWatchRoots', { roots: [...inputs] });
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
    const projectRoot = await this.resolveProjectRoot(filePath);
    await this.runCompiler({
      key: this.documentKey(document),
      targets: [filePath],
      cwd: projectRoot,
      sourceUri: document.uri,
      recurse: false,
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
    const packages = await this.invokeNative('listWorkspacePackages', undefined);
    const candidates = packages.length > 0
      ? packages.map((pkg) => ({
        root: pkg.root,
        sourceUri: vscode.workspace.getWorkspaceFolder(vscode.Uri.file(pkg.root))?.uri
          ?? vscode.Uri.file(pkg.root),
      }))
      : folders.map((folder) => ({ root: folder.uri.fsPath, sourceUri: folder.uri }));
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
          revealFailure: true,
          force: true,
        }, token);
        progress.report({ increment, message: path.basename(root) });
      }));
    });
  }

  async listWorkspacePackageManifests(): Promise<readonly string[]> {
    const packages = await this.invokeNative('listWorkspacePackages', undefined);
    return packages.map((entry) => entry.manifestPath);
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
    const gitExecutable = config.get<string>('git.path', '').trim();
    const packageCache = resolveConfiguredPath(config.get('packageCachePath', ''), context, cwd);
    return {
      includeDirectories,
      langspecPath,
      rootPath,
      userPath,
      language: config.get('language', 'english'),
      loadOvr: config.get('loadOvr', false),
      gitExecutable,
      packageCache,
      maxIncludeDepth: config.get('maxIncludeDepth', 16),
      maxDiagnosticsPerFile: config.get('maxDiagnosticsPerFile', 50),
      noEntrypointCheck: config.get('noEntrypointCheck', true),
    };
  }

  async invokeNative<K extends WorkerLanguageMethod>(
    method: K,
    request: NativeLanguageRequest<K>,
    cancellationToken?: vscode.CancellationToken,
    onProgress?: (event: WorkerProgressEvent) => void,
  ): Promise<NativeLanguageResponse<K>> {
    return this.languageWorker.requestTyped(
      method,
      request,
      cancellationToken,
      {
        sessionKey: this.languageSessionKey(request),
        ...(onProgress === undefined ? {} : { onProgress }),
      },
    );
  }

  async resolveProjectRoot(
    sourcePath: string,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<string> {
    return this.invokeNative('resolvePackageRoot', sourcePath, cancellationToken);
  }

  languageSessionKey(request: unknown): string {
    const requestPath = recordString(request, 'path');
    if (requestPath && path.basename(requestPath) === 'nwpkg.toml') {
      return path.dirname(path.resolve(requestPath));
    }
    const manifestPath = recordString(request, 'manifestPath');
    if (manifestPath) return path.dirname(path.resolve(manifestPath));
    const packageRequest = typeof request === 'object' && request !== null && 'package' in request
      ? (request as { readonly package?: unknown }).package
      : request;
    const projectRoot = recordString(packageRequest, 'projectRoot');
    if (projectRoot) {
      return path.resolve(projectRoot);
    }
    const sourcePath = recordString(packageRequest, 'sourcePath');
    if (sourcePath) {
      return path.resolve(sourcePath);
    }
    const paths = recordStringArray(request, 'paths');
    if (paths?.length === 1 && paths[0]) {
      return path.resolve(paths[0]);
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
      );
    } else {
      const sourcePath = document.uri.fsPath;
      const origin = this.physicalDocumentRequests.get(path.resolve(sourcePath))?.[0];
      if (origin) {
        request = definitionRequestFromSourceContext(
          { ...origin, sourcePath },
          symbol,
          qualifier,
        );
      } else {
        const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
        const configuration = this.compilerConfiguration(document.uri, projectRoot);
        request = buildDefinitionRequest(sourcePath, symbol, {
          projectRoot,
          qualifier,
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
    const reusableRequest = request;
    for (const definition of definitions) {
      if (!definition.uri && path.isAbsolute(definition.path)) {
        this.rememberPhysicalDocument(definition.path, reusableRequest);
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
    const contextKey = sourceContext.projectRoot
      ? path.resolve(sourceContext.projectRoot)
      : key;
    const existing = contexts.findIndex((context) =>
      (context.projectRoot ? path.resolve(context.projectRoot) : key) === contextKey);
    if (existing >= 0) {
      contexts.splice(existing, 1);
    }
    contexts.push(sourceContext);
    contexts.sort((left, right) => {
      const score = (context: NativeDocumentSymbolsRequest): number =>
        context.projectRoot ? 1 : 0;
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
    if (!request.resource) {
      throw new Error('The nwnrs game-source context has no logical resource identity.');
    }
    const response = await this.invokeNative('readVirtualSource', {
      package: request,
      resource: request.resource,
    }, cancellationToken);
    if (!response || response.uri !== key || !(response.contents instanceof Uint8Array)) {
      throw new Error('The compiler returned a different game script than requested.');
    }
    const contents = await vscode.workspace.decode(response.contents, {
      encoding: NWNSCRIPT_ENCODING,
    });
    this.virtualDocuments.set(key, contents);
    return contents;
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
      };
    } else {
      const sourcePath = document.uri.fsPath;
      const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
      const configuration = this.compilerConfiguration(document.uri, projectRoot);
      request = buildDocumentSymbolsRequest(sourcePath, {
        projectRoot,
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
    if (document.uri.scheme !== 'file') return [];
    const diagnostics = actionContext.diagnostics
      .filter((diagnostic) =>
        diagnostic.source === DIAGNOSTIC_OWNER && typeof diagnostic.code === 'number')
      .map((diagnostic) => ({
        code: diagnostic.code as number,
        range: {
          start: {
            line: diagnostic.range.start.line,
            character: diagnostic.range.start.character,
          },
          end: {
            line: diagnostic.range.end.line,
            character: diagnostic.range.end.character,
          },
        },
      }));
    if (diagnostics.length === 0) return [];
    try {
      const sourcePath = document.uri.fsPath;
      const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
      const configuration = this.compilerConfiguration(document.uri, projectRoot);
      const records = await this.invokeNative('codeActionsAt', {
        package: buildDocumentSymbolsRequest(sourcePath, {
          projectRoot,
          ...configuration,
        }),
        diagnostics,
      }, cancellationToken);
      return records.map((record) => {
        const action = new vscode.CodeAction(record.title, vscode.CodeActionKind.QuickFix);
        action.diagnostics = [...actionContext.diagnostics];
        action.isPreferred = record.isPreferred;
        action.edit = new vscode.WorkspaceEdit();
        for (const edit of record.edits) {
          action.edit.replace(
            vscode.Uri.file(edit.path),
            nativeSourceRange(edit.range),
            edit.newText,
          );
        }
        return action;
      });
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs code-action failure: ${String(error)}`);
      }
      return [];
    }
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
      };
    } else {
      const sourcePath = document.uri.fsPath;
      const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
      const configuration = this.compilerConfiguration(document.uri, projectRoot);
      request = buildDocumentSymbolsRequest(sourcePath, {
        projectRoot,
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
        const range = diagnosticRange(record.range);
        if (range.startLine !== range.endLine) {
          continue;
        }
        const length = Math.max(1, range.endColumn - range.startColumn);
        const tokenType = SEMANTIC_TOKEN_TYPES.indexOf(record.kind);
        if (tokenType < 0) {
          continue;
        }
        let modifiers = 0;
        if (record.isDeclaration) {
          modifiers |= 1 << SEMANTIC_TOKEN_MODIFIERS.indexOf('declaration');
        }
        if (record.isReadonly) {
          modifiers |= 1 << SEMANTIC_TOKEN_MODIFIERS.indexOf('readonly');
        }
        if (record.isDefaultLibrary) {
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
            new vscode.Position(record.position.line - 1, record.position.character - 1),
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
    const eventDefinition = await this.provideEventDefinition(
      document,
      position,
      cancellationToken,
    );
    if (eventDefinition) {
      return eventDefinition;
    }
    const included = await this.resolveIncludedSource(document, position, cancellationToken);
    if (included) {
      return new vscode.Location(
        included.uri ? vscode.Uri.parse(included.uri) : vscode.Uri.file(included.path),
        new vscode.Position(0, 0),
      );
    }
    try {
      const references = await this.resolveReferences(document, position, cancellationToken);
      const declarations = references?.records.filter((record) => record.isDeclaration) || [];
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
    const implementations = definitions.filter((definition) => definition.isImplementation);
    const preferred = implementations.length > 0 ? implementations : definitions;
    return preferred.map(definitionLocation);
  }

  async provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.Hover | undefined> {
    const eventHover = await this.provideEventHover(document, position, cancellationToken);
    if (eventHover) {
      return eventHover;
    }
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
      const declarations = references?.records.filter((record) => record.isDeclaration) || [];
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

  async eventCatalog(
    cancellationToken?: vscode.CancellationToken,
  ): Promise<readonly NativeEventCatalogEntry[]> {
    if (!this.eventCatalogPromise) {
      this.eventCatalogPromise = this.invokeNative(
        'listEventCatalog',
        undefined,
        cancellationToken,
      ).catch((error: unknown) => {
        this.eventCatalogPromise = undefined;
        throw error;
      });
    }
    return this.eventCatalogPromise;
  }

  async provideEventHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.Hover | undefined> {
    const match = await this.resolveEventSymbol(document, position, cancellationToken);
    if (!match) return undefined;
    if (match.kind === 'handler' && match.definition) {
      return hoverForDefinition(match.definition, nativeSourceRange(match.range));
    }
    const event = (await this.eventCatalog(cancellationToken))
      .find((candidate) =>
        candidate.subject === match.subject && candidate.phase === match.phase);
    if (!event) return undefined;
    const signature = new vscode.MarkdownString();
    signature.appendCodeblock(
      `#[nwnrs::event::${event.subject}::${event.phase}]`,
      'nwscript',
    );
    const documentation = new vscode.MarkdownString(eventHoverMarkdown(event));
    documentation.isTrusted = false;
    return new vscode.Hover(
      [signature, documentation],
      nativeSourceRange(match.range),
    );
  }

  async provideEventDefinition(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.Location | undefined> {
    const match = await this.resolveEventSymbol(document, position, cancellationToken);
    if (!match) return undefined;
    if (match.kind === 'handler' && match.definition) {
      return definitionLocation(match.definition);
    }
    const catalog = await this.eventCatalog(cancellationToken);
    if (!catalog.some((event) =>
      event.subject === match.subject && event.phase === match.phase)) return undefined;
    const generated = buildEventCatalogDocument(catalog);
    this.eventCatalogDocument = generated;
    const line = generated.routeLines.get(eventRouteKey(match.subject, match.phase));
    if (line === undefined) return undefined;
    const startColumn = match.token === 'subject' ? 3 : 3 + match.subject.length + 2;
    const tokenLength = match.token === 'subject' ? match.subject.length : match.phase.length;
    return new vscode.Location(
      EVENT_CATALOG_URI,
      new vscode.Range(line, startColumn, line, startColumn + tokenLength),
    );
  }

  async provideEventCompletions(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.CompletionItem[] | undefined> {
    const request = await this.eventPositionRequest(document, position, cancellationToken);
    if (!request) return undefined;
    const context = await this.invokeNative('eventCompletionsAt', request, cancellationToken);
    if (!context) return undefined;
    const replacement = new vscode.Range(
      document.positionAt(context.replacementStart),
      document.positionAt(context.replacementEnd),
    );
    return context.items.map((record) => {
      const item = new vscode.CompletionItem(
        record.label,
        eventCompletionItemKind(record.kind),
      );
      item.range = replacement;
      item.detail = record.detail;
      item.sortText = record.sortText;
      if (record.documentation) {
        item.documentation = new vscode.MarkdownString(record.documentation);
      }
      if (context.kind === 'modifier') {
        item.insertText = new vscode.SnippetString(`${record.label}(\${1})`);
      }
      return item;
    });
  }

  async provideNwScriptCompletions(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.CompletionItem[] | undefined> {
    try {
      const eventItems = await this.provideEventCompletions(
        document,
        position,
        cancellationToken,
      );
      if (eventItems) return eventItems;
      const request = await this.nwscriptPositionRequest(document, position, cancellationToken);
      if (!request) return undefined;
      const records = await this.invokeNative('completeNwscript', request, cancellationToken);
      return records.map((record) => {
        const item = new vscode.CompletionItem(
          record.label,
          nwscriptCompletionItemKind(record.kind),
        );
        item.detail = record.detail;
        item.insertText = record.insertText;
        item.sortText = record.sortText;
        item.range = nativeSourceRange(record.replacement);
        if (record.documentation) {
          item.documentation = new vscode.MarkdownString(
            formatHoverDocumentation(record.documentation),
          );
        }
        return item;
      });
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs completion failure: ${String(error)}`);
      }
      return undefined;
    }
  }

  async provideSignatureHelp(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.SignatureHelp | undefined> {
    const request = await this.nwscriptPositionRequest(document, position, cancellationToken);
    if (!request) return undefined;
    try {
      const record = await this.invokeNative('signatureHelp', request, cancellationToken);
      if (!record) return undefined;
      const result = new vscode.SignatureHelp();
      result.activeSignature = record.activeSignature;
      result.activeParameter = record.activeParameter;
      result.signatures = record.signatures.map((signature) => {
        const information = new vscode.SignatureInformation(
          signature.label,
          signature.documentation
            ? new vscode.MarkdownString(formatHoverDocumentation(signature.documentation))
            : undefined,
        );
        information.parameters = signature.parameters.map((parameter) =>
          new vscode.ParameterInformation(
            parameter.label,
            parameter.documentation
              ? new vscode.MarkdownString(parameter.documentation)
              : undefined,
          ));
        return information;
      });
      return result;
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs signature-help failure: ${String(error)}`);
      }
      return undefined;
    }
  }

  async nwscriptPositionRequest(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<NativePositionRequest | undefined> {
    let packageConfiguration;
    if (document.uri.scheme === 'nwnrs-game') {
      const context = this.virtualDocumentRequests.get(document.uri.toString());
      if (!context) return undefined;
      packageConfiguration = context;
    } else if (document.uri.scheme === 'file') {
      const sourcePath = document.uri.fsPath;
      const origin = this.physicalDocumentRequests.get(path.resolve(sourcePath))?.[0];
      if (origin) {
        packageConfiguration = { ...origin, sourcePath };
      } else {
        const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
        packageConfiguration = buildDocumentSymbolsRequest(sourcePath, {
          projectRoot,
          ...this.compilerConfiguration(document.uri, projectRoot),
        });
      }
    } else {
      return undefined;
    }
    return {
      package: {
        ...packageConfiguration,
        includeDirectories: [...packageConfiguration.includeDirectories],
      },
      line: position.line,
      column: position.character,
    };
  }

  async eventPositionRequest(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken?: vscode.CancellationToken,
  ): Promise<NativeEventPositionRequest | undefined> {
    if (document.uri.scheme !== 'file') return undefined;
    const sourcePath = document.uri.fsPath;
    const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
    const configuration = this.compilerConfiguration(document.uri, projectRoot);
    return {
      package: buildDocumentSymbolsRequest(sourcePath, {
        projectRoot,
        ...configuration,
      }),
      offset: document.offsetAt(position),
    };
  }

  async resolveEventSymbol(
    document: vscode.TextDocument,
    position: vscode.Position,
    cancellationToken: vscode.CancellationToken,
  ): Promise<NativeEventSymbol | undefined> {
    const request = await this.eventPositionRequest(document, position, cancellationToken);
    if (!request) return undefined;
    try {
      return await this.invokeNative('resolveEventSymbolAt', request, cancellationToken)
        || undefined;
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs event-symbol failure: ${String(error)}`);
      }
      return undefined;
    }
  }

  async provideEventCatalogDocument(
    cancellationToken: vscode.CancellationToken,
  ): Promise<string> {
    if (!this.eventCatalogDocument) {
      this.eventCatalogDocument = buildEventCatalogDocument(
        await this.eventCatalog(cancellationToken),
      );
    }
    return this.eventCatalogDocument.contents;
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
      request = definitionRequestFromSourceContext(context, '', null);
    } else {
      const sourcePath = document.uri.fsPath;
      const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
      const configuration = this.compilerConfiguration(document.uri, projectRoot);
      request = buildDefinitionRequest(sourcePath, '', {
        projectRoot,
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
    return {
      path: source.path,
      uri: source.uri ?? null,
      resource: source.resource ?? null,
      range,
      includeName,
    };
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
      requests = [definitionRequestFromSourceContext(context, symbol, qualifier)];
    } else {
      const sourcePath = document.uri.fsPath;
      const origins = this.physicalDocumentRequests.get(path.resolve(sourcePath));
      if (origins?.length) {
        requests = origins.map((origin) => definitionRequestFromSourceContext(
          { ...origin, sourcePath },
          symbol,
          qualifier,
        ));
      } else {
        const projectRoot = await this.resolveProjectRoot(sourcePath, cancellationToken);
        const configuration = this.compilerConfiguration(document.uri, projectRoot);
        requests = [buildDefinitionRequest(sourcePath, symbol, {
          projectRoot,
          qualifier,
          ...configuration,
        })];
      }
    }
    const responses = await Promise.all(requests.map((request) => this.invokeNative(
      'findReferences',
      {
        package: request.package,
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
        const key = `${record.uri || path.resolve(record.path)}:`
          + `${record.range.start.line}:${record.range.start.character}`;
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
      const key = `${record.uri || path.resolve(record.path)}:`
        + `${record.range.start.line}:${record.range.start.character}`;
      const recordRequest = recordContexts.get(key) || request;
      if (record.uri && record.resource) {
        this.virtualDocumentRequests.set(
          record.uri,
          sourceContextFromRequest(recordRequest, record.path, record.resource),
        );
      }
      if (!record.uri && path.isAbsolute(record.path)) {
        this.rememberPhysicalDocument(record.path, recordRequest);
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
        .filter((record) => referenceContext.includeDeclaration || !record.isDeclaration)
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
    const request = await this.nwscriptPositionRequest(document, position, cancellationToken);
    if (!request) throw new Error('The symbol at this position cannot be renamed.');
    const plan = await this.invokeNative('renameAt', {
      ...request,
      newName: undefined,
    }, cancellationToken);
    return { range: nativeSourceRange(plan.range), placeholder: plan.name };
  }

  async provideRenameEdits(
    document: vscode.TextDocument,
    position: vscode.Position,
    newName: string,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.WorkspaceEdit> {
    const request = await this.nwscriptPositionRequest(document, position, cancellationToken);
    if (!request) throw new Error('The symbol at this position cannot be renamed.');
    const plan = await this.invokeNative('renameAt', {
      ...request,
      newName,
    }, cancellationToken);
    const edit = new vscode.WorkspaceEdit();
    for (const record of plan.edits) {
      const location = referenceLocation(record);
      edit.replace(location.uri, location.range, newName);
    }
    return edit;
  }

  async provideWorkspaceSymbols(
    search: string,
    cancellationToken: vscode.CancellationToken,
  ): Promise<vscode.SymbolInformation[]> {
    const results: vscode.SymbolInformation[] = [];
    const representative = vscode.workspace.workspaceFolders?.[0]?.uri;
    if (!representative) return results;
    const configuration = this.compilerConfiguration(representative, representative.fsPath);
    const request = buildDocumentSymbolsRequest(representative.fsPath, {
      projectRoot: representative.fsPath,
      ...configuration,
    });
    try {
      const symbols = await this.invokeNative('listWorkspaceSymbols', request, cancellationToken);
      for (const entry of symbols) {
        appendWorkspaceSymbols(
          results,
          [entry.symbol],
          vscode.Uri.file(entry.sourcePath),
          search,
          '',
        );
      }
    } catch (error) {
      if (!cancellationToken.isCancellationRequested) {
        this.output.appendLine(`nwnrs workspace-symbol warning: ${String(error)}`);
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
      package: {
        ...resolved.request.package,
        sourcePath: definition.path,
      },
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
      package: request.package,
      line: item.selectionRange.start.line + 1,
      column: item.selectionRange.start.character + 1,
    }, cancellationToken);
    const groups = new Map();
    for (const reference of references || []) {
      if (reference.isDeclaration || !reference.container || reference.uri) {
        continue;
      }
      const key = `${reference.path}:${reference.container}`;
      const group = groups.get(key) || { reference, ranges: [] };
      group.ranges.push(referenceLocation(reference).range);
      groups.set(key, group);
    }
    const incoming = [];
    for (const group of groups.values()) {
      const callerRoot = request.package.projectRoot
        || await this.resolveProjectRoot(group.reference.path, cancellationToken);
      const callerRequest: NativeDefinitionRequest = {
        package: {
          ...request.package,
          sourcePath: group.reference.path,
          projectRoot: callerRoot,
        },
        symbol: group.reference.container,
        qualifier: undefined,
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
      package: request.package,
      line: item.selectionRange.start.line + 1,
      column: item.selectionRange.start.character + 1,
    }, cancellationToken);
    return (calls || []).map((call) => {
      const targetRoot = request.package.projectRoot;
      const targetRequest: NativeDefinitionRequest = {
        ...request,
        package: {
          ...request.package,
          sourcePath: call.target.path,
          projectRoot: targetRoot,
        },
        symbol: call.target.name,
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
    records: readonly NativeLanguageDiagnostic[],
    request: CompilerRunRequest,
  ): Promise<Map<string, DiagnosticRunEntry>> {
    const mapped = new Map<string, DiagnosticRunEntry>();
    for (const record of records) {
      const uri = this.resolveDiagnosticUri(record, request);
      const range = diagnosticRange(record.range);
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
      if (record.code !== undefined) {
        diagnostic.code = record.code;
      }
      const key = uri.toString();
      const entry = mapped.get(key) || { uri, diagnostics: [] };
      entry.diagnostics.push(diagnostic);
      mapped.set(key, entry);
    }
    return mapped;
  }

  resolveDiagnosticUri(
    record: NativeLanguageDiagnostic,
    request: CompilerRunRequest,
  ): vscode.Uri {
    if (record.path) return vscode.Uri.file(record.path);
    if (record.uri) return vscode.Uri.parse(record.uri);
    return request.sourceUri;
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
  sourcePath: string = 'package' in request ? request.package.sourcePath : request.sourcePath,
  resource: string | null = 'resource' in request ? request.resource ?? null : null,
): NativeDocumentSymbolsRequest {
  const packageConfiguration = 'package' in request ? request.package : request;
  return {
    ...packageConfiguration,
    sourcePath: resource || sourcePath,
    resource: resource || undefined,
    includeDirectories: [...packageConfiguration.includeDirectories],
  };
}

function definitionRequestFromSourceContext(
  context: NativeDocumentSymbolsRequest,
  symbol: string,
  qualifier: string | null,
): NativeDefinitionRequest {
  return {
    package: {
      ...context,
      includeDirectories: [...context.includeDirectories],
    },
    symbol,
    qualifier: qualifier || undefined,
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

function documentSymbol(record: NativeDocumentSymbol): vscode.DocumentSymbol {
  const full = nativeSourceRange(record.range);
  const selection = nativeSourceRange(record.selection);
  const symbol = new vscode.DocumentSymbol(
    String(record.name || ''),
    typeof record.detail === 'string' ? record.detail : '',
    documentSymbolKind(record.kind),
    full,
    selection,
  );
  symbol.children = record.children.map((child) => documentSymbol(child));
  return symbol;
}

function referenceLocation(record: NativeReference): vscode.Location {
  return new vscode.Location(
    record.uri ? vscode.Uri.parse(record.uri) : vscode.Uri.file(record.path),
    nativeSourceRange(record.range),
  );
}

function nativeSourceRange(record: NativeRange): vscode.Range {
  return new vscode.Range(
    Math.max(0, record.start.line),
    Math.max(0, record.start.character),
    Math.max(0, record.end.line),
    Math.max(0, record.end.character),
  );
}

function definitionLocation(definition: NativeDefinition): vscode.Location {
  return new vscode.Location(
    definition.uri ? vscode.Uri.parse(definition.uri) : vscode.Uri.file(definition.path),
    nativeSourceRange(definition.range),
  );
}

function hoverForDefinition(
  definition: NativeDefinition,
  range: vscode.Range,
): vscode.Hover | undefined {
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
  return contents.length > 0 ? new vscode.Hover(contents, range) : undefined;
}

function eventCompletionItemKind(
  kind: string,
): vscode.CompletionItemKind {
  switch (kind) {
    case 'event':
      return vscode.CompletionItemKind.Event;
    case 'enumMember':
      return vscode.CompletionItemKind.EnumMember;
    case 'function':
      return vscode.CompletionItemKind.Function;
    default:
      return vscode.CompletionItemKind.Keyword;
  }
}

function nwscriptCompletionItemKind(kind: string): vscode.CompletionItemKind {
  switch (kind) {
    case 'function':
      return vscode.CompletionItemKind.Function;
    case 'variable':
      return vscode.CompletionItemKind.Variable;
    case 'constant':
      return vscode.CompletionItemKind.Constant;
    case 'parameter':
      return vscode.CompletionItemKind.Variable;
    case 'field':
      return vscode.CompletionItemKind.Field;
    case 'type':
      return vscode.CompletionItemKind.Struct;
    case 'enum':
      return vscode.CompletionItemKind.Enum;
    case 'enumMember':
      return vscode.CompletionItemKind.EnumMember;
    case 'macro':
      return vscode.CompletionItemKind.Snippet;
    default:
      return vscode.CompletionItemKind.Text;
  }
}

function definitionMatchesReference(
  definition: NativeDefinition,
  reference: NativeReference,
): boolean {
  const sameSource = definition.uri && reference.uri
    ? definition.uri === reference.uri
    : path.resolve(definition.path) === path.resolve(reference.path);
  return sameSource
    && definition.range.start.line === reference.range.start.line
    && definition.range.start.character === reference.range.start.character;
}

function callHierarchyItem(
  definition: NativeDefinition,
  request: NativeDefinitionRequest,
): NwnrsCallHierarchyItem {
  const selection = nativeSourceRange(definition.range);
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
    const range = nativeSourceRange(record.selection);
    if (!normalizedSearch || String(record.name).toLocaleLowerCase().includes(normalizedSearch)) {
      results.push(new vscode.SymbolInformation(
        record.name,
        documentSymbolKind(record.kind),
        container,
        new vscode.Location(
          uri,
          range,
        ),
      ));
    }
    appendWorkspaceSymbols(results, record.children, uri, search, record.name);
  }
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

function unrepresentableEncodingDiagnostics(
  document: vscode.TextDocument,
  source: string,
  decoded: string,
): vscode.Diagnostic[] {
  return unrepresentableUtf16Ranges(source, decoded).map(({ start, end }) => {
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(document.positionAt(start), document.positionAt(end)),
      'This text cannot be represented exactly in Windows-1252. '
      + 'NWScript analysis is suspended until it is replaced.',
      vscode.DiagnosticSeverity.Error,
    );
    diagnostic.source = 'nwnrs encoding';
    diagnostic.code = 'nwnrs.encoding.unrepresentable';
    return diagnostic;
  });
}

function normalizeFsPath(value: string): string {
  return path.resolve(value);
}

function shortCommit(value: string | undefined): string {
  if (!value) return '<unlocked>';
  return value.length > 12 ? value.slice(0, 12) : value;
}

export function activate(context: vscode.ExtensionContext): void {
  const controller = new CompilerController(context);
  controller.register();
  const resourceEditors = new ResourceCustomEditorProvider(
    context,
    controller.output,
    (sourcePath) => controller.resolveProjectRoot(sourcePath),
  );
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
