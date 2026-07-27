import * as fs from 'node:fs';
import * as path from 'node:path';

interface ResourceCapability {
  readonly extension: string;
  readonly handler: string;
  readonly customEditor: boolean;
  readonly viewer: boolean;
  readonly text: boolean;
  readonly writable: boolean;
}

interface ExtensionManifest {
  contributes?: {
    customEditors?: Array<{
      viewType?: string;
      selector?: Array<{ filenamePattern?: string }>;
    }>;
  };
}

const extensionRoot = path.resolve(__dirname, '..', '..');
const registryPath = path.join(extensionRoot, 'resource-capabilities.json');
const packagePath = path.join(extensionRoot, 'package.json');
const hostOutputPath = path.join(
  extensionRoot,
  'src',
  'resource-capabilities.generated.ts',
);
const webviewOutputPath = path.join(
  extensionRoot,
  'media',
  'resource-capabilities.generated.ts',
);
const write = process.argv.includes('--write');

function parseRegistry(): readonly ResourceCapability[] {
  const value: unknown = JSON.parse(fs.readFileSync(registryPath, 'utf8'));
  if (!Array.isArray(value)) throw new Error('resource capability registry must be an array');
  const seen = new Set<string>();
  return value.map((entry, index) => {
    if (typeof entry !== 'object' || entry === null) {
      throw new Error(`resource capability ${index} must be an object`);
    }
    const record = entry as Readonly<Record<string, unknown>>;
    const extension = record.extension;
    const handler = record.handler;
    if (
      typeof extension !== 'string'
      || !/^[a-z0-9]+(?:\.[a-z0-9]+)*$/u.test(extension)
    ) {
      throw new Error(`resource capability ${index} has an invalid extension`);
    }
    if (seen.has(extension)) throw new Error(`duplicate resource capability: ${extension}`);
    seen.add(extension);
    if (
      typeof handler !== 'string'
      || !['2da', 'tlk', 'dds', 'tga', 'plt', 'gff', 'erf', 'key', 'ncs', 'ndb', 'viewer', 'text'].includes(handler)
    ) {
      throw new Error(`resource capability ${extension} has an invalid handler`);
    }
    for (const field of ['customEditor', 'viewer', 'text', 'writable'] as const) {
      if (typeof record[field] !== 'boolean') {
        throw new Error(`resource capability ${extension} requires boolean ${field}`);
      }
    }
    const capability = {
      extension,
      handler,
      customEditor: record.customEditor as boolean,
      viewer: record.viewer as boolean,
      text: record.text as boolean,
      writable: record.writable as boolean,
    };
    if (capability.viewer && !capability.customEditor) {
      throw new Error(`viewer resource ${extension} must contribute the custom editor`);
    }
    if (capability.text && capability.customEditor) {
      throw new Error(`text resource ${extension} cannot also use the custom editor`);
    }
    if (capability.viewer && capability.writable) {
      throw new Error(`viewer resource ${extension} cannot be writable`);
    }
    if (['ncs', 'ndb'].includes(capability.handler) && capability.writable) {
      throw new Error(`script workbench resource ${extension} cannot be writable`);
    }
    return capability;
  });
}

function generatedHost(capabilities: readonly ResourceCapability[]): string {
  const suffixes = (predicate: (value: ResourceCapability) => boolean): string[] => capabilities
    .filter(predicate)
    .map((value) => `.${value.extension}`);
  return `// Generated from ../resource-capabilities.json by sync-resource-capabilities.ts.
// Do not edit by hand.

const CUSTOM_EDITOR_SUFFIXES = ${JSON.stringify(suffixes((value) => value.customEditor))} as const;
const VIEWER_SUFFIXES = ${JSON.stringify(suffixes((value) => value.viewer))} as const;
const TEXT_SUFFIXES = ${JSON.stringify(suffixes((value) => value.text))} as const;

function hasSuffix(value: string, suffixes: readonly string[]): boolean {
  const normalized = value.toLowerCase();
  return suffixes.some((suffix) => normalized.endsWith(suffix));
}

export function isCustomEditorResource(value: string): boolean {
  return hasSuffix(value, CUSTOM_EDITOR_SUFFIXES);
}

export function isViewerResource(value: string): boolean {
  return hasSuffix(value, VIEWER_SUFFIXES);
}

export function isTextResource(value: string): boolean {
  return hasSuffix(value, TEXT_SUFFIXES);
}
`;
}

function generatedWebview(capabilities: readonly ResourceCapability[]): string {
  const resourceTypes = capabilities
    .filter((value) => value.customEditor)
    .map((value) => value.extension);
  return `// Generated from ../resource-capabilities.json by sync-resource-capabilities.ts.
// Do not edit by hand.

var CUSTOM_EDITOR_RESOURCE_TYPES: ReadonlySet<string> = new Set(${JSON.stringify(resourceTypes)});
`;
}

function expectedPackage(
  manifest: ExtensionManifest,
  capabilities: readonly ResourceCapability[],
): string {
  const editor = manifest.contributes?.customEditors?.find(
    (candidate) => candidate.viewType === 'nwnrs.resourceEditor',
  );
  if (!editor) throw new Error('package.json does not contribute nwnrs.resourceEditor');
  editor.selector = capabilities
    .filter((value) => value.customEditor)
    .map((value) => ({ filenamePattern: `*.${value.extension}` }));
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

function synchronize(target: string, expected: string): void {
  const actual = fs.existsSync(target) ? fs.readFileSync(target, 'utf8') : '';
  if (actual === expected) return;
  if (!write) {
    throw new Error(
      `${path.relative(extensionRoot, target)} is stale; run npm run sync:capabilities`,
    );
  }
  fs.writeFileSync(target, expected);
}

const capabilities = parseRegistry();
const manifest = JSON.parse(fs.readFileSync(packagePath, 'utf8')) as ExtensionManifest;
synchronize(hostOutputPath, generatedHost(capabilities));
synchronize(webviewOutputPath, generatedWebview(capabilities));
synchronize(packagePath, expectedPackage(manifest, capabilities));
