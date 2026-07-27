import * as fs from 'node:fs';
import * as path from 'node:path';
import type {
  NativeCheckRequest,
  NativeDefinition,
  NativeDefinitionRequest,
  NativePackageConfiguration,
  NativeRange,
} from './generated/native';

export interface PathVariableContext {
  readonly workspaceFolder?: string;
  readonly projectRoot?: string;
  readonly fileDirname?: string;
}

export interface CompilerRequestOptions {
  readonly noEntrypointCheck?: boolean;
  readonly langspecPath?: string | null;
  readonly includeDirectories?: readonly string[];
  readonly maxIncludeDepth?: number;
  readonly maxDiagnosticsPerFile?: number;
  readonly recurse?: boolean;
  readonly rootPath?: string | null;
  readonly userPath?: string | null;
  readonly language?: string;
  readonly loadOvr?: boolean;
  readonly gitExecutable?: string | null;
  readonly packageCache?: string | null;
  readonly qualifier?: string | null;
  readonly projectRoot?: string | null;
  readonly resource?: string | null;
}

export type { NativeCheckRequest, NativeDefinitionRequest };
export type NativeDocumentSymbolsRequest = NativePackageConfiguration & {
  readonly resource?: string;
};

interface DocumentationEntry {
  description: string;
}

interface DocumentationParameter extends DocumentationEntry {
  name: string;
}

export function isNssPath(filePath: string): boolean {
  return path.extname(filePath).toLowerCase() === '.nss';
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
  const request: NativeCheckRequest = {
    paths: [...targets],
    noEntrypointCheck: options.noEntrypointCheck !== false,
    langspec: options.langspecPath || undefined,
    includeDirectories: [...(options.includeDirectories || [])],
    optimization: 'O0',
    optimizationFlags: [],
    maxIncludeDepth: options.maxIncludeDepth || 16,
    maxDiagnosticsPerInput: options.maxDiagnosticsPerFile || 50,
    recurse: options.recurse === true,
    followSymlinks: false,
    jobs: undefined,
    root: options.rootPath || undefined,
    user: options.userPath || undefined,
    language: options.language || 'english',
    loadOvr: options.loadOvr === true,
  };
  if (options.gitExecutable) request.gitExecutable = options.gitExecutable;
  if (options.packageCache) request.packageCache = options.packageCache;
  return request;
}

export function buildDefinitionRequest(
  sourcePath: string,
  symbol: string,
  options: CompilerRequestOptions = {},
): NativeDefinitionRequest {
  return {
    package: buildDocumentSymbolsRequest(sourcePath, options),
    symbol,
    qualifier: options.qualifier || undefined,
  };
}

export function buildDocumentSymbolsRequest(
  sourcePath: string,
  options: CompilerRequestOptions = {},
): NativeDocumentSymbolsRequest {
  const request: NativeDocumentSymbolsRequest = {
    sourcePath: options.resource || sourcePath,
    resource: options.resource || undefined,
    projectRoot: options.projectRoot || undefined,
    includeDirectories: [...(options.includeDirectories || [])],
    langspec: options.langspecPath || undefined,
    maxIncludeDepth: options.maxIncludeDepth || 16,
    root: options.rootPath || undefined,
    user: options.userPath || undefined,
    language: options.language || 'english',
    loadOvr: options.loadOvr === true,
  };
  if (options.gitExecutable) request.gitExecutable = options.gitExecutable;
  if (options.packageCache) request.packageCache = options.packageCache;
  return request;
}

export function selectHoverDefinition(
  definitions: readonly NativeDefinition[] | unknown,
): NativeDefinition | undefined {
  if (!Array.isArray(definitions) || definitions.length === 0) {
    return undefined;
  }
  return definitions.find((definition) =>
    definition.isImplementation && definition.documentation)
    || definitions.find((definition) => definition.documentation)
    || definitions.find((definition) => definition.isImplementation)
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
  const target = `${platform}-${architecture}`;
  const supported = new Set([
    'darwin-arm64',
    'linux-x64',
    'win32-x64',
  ]);
  if (!supported.has(target)) {
    throw new Error(
      `the bundled nwnrs native service does not support ${target}; supported targets are `
      + [...supported].join(', '),
    );
  }
  const binding = path.join(extensionPath, 'native', `nwnrs-vscode.${target}.node`);
  if (!fs.existsSync(binding)) {
    throw new Error(`the bundled native service is missing for ${target}: ${binding}`);
  }
  return binding;
}

export function diagnosticRange(record: NativeRange): {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
} {
  const startLine = record.start.line;
  const startColumn = record.start.character;
  const endLine = Math.max(startLine, record.end.line);
  let endColumn = record.end.character;
  if (endLine === startLine && endColumn <= startColumn) {
    endColumn = startColumn + 1;
  }
  return { startLine, startColumn, endLine, endColumn };
}
