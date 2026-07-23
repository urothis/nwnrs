import * as fs from 'node:fs';
import * as path from 'node:path';
import type { NativeDefinition } from './native-types';

export const PROJECT_MANIFEST = 'nwpkg.toml';

export interface PathVariableContext {
  readonly workspaceFolder?: string;
  readonly projectRoot?: string;
  readonly fileDirname?: string;
}

export interface SourceOverlay {
  readonly path: string;
  readonly contents: string;
}

export interface CompilerRequestOptions {
  readonly noEntrypointCheck?: boolean;
  readonly langspecPath?: string | null;
  readonly includeDirectories?: readonly string[];
  readonly overlays?: readonly SourceOverlay[];
  readonly maxIncludeDepth?: number;
  readonly maxDiagnosticsPerFile?: number;
  readonly recurse?: boolean;
  readonly rootPath?: string | null;
  readonly userPath?: string | null;
  readonly language?: string;
  readonly loadOvr?: boolean;
  readonly qualifier?: string | null;
  readonly projectRoot?: string | null;
  readonly resource?: string | null;
}

export interface NativeCheckRequest {
  readonly paths: string[];
  readonly no_entrypoint_check: boolean;
  readonly langspec: string | null;
  readonly include_dirs: string[];
  readonly overlays: SourceOverlay[];
  readonly max_include_depth: number;
  readonly max_diagnostics_per_input: number;
  readonly recurse: boolean;
  readonly root: string | null;
  readonly user: string | null;
  readonly language: string;
  readonly load_ovr: boolean;
}

export interface NativeDefinitionRequest {
  readonly source_path: string;
  readonly symbol: string;
  readonly qualifier: string | null;
  readonly project_root: string | null;
  readonly include_dirs: string[];
  readonly overlays: SourceOverlay[];
  readonly langspec: string | null;
  readonly max_include_depth: number;
  readonly root: string | null;
  readonly user: string | null;
  readonly language: string;
  readonly load_ovr: boolean;
}

export interface NativeDocumentSymbolsRequest {
  readonly source_path: string;
  readonly resource: string | null;
  readonly project_root: string | null;
  readonly include_dirs: string[];
  readonly overlays: SourceOverlay[];
  readonly langspec: string | null;
  readonly max_include_depth: number;
  readonly root: string | null;
  readonly user: string | null;
  readonly language: string;
  readonly load_ovr: boolean;
}

export interface NativeDiagnosticRecord {
  readonly start_line?: number | null;
  readonly start_column?: number | null;
  readonly end_line?: number | null;
  readonly end_column?: number | null;
}

interface DocumentationEntry {
  description: string;
}

interface DocumentationParameter extends DocumentationEntry {
  name: string;
}

export function isNssPath(filePath: string): boolean {
  return path.extname(filePath).toLowerCase() === '.nss';
}

export function findProjectRoot(filePath: string): string {
  const resolved = path.resolve(filePath);
  let current;
  try {
    current = fs.statSync(resolved).isDirectory() ? resolved : path.dirname(resolved);
  } catch {
    current = path.extname(resolved) ? path.dirname(resolved) : resolved;
  }
  const fallback = current;
  for (;;) {
    if (fs.existsSync(path.join(current, PROJECT_MANIFEST))) {
      return current;
    }
    const parent = path.dirname(current);
    if (parent === current) {
      return fallback;
    }
    current = parent;
  }
}

export function expandPathVariables(value: string | undefined, context: PathVariableContext): string {
  if (!value) {
    return '';
  }
  return value
    .replaceAll('${workspaceFolder}', context.workspaceFolder || '')
    .replaceAll('${projectRoot}', context.projectRoot || '')
    .replaceAll('${fileDirname}', context.fileDirname || '');
}

export function resolveConfiguredPath(
  value: string | undefined,
  context: PathVariableContext,
  baseDirectory: string,
): string {
  const expanded = expandPathVariables(value, context);
  if (!expanded || path.isAbsolute(expanded)) {
    return expanded;
  }
  return path.resolve(baseDirectory, expanded);
}

export function buildCheckRequest(
  targets: readonly string[],
  options: CompilerRequestOptions = {},
): NativeCheckRequest {
  return {
    paths: [...targets],
    no_entrypoint_check: options.noEntrypointCheck !== false,
    langspec: options.langspecPath || null,
    include_dirs: [...(options.includeDirectories || [])],
    overlays: [...(options.overlays || [])],
    max_include_depth: options.maxIncludeDepth || 16,
    max_diagnostics_per_input: options.maxDiagnosticsPerFile || 50,
    recurse: options.recurse === true,
    root: options.rootPath || null,
    user: options.userPath || null,
    language: options.language || 'english',
    load_ovr: options.loadOvr === true,
  };
}

export function buildDefinitionRequest(
  sourcePath: string,
  symbol: string,
  options: CompilerRequestOptions = {},
): NativeDefinitionRequest {
  return {
    source_path: sourcePath,
    symbol,
    qualifier: options.qualifier || null,
    project_root: options.projectRoot || null,
    include_dirs: [...(options.includeDirectories || [])],
    overlays: [...(options.overlays || [])],
    langspec: options.langspecPath || null,
    max_include_depth: options.maxIncludeDepth || 16,
    root: options.rootPath || null,
    user: options.userPath || null,
    language: options.language || 'english',
    load_ovr: options.loadOvr === true,
  };
}

export function buildDocumentSymbolsRequest(
  sourcePath: string,
  options: CompilerRequestOptions = {},
): NativeDocumentSymbolsRequest {
  return {
    source_path: sourcePath,
    resource: options.resource || null,
    project_root: options.projectRoot || null,
    include_dirs: [...(options.includeDirectories || [])],
    overlays: [...(options.overlays || [])],
    langspec: options.langspecPath || null,
    max_include_depth: options.maxIncludeDepth || 16,
    root: options.rootPath || null,
    user: options.userPath || null,
    language: options.language || 'english',
    load_ovr: options.loadOvr === true,
  };
}

export function selectHoverDefinition(
  definitions: readonly NativeDefinition[] | unknown,
): NativeDefinition | undefined {
  if (!Array.isArray(definitions) || definitions.length === 0) {
    return undefined;
  }
  return definitions.find((definition) =>
    definition.is_implementation && definition.documentation)
    || definitions.find((definition) => definition.documentation)
    || definitions.find((definition) => definition.is_implementation)
    || definitions[0];
}

export function formatHoverDocumentation(documentation: string | null | undefined): string {
  if (!documentation) {
    return '';
  }
  const description: string[] = [];
  const parameters: DocumentationParameter[] = [];
  const returns: DocumentationEntry[] = [];
  const notes: DocumentationEntry[] = [];
  let privateApi = false;
  let continuation: DocumentationEntry | undefined;

  for (const sourceLine of documentation.split(/\r?\n/u)) {
    const line = sourceLine.trim();
    const parameter = line.match(/^@param\s+(\S+)\s*(.*)$/u);
    const returnValue = line.match(/^@returns?\s*(.*)$/u);
    const vanillaParameter = line.match(/^-\s+([a-z][A-Za-z0-9_]*)\s*(?::|-|\s)\s*(.*)$/u);
    const vanillaReturn = line.match(/^\*\s*((?:No\s+)?[Rr]eturn(?:s| value)?(?:\s+value)?[^:]*(?::\s*)?.*)$/u)
      || line.match(/^([Rr]eturn(?:s| value)?(?:\s+value)?[^:]*(?::\s*)?.*)$/u);
    const note = line.match(/^(?:Notes?|NB)\s*:?\s*(.*)$/iu);
    if (parameter) {
      const entry = { name: parameter[1] ?? '', description: parameter[2] ?? '' };
      parameters.push(entry);
      continuation = entry;
    } else if (vanillaParameter) {
      const entry = {
        name: vanillaParameter[1] ?? '',
        description: vanillaParameter[2] ?? '',
      };
      parameters.push(entry);
      continuation = entry;
    } else if (returnValue) {
      const entry = { description: returnValue[1] ?? '' };
      returns.push(entry);
      continuation = entry;
    } else if (vanillaReturn) {
      const entry = { description: vanillaReturn[1] ?? '' };
      returns.push(entry);
      continuation = entry;
    } else if (note) {
      const entry = { description: note[1] ?? '' };
      notes.push(entry);
      continuation = entry;
    } else if (line === '@private') {
      privateApi = true;
      continuation = undefined;
    } else if (line && continuation && /^\s/u.test(sourceLine)) {
      continuation.description = `${continuation.description} ${line}`.trim();
    } else if (line) {
      description.push(line);
      continuation = undefined;
    }
  }

  const sections = [];
  if (description.length > 0) {
    sections.push(description.join(' '));
  }
  if (parameters.length > 0) {
    sections.push(
      `**Parameters**\n\n${parameters.map((parameter) =>
        `- \`${parameter.name}\` — ${parameter.description}`).join('\n')}`,
    );
  }
  if (returns.length > 0) {
    sections.push(`**Returns**\n\n${returns.map((entry) => entry.description).join(' ')}`);
  }
  if (notes.length > 0) {
    sections.push(`**Notes**\n\n${notes.map((entry) => entry.description).join(' ')}`);
  }
  if (privateApi) {
    sections.push('_Internal API._');
  }
  return sections.join('\n\n');
}

export function nativeBindingPath(
  extensionPath: string,
  platform: NodeJS.Platform = process.platform,
  architecture = process.arch,
): string {
  if (platform !== 'darwin' || architecture !== 'arm64') {
    throw new Error(
      `the bundled nwnrs compiler does not yet support ${platform}-${architecture}; `
      + 'Windows, Linux, and other macOS architectures are tracked in VSCODE_TODO.md',
    );
  }
  return path.join(extensionPath, 'native', 'nwnrs-vscode.darwin-arm64.node');
}

export function diagnosticRange(record: NativeDiagnosticRecord): {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
} {
  const startLine = positiveInteger(record.start_line, 1) - 1;
  const startColumn = positiveInteger(record.start_column, 1) - 1;
  const endLine = Math.max(startLine, positiveInteger(record.end_line, startLine + 1) - 1);
  let endColumn = positiveInteger(record.end_column, startColumn + 2) - 1;
  if (endLine === startLine && endColumn <= startColumn) {
    endColumn = startColumn + 1;
  }
  return { startLine, startColumn, endLine, endColumn };
}

function positiveInteger(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isInteger(value) && value > 0 ? value : fallback;
}
