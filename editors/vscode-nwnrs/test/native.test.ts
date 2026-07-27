'use strict';

import type {
  NativeDocumentSymbol,
  NativeLanguageDiagnostic as NativeCheckDiagnostic,
  NativeReference,
  NativeViewerAreaObject as NativeAreaObject,
  NativeViewerPackageInfo as NativePackageInfo,
  NativeViewerPackageSourceArea as NativePackageSourceArea,
  NativeViewerPackageSourceFile as NativePackageSourceFile,
  NativeViewerResourceCatalogItem as NativeResourceCatalogItem,
  ResourceEditorService as NativeResourceService,
} from '../src/generated/native';
import type { ResourceEdit } from '../src/generated/ResourceEdit';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { Worker } = require('node:worker_threads');
const { LanguageWorkerClient } = require('../dist/src/language-worker-client');
const { ResourceEditorWorkerClient } = require('../dist/src/resource-editor-worker-client');
const { ViewerWorkerClient } = require('../dist/src/viewer-worker-client');

const bindingPath = path.resolve(
  __dirname,
  '..',
  'native',
  'nwnrs-vscode.darwin-arm64.node',
);

interface TestContext {
  skip(message?: string): void;
}

interface ViewerPacketResult {
  readonly manifest: {
    readonly assetKey?: string;
    readonly name?: string;
    readonly source?: string;
    readonly models: Array<{ readonly animations: PacketAnimation[] }>;
    readonly animation: PacketAnimation;
  };
  readonly binaryStart: number;
}

interface WorkerIndexResponse {
  readonly documents: Array<{
    readonly symbols: Array<{ readonly name: string }>;
  }>;
}

function isWorkerIndexResponse(value: unknown): value is WorkerIndexResponse {
  return typeof value === 'object'
    && value !== null
    && 'documents' in value
    && Array.isArray(value.documents)
    && value.documents.every((document) => typeof document === 'object'
      && document !== null
      && 'symbols' in document
      && Array.isArray(document.symbols)
      && document.symbols.every((symbol: unknown) => typeof symbol === 'object'
        && symbol !== null
        && 'name' in symbol
        && typeof symbol.name === 'string'));
}

function required<Value>(value: Value | null | undefined, label: string): Value {
  assert.ok(value, label);
  if (value == null) throw new Error(label);
  return value;
}

async function resourceRequest(
  service: NativeResourceService,
  method: string,
  request: unknown,
): Promise<any> {
  const value = request as Record<string, unknown>;
  const documentRequest = {
    documentId: String(value.documentId ?? ''),
    path: typeof value.path === 'string' ? value.path : undefined,
    backupPath: typeof value.backupPath === 'string' ? value.backupPath : undefined,
    readOnlyOrigin: typeof value.readOnlyOrigin === 'boolean' ? value.readOnlyOrigin : undefined,
    offset: typeof value.offset === 'number' ? value.offset : undefined,
    limit: typeof value.limit === 'number' ? value.limit : undefined,
    query: typeof value.query === 'string' ? value.query : undefined,
    force: typeof value.force === 'boolean' ? value.force : undefined,
    confirmationToken: typeof value.confirmationToken === 'string'
      ? value.confirmationToken : undefined,
    resource: typeof value.resource === 'string' ? value.resource : undefined,
  };
  switch (method) {
    case 'openDocument':
      return service.openDocument({
        documentId: documentRequest.documentId,
        path: String(value.path ?? ''),
        backupPath: documentRequest.backupPath,
        readOnlyOrigin: documentRequest.readOnlyOrigin,
      });
    case 'openDocumentBytes':
      return service.openDocumentBytes(
        {
          documentId: documentRequest.documentId,
          path: String(value.path ?? ''),
          backupPath: documentRequest.backupPath,
          readOnlyOrigin: documentRequest.readOnlyOrigin,
        },
        Buffer.from(value.contents as Uint8Array),
      );
    case 'snapshot':
      return service.snapshot({
        documentId: documentRequest.documentId,
        offset: documentRequest.offset,
        limit: documentRequest.limit,
        query: documentRequest.query,
      });
    case 'gffNode':
      return service.gffNode({
        documentId: documentRequest.documentId,
        path: Array.isArray(value.path) ? value.path : [],
        offset: documentRequest.offset,
        limit: documentRequest.limit,
      });
    case 'configureScriptDebug':
      return service.configureScriptDebug({ documentId: documentRequest.documentId }, {
        ncs: value.ncs as Buffer | undefined,
        ndb: value.ndb as Buffer | undefined,
        langspec: value.langspec as Buffer | undefined,
        sourceNames: value.sourceNames as string[] | undefined,
        sourceContents: value.sourceContents as Buffer[] | undefined,
      });
    case 'applyEdit':
      return service.applyEdit(
        { documentId: documentRequest.documentId },
        value.edit as ResourceEdit,
      );
    case 'exportDocument':
      return service.exportDocument(documentRequest.documentId);
    case 'readTexturePixels':
      return service.readTexturePixels(documentRequest.documentId);
    case 'replaceTexturePixels':
      return service.replaceTexturePixels({
        documentId: documentRequest.documentId,
        width: Number(value.width),
        height: Number(value.height),
      }, Buffer.from(value.rgba as Uint8Array));
    case 'applyBinaryUndo':
      return service.applyBinaryUndo({
        documentId: documentRequest.documentId,
        token: String(value.token ?? ''),
      });
    case 'saveDocument':
      return service.saveDocument({
        documentId: documentRequest.documentId,
        force: documentRequest.force,
      });
    case 'planSaveDocumentAs':
      return service.planSaveDocumentAs({
        documentId: documentRequest.documentId,
        path: String(value.path ?? ''),
      });
    case 'saveDocumentAs':
      return service.saveDocumentAs({
        documentId: documentRequest.documentId,
        path: String(value.path ?? ''),
        confirmationToken: documentRequest.confirmationToken,
      });
    case 'backupDocument':
      return service.backupDocument({
        documentId: documentRequest.documentId,
        path: String(value.path ?? ''),
      });
    case 'revertDocument':
      return service.revertDocument({
        documentId: documentRequest.documentId,
        offset: documentRequest.offset,
        limit: documentRequest.limit,
        query: documentRequest.query,
      });
    case 'closeDocument':
      return service.closeDocument({ documentId: documentRequest.documentId });
    default:
      throw new Error(`unsupported typed resource test method: ${method}`);
  }
}

function testPackageConfiguration(request: Record<string, unknown>) {
  return {
    sourcePath: String(request.source_path ?? request.sourcePath ?? ''),
    projectRoot: typeof (request.project_root ?? request.projectRoot) === 'string'
      ? String(request.project_root ?? request.projectRoot) : undefined,
    includeDirectories: Array.isArray(request.include_dirs ?? request.includeDirectories)
      ? (request.include_dirs ?? request.includeDirectories) as string[] : [],
    langspec: typeof request.langspec === 'string' ? request.langspec : undefined,
    maxIncludeDepth: typeof request.max_include_depth === 'number'
      ? request.max_include_depth : 64,
    root: typeof request.root === 'string' ? request.root : undefined,
    user: typeof request.user === 'string' ? request.user : undefined,
    language: typeof request.language === 'string' ? request.language : 'en',
    loadOvr: request.load_ovr === true || request.loadOvr === true,
  };
}

async function languageRequest(
  binding: any,
  method: string,
  requestValue: unknown = {},
): Promise<any> {
  const request = requestValue as Record<string, unknown>;
  const service = languageService(binding);
  const packageConfiguration = testPackageConfiguration(request);
  if (packageConfiguration.projectRoot) service.addWorkspace(packageConfiguration.projectRoot);
  const overlays = Array.isArray(request.overlays)
    ? request.overlays as Array<{ path: string; contents: string }> : [];
  for (const [version, overlay] of overlays.entries()) {
    service.openDocument(overlay.path, version + 1, Buffer.from(overlay.contents, 'latin1'));
  }
  const signal = new AbortController().signal;
  switch (method) {
    case 'checkNss':
      return service.checkNss({
        paths: Array.isArray(request.paths) ? request.paths : [],
        noEntrypointCheck: request.no_entrypoint_check === true,
        langspec: typeof request.langspec === 'string' ? request.langspec : undefined,
        includeDirectories: packageConfiguration.includeDirectories,
        optimization: typeof request.optimization === 'string' ? request.optimization : 'O0',
        optimizationFlags: Array.isArray(request.optimization_flags)
          ? request.optimization_flags : [],
        maxIncludeDepth: packageConfiguration.maxIncludeDepth,
        maxDiagnosticsPerInput: typeof request.max_diagnostics_per_input === 'number'
          ? request.max_diagnostics_per_input : 100,
        recurse: request.recurse === true,
        followSymlinks: request.follow_symlinks === true,
        jobs: typeof request.jobs === 'number' ? request.jobs : undefined,
        root: packageConfiguration.root,
        user: packageConfiguration.user,
        language: packageConfiguration.language,
        loadOvr: packageConfiguration.loadOvr,
      }, signal);
    case 'findDefinitions':
      return service.findDefinitions({
        package: packageConfiguration,
        symbol: String(request.symbol ?? ''),
        qualifier: typeof request.qualifier === 'string' ? request.qualifier : undefined,
      }, signal);
    case 'completeNwscript':
      return service.completeNwscript({
        package: packageConfiguration,
        line: Number(request.line ?? 0),
        column: Number(request.column ?? 0),
      }, signal);
    case 'signatureHelp':
      return service.signatureHelp({
        package: packageConfiguration,
        line: Number(request.line ?? 0),
        column: Number(request.column ?? 0),
      }, signal);
    case 'findIncludeCandidates':
      return service.findIncludeCandidates({
        package: packageConfiguration,
        symbol: String(request.symbol ?? ''),
        qualifier: typeof request.qualifier === 'string' ? request.qualifier : undefined,
      }, signal);
    case 'findReferences':
      return service.findReferences({
        package: packageConfiguration,
        line: Number(request.line ?? 0),
        column: Number(request.column ?? 0),
      }, signal);
    case 'listDocumentSymbols':
      return service.listDocumentSymbols(packageConfiguration, signal);
    case 'indexProject':
      return service.indexProject(packageConfiguration, signal);
    case 'analyzeDocument':
      return service.analyzeDocument(packageConfiguration, signal);
    case 'readVirtualSource':
      return service.readVirtualSource({
        package: packageConfiguration,
        resource: String(request.resource ?? ''),
      });
    case 'resolveSource':
      return service.resolveSource({
        package: packageConfiguration,
        resource: String(request.resource ?? ''),
      });
    case 'deduplicateProjectRoots':
      return service.deduplicateProjectRoots({
        roots: Array.isArray(request.roots) ? request.roots : [],
      });
    case 'resolveWatchRoots':
      return service.resolveWatchRoots({
        roots: Array.isArray(request.roots) ? request.roots : [],
      });
    case 'checkNwpkg':
      service.openDocument(
        String(request.path ?? ''),
        1,
        Buffer.from(String(request.contents ?? ''), 'utf8'),
      );
      return {
        diagnostics: service.manifestDiagnostics({
          path: String(request.path ?? ''),
        }),
      };
    case 'listEventCatalog':
      return service.listEventCatalog();
    case 'eventCompletionsAt':
      return service.eventCompletionsAt({
        package: packageConfiguration,
        offset: Number(request.offset ?? 0),
      }, signal);
    case 'resolveEventSymbolAt':
      return service.resolveEventSymbolAt({
        package: packageConfiguration,
        offset: Number(request.offset ?? 0),
      }, signal);
    default:
      throw new Error(`unsupported typed language test method: ${method}`);
  }
}

function languageService(binding: any): any {
  const service = new binding.LanguageService();
  return new Proxy(service, {
    get(target, property, receiver) {
      const value = Reflect.get(target, property, receiver);
      if (typeof value !== 'function') return value;
      return (...arguments_: unknown[]) => {
        const result = Reflect.apply(value, target, arguments_) as unknown;
        return result instanceof Promise
          ? result.then(unwrapNativeLanguageResult)
          : unwrapNativeLanguageResult(result);
      };
    },
  });
}

function unwrapNativeLanguageResult(result: unknown): unknown {
  if (typeof result !== 'object' || result === null) return result;
  const record = result as {
    readonly value?: unknown;
    readonly error?: {
      readonly code: string;
      readonly message: string;
      readonly path?: string;
      readonly manifestField: readonly string[];
    };
    readonly succeeded?: boolean;
  };
  if (!Object.hasOwn(record, 'error')
    && !Object.hasOwn(record, 'value')
    && !Object.hasOwn(record, 'succeeded')) {
    return result;
  }
  if (record.error) {
    const error = new Error(record.error.message) as Error & {
      code: string;
      path?: string;
      manifestField: readonly string[];
    };
    error.code = record.error.code;
    if (record.error.path !== undefined) error.path = record.error.path;
    error.manifestField = record.error.manifestField;
    throw error;
  }
  if (Object.hasOwn(record, 'succeeded')) {
    if (record.succeeded !== true) throw new Error('native lifecycle operation did not succeed');
    return undefined;
  }
  return record.value;
}

function typedViewerLoadRequest(value: Record<string, any>) {
  const authored = value.authoredArea ?? value.authored_area;
  return {
    sessionKey: String(value.sessionKey ?? value.session_key ?? ''),
    path: String(value.path ?? ''),
    projectRoot: String(value.projectRoot ?? value.project_root ?? ''),
    area: typeof value.area === 'string' ? value.area : undefined,
    authoredArea: authored ? {
      resref: String(authored.resref ?? ''),
      are: String(authored.are ?? ''),
      git: String(authored.git ?? ''),
      gic: typeof authored.gic === 'string' ? authored.gic : undefined,
    } : undefined,
    root: typeof value.root === 'string' ? value.root : undefined,
    user: typeof value.user === 'string' ? value.user : undefined,
    language: typeof value.language === 'string' ? value.language : undefined,
    loadOvr: value.loadOvr === true || value.load_ovr === true,
    archives: Array.isArray(value.archives) ? value.archives : undefined,
    includeProjectResources:
      value.includeProjectResources === true || value.include_project_resources === true,
  };
}

function decodeViewerPacket(packet: Buffer): ViewerPacketResult {
  assert.equal(packet.subarray(0, 8).toString('binary'), 'NWNRS3D\0');
  const manifestLength = packet.readUInt32LE(8);
  const binaryStart = 12 + manifestLength;
  assert.ok(binaryStart <= packet.length, 'viewer packet manifest is truncated');
  return {
    manifest: JSON.parse(packet.subarray(12, binaryStart).toString('utf8')),
    binaryStart,
  };
}

function assertViewerPacketTypedViewsAreConstructible(
  packet: Buffer,
  manifest: unknown,
  binaryStart: number,
): void {
  const visit = (value: unknown): void => {
    if (!value || typeof value !== 'object') return;
    if (!Array.isArray(value)
        && 'byteOffset' in value
        && 'byteLength' in value
        && 'component' in value
        && typeof value.byteOffset === 'number'
        && typeof value.byteLength === 'number'
        && typeof value.component === 'string') {
      const byteLength = value.byteLength;
      const absoluteOffset = packet.byteOffset + binaryStart + value.byteOffset;
      const packetBuffer = packet.buffer;
      if (!(packetBuffer instanceof ArrayBuffer)) {
        throw new Error('Viewer packet does not own an ArrayBuffer.');
      }
      assert.ok(binaryStart + value.byteOffset + byteLength <= packet.length);
      if (value.component === 'u8') {
        assert.doesNotThrow(() => new Uint8Array(packetBuffer, absoluteOffset, byteLength));
      } else {
        assert.equal(absoluteOffset % 4, 0, `${value.component} view starts at ${absoluteOffset}`);
        assert.equal(byteLength % 4, 0, `${value.component} view has partial scalar bytes`);
        const View = value.component === 'f32' ? Float32Array
          : value.component === 'u32' ? Uint32Array : Int32Array;
        assert.doesNotThrow(() => new View(packetBuffer, absoluteOffset, byteLength / 4));
      }
      return;
    }
    for (const child of Array.isArray(value) ? value : Object.values(value)) visit(child);
  };
  visit(manifest);
}

test('persistent resource worker opens the complete valid GFF corpus', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const corpusDirectory = path.join(
    repositoryRoot,
    'sources',
    'neverwinter.nim',
    'tests',
    'fuzzing',
    'gff-testing-corpus',
  );
  if (!fs.existsSync(corpusDirectory)) {
    context.skip('GFF compatibility corpus is not available');
    return;
  }
  const fixtures = fs.readdirSync(corpusDirectory)
    .map((name: string) => path.join(corpusDirectory, name))
    .filter((fixture: string) => fs.statSync(fixture).isFile())
    .sort();
  assert.ok(fixtures.length > 0, 'GFF compatibility corpus must not be empty');

  const client = new ResourceEditorWorkerClient(
    path.resolve(__dirname, '..', 'dist', 'src', 'resource-editor-worker.js'),
    bindingPath,
    { appendLine() {} },
  );
  try {
    for (const [index, fixture] of fixtures.entries()) {
      const documentId = `gff-corpus-${index}`;
      const snapshot = await client.request('openDocument', {
        documentId,
        path: fixture,
      });
      assert.equal(snapshot.kind, 'gff', path.basename(fixture));
      assert.equal(snapshot.data.fileType.length, 4, path.basename(fixture));
      assert.ok(Array.isArray(snapshot.data.root.fields), path.basename(fixture));
      await client.request('closeDocument', { documentId });
    }
  } finally {
    client.dispose();
  }
});

test('native resource editor exposes an editable 2DA custom-document lifecycle', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-resource-2da-'));
  const sourcePath = path.join(root, 'demo.2da');
  const backupPath = path.join(root, 'demo.backup');
  fs.writeFileSync(sourcePath, '2DA V2.0\n\nName Value\n0 alpha 1\n1 beta ****\n');
  const service = new binding.ResourceEditorService();
  const documentId = '2da-document';
  try {
    const opened = await resourceRequest(service, 'openDocument', {
      documentId,
      path: sourcePath,
    });
    assert.equal(opened.kind, '2da');
    assert.deepEqual(opened.data.columns, ['Name', 'Value']);
    assert.equal(opened.data.rows[1].cells[1], null);

    const changed = await resourceRequest(service, 'applyEdit', {
      documentId,
      edit: { action: 'set2daCell', row: 1, column: 'Value', value: '7' },
    });
    assert.equal(changed.snapshot.data.rows[1].cells[1], '7');
    assert.deepEqual(changed.inverse, {
      action: 'set2daCell', row: 1, column: 'Value', value: null,
    });

    await resourceRequest(service, 'backupDocument', { documentId, path: backupPath });
    assert.ok(fs.statSync(backupPath).size > 0);
    assert.deepEqual(
      fs.readFileSync(backupPath).subarray(0, 8),
      Buffer.from('NWNRSB02'),
    );
    const restoredId = '2da-restored-document';
    const restored = await resourceRequest(service, 'openDocument', {
      documentId: restoredId,
      path: sourcePath,
      backupPath,
    });
    assert.equal(restored.data.rows[1].cells[1], '7');
    await resourceRequest(service, 'closeDocument', { documentId: restoredId });
    await resourceRequest(service, 'saveDocument', { documentId });
    assert.match(fs.readFileSync(sourcePath, 'utf8'), /beta\s+7/u);

    await resourceRequest(service, 'applyEdit', {
      documentId,
      edit: changed.inverse,
    });
    const reverted = await resourceRequest(service, 'snapshot', { documentId });
    assert.equal(reverted.data.rows[1].cells[1], null);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native resource editor exposes structured NCS control flow and NDB source mappings', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const service = new binding.ResourceEditorService();
  const code = Buffer.from([
    0x1e, 0x00, 0x00, 0x00, 0x00, 0x08, // JSR sub_0008
    0x20, 0x00, // RET
    0x20, 0x00, // sub_0008: RET
  ]);
  const ncs = Buffer.alloc(13 + code.length);
  ncs.write('NCS V1.0B', 0, 'ascii');
  ncs.writeUInt32BE(ncs.length, 9); code.copy(ncs, 13);
  const ndb = Buffer.from([
    'NDB V1.0',
    '0000001 0000000 0000001 0000000 0000001',
    'N00 helper',
    'f 00000015 00000017 000 v helper',
    'l00 0000001 00000015 00000017',
    '',
  ].join('\n'));
  const documentId = 'ncs-workbench-document';
  const opened = await resourceRequest(service, 'openDocumentBytes', {
    documentId,
    path: '/virtual/demo.ncs',
    contents: ncs,
  });
  assert.equal(opened.kind, 'ncs');
  assert.equal(opened.data.header.instructionCount, 3);
  assert.equal(opened.data.instructions[0].opcode, 'JSR');
  assert.equal(opened.data.instructions[0].jumpTarget, 8);
  assert.deepEqual(opened.data.instructions[0].successors, [{ offset: 6, kind: 'fallthrough' }]);
  assert.equal(opened.data.functions[1].name, 'sub_0008');
  assert.ok(opened.data.functions[0].blocks.length > 0);

  const configured = await resourceRequest(service, 'configureScriptDebug', {
    documentId,
    ndb,
    sourceNames: ['helper'],
    sourceContents: [Buffer.from('void helper() { return; }\n')],
  });
  assert.equal(configured.data.hasNdb, true);
  assert.equal(configured.data.sourceFiles[0].available, true);
  const helper = configured.data.functions.find(
    (entry: { readonly name: string; readonly synthetic: boolean }) => entry.name === 'helper',
  );
  assert.ok(helper);
  assert.equal(helper.synthetic, false);
  const mapped = configured.data.instructions.find(
    (entry: { readonly offset: number; readonly source: unknown }) => entry.offset === 8,
  );
  assert.deepEqual(mapped.source, {
    file: 'helper', line: 1, text: 'void helper() { return; }', available: true,
  });
  await resourceRequest(service, 'closeDocument', { documentId });
});

test('native resource editor detects external changes before overwriting a file', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-resource-conflict-'));
  const sourcePath = path.join(root, 'conflict.2da');
  fs.writeFileSync(sourcePath, '2DA V2.0\n\nValue\n0 original\n');
  const service = new binding.ResourceEditorService();
  try {
    await resourceRequest(service, 'openDocument', { documentId: 'conflict', path: sourcePath });
    await resourceRequest(service, 'applyEdit', {
      documentId: 'conflict',
      edit: { action: 'set2daCell', row: 0, column: 'Value', value: 'editor' },
    });
    fs.writeFileSync(sourcePath, '2DA V2.0\n\nValue\n0 external\n');
    await assert.rejects(
      resourceRequest(service, 'saveDocument', { documentId: 'conflict' }),
      /EXTERNAL_CHANGE/u,
    );
    assert.match(fs.readFileSync(sourcePath, 'utf8'), /external/u);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native resource editor decodes, replaces, and exports TGA pixels', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-resource-tga-'));
  const sourcePath = path.join(root, 'pixel.tga');
  const header = Buffer.alloc(18);
  header[2] = 2;
  header.writeUInt16LE(1, 12);
  header.writeUInt16LE(1, 14);
  header[16] = 32;
  header[17] = 0x28;
  fs.writeFileSync(sourcePath, Buffer.concat([header, Buffer.from([0, 0, 255, 255])]));
  const service = new binding.ResourceEditorService();
  try {
    const opened = await resourceRequest(service, 'openDocument', {
      documentId: 'texture', path: sourcePath,
    });
    assert.equal(opened.kind, 'tga');
    assert.deepEqual(
      Buffer.from(await resourceRequest(service, 'readTexturePixels', { documentId: 'texture' })),
      Buffer.from([255, 0, 0, 255]),
    );
    const changed = await resourceRequest(service, 'replaceTexturePixels', {
      documentId: 'texture',
      width: 1,
      height: 1,
      rgba: Buffer.from([0, 255, 0, 255]),
    });
    assert.equal(typeof changed.inverseToken, 'string');
    const exported = await resourceRequest(service, 'exportDocument', { documentId: 'texture' });
    const bytes = Buffer.from(exported);
    assert.deepEqual(bytes.subarray(18, 22), Buffer.from([0, 255, 0, 255]));
    await resourceRequest(service, 'applyBinaryUndo', {
      documentId: 'texture',
      token: changed.inverseToken,
    });
    const restored = Buffer.from(
      await resourceRequest(service, 'exportDocument', { documentId: 'texture' }),
    );
    assert.deepEqual(restored, Buffer.concat([header, Buffer.from([0, 0, 255, 255])]));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native TLK edits page lazily and undo back to the original bytes', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-resource-tlk-'));
  const sourcePath = path.join(root, 'dialog.tlk');
  const header = Buffer.alloc(20);
  header.write('TLK ', 0, 'ascii');
  header.write('V3.0', 4, 'ascii');
  header.writeInt32LE(0, 8);
  header.writeInt32LE(1, 12);
  header.writeInt32LE(60, 16);
  const descriptor = Buffer.alloc(40);
  descriptor.writeInt32LE(1, 0);
  descriptor.writeInt32LE(0, 28);
  descriptor.writeInt32LE(5, 32);
  const original = Buffer.concat([header, descriptor, Buffer.from('Hello')]);
  fs.writeFileSync(sourcePath, original);
  const service = new binding.ResourceEditorService();
  try {
    const opened = await resourceRequest(service, 'openDocument', {
      documentId: 'tlk', path: sourcePath,
    });
    assert.equal(opened.data.entries[0].text, 'Hello');
    const changed = await resourceRequest(service, 'applyEdit', {
      documentId: 'tlk',
      edit: {
        action: 'setTlkEntry', strRef: 0,
        entry: { ...opened.data.entries[0], text: 'Changed' },
      },
    });
    assert.equal(changed.inverse.action, 'clearTlkOverride');
    await resourceRequest(service, 'applyEdit', {
      documentId: 'tlk', edit: changed.inverse,
    });
    const exported = await resourceRequest(service, 'exportDocument', { documentId: 'tlk' });
    assert.deepEqual(Buffer.from(exported), original);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native custom-document lifecycle edits DLG JSON without changing its source format', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-dialog-json-'));
  const sourcePath = path.join(root, 'conversation.dlg.json');
  const source: Record<string, unknown> = {
    __data_type: 'DLG ',
    EndConverAbort: { type: 'resref', value: 'before' },
  };
  for (let index = 0; index < 205; index += 1) {
    source[`Field${index}`] = { type: 'int', value: index };
  }
  fs.writeFileSync(sourcePath, JSON.stringify(source, null, 2));
  const service = new binding.ResourceEditorService();
  try {
    const opened = await resourceRequest(service, 'openDocument', {
      documentId: 'dialog-json', path: sourcePath,
    });
    assert.equal(opened.kind, 'gff');
    assert.equal(opened.data.fileType, 'DLG ');
    assert.equal(opened.data.root.total, 206);
    assert.equal(opened.data.root.fields.length, 200);
    const finalPage = await resourceRequest(service, 'gffNode', {
      documentId: 'dialog-json', path: [], offset: 200, limit: 200,
    });
    assert.equal(finalPage.fields.length, 6);
    await resourceRequest(service, 'applyEdit', {
      documentId: 'dialog-json',
      edit: {
        action: 'setGffValue',
        path: [],
        label: 'EndConverAbort',
        value: 'after',
      },
    });
    await resourceRequest(service, 'saveDocument', { documentId: 'dialog-json' });
    const saved = JSON.parse(fs.readFileSync(sourcePath, 'utf8'));
    assert.equal(saved.__data_type, 'DLG ');
    assert.equal(saved.EndConverAbort.value, 'after');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native binding reports a real NSS source diagnostic', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const response = await languageRequest(binding, 'checkNss', {
    paths: [path.join(fixtureDirectory, 'broken.nss')],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  });

  assert.equal(response.summary.failed, 1);
  assert.equal(response.diagnostics.length, 1);
  assert.equal(response.diagnostics[0].code, -622);
  assert.equal(response.diagnostics[0].range.start.line, 2);
  assert.equal(response.diagnostics[0].range.start.character, 16);
});

test('native language failures remain structured across async and lifecycle methods', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-errors-'));
  const sourcePath = path.join(root, 'main.nss');
  fs.writeFileSync(sourcePath, 'void Main() {}\n');
  const service = new binding.LanguageService();
  try {
    const opened = service.openDocument(sourcePath, 2, Buffer.from('void Main() {}\n'));
    assert.equal(opened.succeeded, true);
    assert.equal(opened.error, undefined);

    const stale = service.updateDocument(sourcePath, 1, Buffer.from('void Older() {}\n'));
    assert.equal(stale.succeeded, false);
    assert.equal(stale.error?.code, 'language.document.staleVersion');
    assert.equal(stale.error?.path, fs.realpathSync(sourcePath));

    const invalid = await service.checkNss({
      paths: [sourcePath],
      noEntrypointCheck: false,
      includeDirectories: [],
      optimization: 'O0',
      optimizationFlags: [],
      maxIncludeDepth: 0,
      maxDiagnosticsPerInput: 100,
      recurse: false,
      followSymlinks: false,
      language: 'en',
      loadOvr: false,
    }, new AbortController().signal);
    assert.equal(invalid.value, undefined);
    assert.equal(typeof invalid.error?.code, 'string');
    assert.match(invalid.error?.message || '', /include depth/u);
  } finally {
    service.closeDocument(sourcePath);
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('compiler worker keeps the native check off the extension thread', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const worker = new Worker(path.resolve(__dirname, '..', 'dist', 'src', 'compiler-worker.js'), {
    workerData: {
      bindingPath,
      method: 'checkNss',
      request: {
        paths: [path.join(fixtureDirectory, 'broken.nss')],
        noEntrypointCheck: false,
        langspec: path.join(fixtureDirectory, 'nwscript.nss'),
        includeDirectories: [],
        optimization: 'O0',
        optimizationFlags: [],
        maxIncludeDepth: 64,
        maxDiagnosticsPerInput: 100,
        recurse: false,
        followSymlinks: false,
        language: 'en',
        loadOvr: false,
      },
    },
  });
  const result: {
    readonly error?: unknown;
    readonly response: {
      readonly summary: { readonly failed: number };
      readonly diagnostics: Array<{ readonly code: number }>;
    };
  } = await new Promise((resolve, reject) => {
    worker.once('message', resolve);
    worker.once('error', reject);
  });
  assert.equal(result.error, undefined);
  assert.equal(result.response.summary.failed, 1);
  assert.equal(result.response.diagnostics[0]?.code, -622);
});

test('native parser diagnostic underlines the expression instead of the closing brace', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const response = await languageRequest(binding, 'checkNss', {
    paths: [path.join(fixtureDirectory, 'missing-semicolon.nss')],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  });

  assert.equal(response.diagnostics[0].code, -573);
  assert.deepEqual(response.diagnostics[0].range, {
    start: { line: 2, character: 4 },
    end: { line: 2, character: 7 },
  });
});

test('typed N-API document lifecycle preserves exact Windows-1252 overlay bytes', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-encoding-'));
  const sourcePath = path.join(root, 'encoded.nss');
  fs.writeFileSync(
    path.join(root, 'nwpkg.toml'),
    '[project]\nname = "encoding"\nkind = "include"\n\n[source]\npath = "."\n',
  );
  fs.writeFileSync(sourcePath, '/// disk text\nvoid Encoded() {}\n');
  const overlay = Buffer.concat([
    Buffer.from('/// caf', 'ascii'),
    Buffer.from([0xe9]),
    Buffer.from(' from Windows-1252\nvoid Encoded() {}\n', 'ascii'),
  ]);
  const service = languageService(binding);
  const packageConfiguration = testPackageConfiguration({
    source_path: sourcePath,
    project_root: root,
  });
  const signal = new AbortController().signal;
  try {
    service.addWorkspace(root);
    service.openDocument(sourcePath, 1, overlay);
    const definitions = await service.findDefinitions({
      package: packageConfiguration,
      symbol: 'Encoded',
    }, signal);
    assert.equal(definitions.length, 1);
    assert.match(definitions[0]?.documentation || '', /café from Windows-1252/u);
    assert.doesNotMatch(definitions[0]?.documentation || '', /cafÃ©/u);
  } finally {
    service.closeDocument(sourcePath);
    service.releasePackage(root);
    service.removeWorkspace(root);
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native EOF diagnostic underlines visible source instead of a following blank line', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const response = await languageRequest(binding, 'checkNss', {
    paths: [path.join(fixtureDirectory, 'eof-error.nss')],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  });

  assert.equal(response.summary.failed, 1);
  assert.deepEqual(response.diagnostics[0].range, {
    start: { line: 0, character: 9 },
    end: { line: 0, character: 10 },
  });
});

test('native event catalog exposes authoritative module-load editor metadata', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const catalog = await languageRequest(binding, 'listEventCatalog');
  const moduleLoad = required(
    catalog.find((event: { readonly subject: string; readonly phase: string }) =>
      event.subject === 'module_load' && event.phase === 'before'),
    'module_load event metadata is missing',
  );
  assert.equal(moduleLoad.name, 'module.load');
  assert.equal(moduleLoad.phase, 'before');
  assert.equal(moduleLoad.skippable, false);
  assert.equal(moduleLoad.control, 'observe only');
  assert.equal(moduleLoad.resultKind, 'none');
});

test('native event analysis owns relationship completion, navigation, and Outline metadata', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-events-'));
  const sourcePath = path.join(root, 'events.nss');
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const includeRoot = path.join(repositoryRoot, 'include', 'nwnrs');
  const completeSource = '/// Validates an object use.\n'
    + '#[nwnrs::event::object_use::before]\n'
    + 'void ValidateUse(json jEvent) {}\n'
    + '#[nwnrs::event::object_use::before(after(ValidateUse))]\n'
    + 'void HandleUse(json jEvent) {}\n';
  const incompleteSource = completeSource.replace('after(ValidateUse)', 'after(Val)');
  fs.writeFileSync(
    path.join(root, 'nwpkg.toml'),
    '[project]\nname = "events"\nkind = "mod"\n\n'
      + '[source]\npath = "."\n\n'
      + `[dependencies]\nnwnrs = { path = ${JSON.stringify(includeRoot)} }\n`,
  );
  fs.writeFileSync(sourcePath, completeSource);
  try {
    const completionOffset = incompleteSource.indexOf('after(Val)') + 'after(Val'.length;
    const completions = await languageRequest(binding, 'eventCompletionsAt', {
      source_path: sourcePath,
      project_root: root,
      overlays: [{ path: sourcePath, contents: incompleteSource }],
      offset: completionOffset,
    });
    assert.equal(completions.kind, 'target');
    assert.equal(completions.prefix, 'Val');
    assert.deepEqual(
      completions.items.map((item: { readonly label: string }) => item.label),
      ['ValidateUse'],
    );
    assert.match(completions.items[0].documentation, /Validates an object use/u);
    assert.equal(completions.items[0].definition.path, fs.realpathSync(sourcePath));

    const relationshipOffset = completeSource.indexOf('ValidateUse))') + 2;
    const relationship = await languageRequest(binding, 'resolveEventSymbolAt', {
      source_path: sourcePath,
      project_root: root,
      offset: relationshipOffset,
    });
    assert.equal(relationship.kind, 'handler');
    assert.equal(relationship.definition.name, 'ValidateUse');
    assert.equal(relationship.definition.path, fs.realpathSync(sourcePath));

    const subjectOffset = completeSource.indexOf('object_use') + 2;
    const route = await languageRequest(binding, 'resolveEventSymbolAt', {
      source_path: sourcePath,
      project_root: root,
      offset: subjectOffset,
    });
    assert.equal(route.kind, 'route');
    assert.equal(route.subject, 'object_use');
    assert.equal(route.phase, 'before');

    const symbols: NativeDocumentSymbol[] = await languageRequest(
      binding,
      'listDocumentSymbols',
      { source_path: sourcePath, project_root: root },
    );
    const handler = required(
      symbols.find((symbol) => symbol.name === 'HandleUse'),
      'event handler is missing from Outline',
    );
    assert.equal(
      handler.detail,
      'event: object_use::before(after(ValidateUse)) · void HandleUse(json jEvent)',
    );
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native project check reports canonical event-macro errors at the attribute route', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const projectDirectory = path.join(fixtureDirectory, 'project-invalid-event');
  const eventPath = path.join(projectDirectory, 'events.nss');
  const response = await languageRequest(binding, 'checkNss', {
    paths: [eventPath],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  });

  assert.equal(response.summary.failed, 1);
  assert.equal(response.diagnostics.length, 1);
  assert.equal(response.diagnostics[0].path, eventPath);
  assert.match(
    response.diagnostics[0].message,
    /unsupported nwnrs event subject/u,
  );
  assert.equal(response.diagnostics[0].range.start.line, 0);
  assert.equal(response.diagnostics[0].range.start.character, 16);
  assert.equal(response.diagnostics[0].range.end.character, 32);
});

test('native definition lookup follows the real local nwpkg include dependency', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const response = await languageRequest(binding, 'findDefinitions', {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
    symbol: 'NWNRS_Log',
  });

  assert.ok(response.length >= 2);
  assert.equal(response[0].isImplementation, true);
  assert.equal(
    response[0].path,
    path.join(repositoryRoot, 'include', 'nwnrs', 'nwnrs.nss'),
  );
  assert.equal(response[0].name, 'NWNRS_Log');
  assert.match(
    response[0].documentation,
    /Sends a message through the runtime's structured tracing pipeline/u,
  );
});

test('native completion and signature help use the current compiler recovery snapshot', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-completion-'));
  const sourcePath = path.join(root, 'main.nss');
  const langspec = path.join(root, 'nwscript.nss');
  fs.writeFileSync(
    sourcePath,
    'void Helper(int first, string second) {}\n'
      + 'void main() {\n'
      + '    int localValue;\n'
      + '    MissingSymbol();\n'
      + '    Helper(localValue, "ok");\n'
      + '}\n',
  );
  fs.writeFileSync(
    langspec,
    '#define ENGINE_NUM_STRUCTURES 0\nvoid PrintString(string sMessage);\n',
  );
  const request = {
    source_path: sourcePath,
    project_root: root,
    langspec,
  };
  try {
    const completions = await languageRequest(binding, 'completeNwscript', {
      ...request,
      line: 4,
      column: 16,
    });
    const local = required(
      completions.find((completion: { readonly label: string }) =>
        completion.label === 'localValue'),
      'local completion',
    );
    assert.equal(local.kind, 'variable');
    assert.deepEqual(local.replacement, {
      start: { line: 4, character: 11 },
      end: { line: 4, character: 16 },
    });

    const signature = await languageRequest(binding, 'signatureHelp', {
      ...request,
      line: 4,
      column: 23,
    });
    assert.equal(signature.activeParameter, 1);
    assert.equal(
      signature.signatures[0]?.label,
      'void Helper(int first, string second)',
    );
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native definition lookup prefers an editable workspace langspec override', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const moduleRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-game-langspec-'));
  const sourcePath = path.join(moduleRoot, 'main.nss');
  fs.writeFileSync(sourcePath, 'void main() { ActionMoveToLocation(GetLocation(OBJECT_SELF)); }\n');
  fs.writeFileSync(
    path.join(moduleRoot, 'nwscript.nss'),
    '// This test file must never shadow the packed game asset.\n'
      + 'void ActionMoveToLocation(string fakeParameter);\n',
  );
  const request = {
    source_path: sourcePath,
    project_root: moduleRoot,
    symbol: 'ActionMoveToLocation',
  };
  try {
    const response = await languageRequest(binding, 'findDefinitions', request);
    assert.equal(response.length, 1);
    assert.equal(response[0].kind, 'builtinFunction');
    assert.equal(response[0].path, fs.realpathSync(path.join(moduleRoot, 'nwscript.nss')));
    assert.equal(response[0].uri, undefined);
    assert.equal(response[0].resource, undefined);
    assert.match(response[0].signature, /^void ActionMoveToLocation\(string fakeParameter/u);
    assert.match(response[0].documentation, /test file must never shadow/u);
  } finally {
    fs.rmSync(moduleRoot, { recursive: true, force: true });
  }
});

test('native definition lookup exposes the packed game fallback read-only', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const request = {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
    symbol: 'ActionMoveToLocation',
  };
  const response = await languageRequest(binding, 'findDefinitions', request);
  if (response.length === 0) {
    context.skip('Neverwinter Nights installation was not discovered');
    return;
  }

  assert.equal(response.length, 1);
  assert.equal(response[0].path, 'nwscript.nss');
  assert.equal(response[0].resource, 'nwscript');
  assert.match(response[0].uri, /^nwnrs-game:\/[0-9a-f]+\/nwscript\.nss$/u);
  assert.match(response[0].signature, /^void ActionMoveToLocation\(location/u);
  assert.match(response[0].documentation, /The action subject will move to lDestination/u);

  const source = await languageRequest(binding, 'readVirtualSource', {
    ...request,
    resource: response[0].resource,
  });
  assert.equal(source.uri, response[0].uri);
  const sourceText = new TextDecoder('windows-1252').decode(source.contents);
  assert.match(sourceText, /void ActionMoveToLocation\(location/u);
  assert.ok(source.contents.length > 500_000);
});

test('native symbol lookup exposes strong enums, variants, and compatibility aliases', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const baseRequest = {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
  };

  const enumType = await languageRequest(binding, 'findDefinitions', {
    ...baseRequest,
    symbol: 'NwnrsLogLevel',
  });
  assert.equal(enumType[0].kind, 'enum');
  assert.match(enumType[0].documentation, /Severity used by the runtime/u);

  const variant = await languageRequest(binding, 'findDefinitions', {
    ...baseRequest,
    symbol: 'Info',
  });
  assert.equal(variant[0].kind, 'enumVariant');
  assert.match(variant[0].signature, /^NwnrsLogLevel::Info/u);

  const alias = await languageRequest(binding, 'findDefinitions', {
    ...baseRequest,
    symbol: 'NWNRS_LOG_LEVEL_INFO',
  });
  assert.equal(alias[0].kind, 'constant');
  assert.match(alias[0].signature, /NwnrsLogLevel::Info/u);
});

test('native document Outline is hierarchical, overlay-aware, and source-only', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-outline-'));
  const sourcePath = path.join(root, 'outline.nss');
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const includeRoot = path.join(repositoryRoot, 'include', 'nwnrs');
  fs.writeFileSync(
    path.join(root, 'nwpkg.toml'),
    '[project]\nname = "outline"\nkind = "mod"\n\n'
      + '[source]\npath = "."\n\n'
      + `[dependencies]\nnwnrs = { path = ${JSON.stringify(includeRoot)} }\n`,
  );
  fs.writeFileSync(path.join(root, 'included.nss'), 'void IncludedOnly() {}\n');
  fs.writeFileSync(sourcePath, 'void SavedOnly() {}\n');
  const contents = '#define FEATURE 1\n'
    + '#include "included"\n'
    + 'struct Stats { int score; string label; };\n'
    + 'enum Mode : int { #[default] Ready = 0, Running };\n'
    + 'type CurrentMode = Mode;\n'
    + 'macro_rules! make { ($name:ident) => { void $name() {} }; }\n'
    + 'make!(Generated)\n'
    + 'int Counter = 1;\n'
    + '#[nwnrs::event::module_load::before]\n'
    + 'void Authored(json jEvent) {}\n';
  try {
    const symbols: NativeDocumentSymbol[] = await languageRequest(binding, 'listDocumentSymbols', {
      source_path: sourcePath,
      project_root: root,
      overlays: [{ path: sourcePath, contents }],
    });
    assert.deepEqual(
      symbols.map((symbol) => symbol.name),
      ['FEATURE', 'Stats', 'Mode', 'CurrentMode', 'make', 'Counter', 'Authored'],
    );
    assert.equal(symbols.some((symbol) => symbol.name === 'SavedOnly'), false);
    assert.equal(symbols.some((symbol) => symbol.name === 'IncludedOnly'), false);
    assert.equal(symbols.some((symbol) => symbol.name === 'Generated'), false);

    const structure = required(
      symbols.find((symbol) => symbol.name === 'Stats'),
      'Stats outline symbol is missing',
    );
    assert.equal(structure.kind, 'struct');
    assert.deepEqual(structure.children.map((child) => child.name), ['score', 'label']);
    assert.deepEqual(structure.children.map((child) => child.kind), ['field', 'field']);

    const enumeration = required(
      symbols.find((symbol) => symbol.name === 'Mode'),
      'Mode outline symbol is missing',
    );
    assert.equal(enumeration.kind, 'enum');
    assert.deepEqual(enumeration.children.map((child) => child.name), ['Ready', 'Running']);

    const functionSymbol = required(
      symbols.find((symbol) => symbol.name === 'Authored'),
      'Authored outline symbol is missing',
    );
    assert.equal(functionSymbol.kind, 'function');
    assert.equal(functionSymbol.detail, 'event: module_load::before · void Authored(json jEvent)');
    assert.equal(functionSymbol.range.start.line, 8);
    assert.deepEqual(functionSymbol.selection, {
      start: { line: 9, character: 5 },
      end: { line: 9, character: 13 },
    });

    const semantic = await languageRequest(binding, 'analyzeDocument', {
      source_path: sourcePath,
      project_root: root,
      overlays: [{ path: sourcePath, contents }],
    });
    const eventToken = required(
      semantic.tokens.find((token: { readonly kind: string }) => token.kind === 'event'),
      'event subject semantic token is missing',
    );
    assert.deepEqual(eventToken.range, {
      start: { line: 8, character: 16 },
      end: { line: 8, character: 27 },
    });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native document Outline indexes a packed game script read-only', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const request = {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
    symbol: 'ActionMoveToLocation',
  };
  const definitions = await languageRequest(binding, 'findDefinitions', request);
  if (definitions.length === 0 || !definitions[0].resource) {
    context.skip('Neverwinter Nights installation was not discovered');
    return;
  }
  const symbols: NativeDocumentSymbol[] = await languageRequest(binding, 'listDocumentSymbols', {
    source_path: definitions[0].resource,
    project_root: moduleRoot,
  });
  const action = required(
    symbols.find((symbol) => symbol.name === 'ActionMoveToLocation'),
    'ActionMoveToLocation outline symbol is missing',
  );
  assert.equal(action.kind, 'function');
  assert.match(action.detail, /^void ActionMoveToLocation\(location/u);
});

test('native compiler accepts the real nwnrs include with strong enums', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-real-include-'));
  const source = path.join(root, 'main.nss');
  fs.writeFileSync(
    source,
    '#include "nwnrs"\nvoid main() { NWNRS_Log("ready", NwnrsLogLevel::Info); }\n',
  );
  try {
    const response = await languageRequest(binding, 'checkNss', {
      paths: [source],
      include_dirs: [path.join(repositoryRoot, 'include', 'nwnrs')],
    });

    assert.equal(response.summary.failed, 0, JSON.stringify(response.diagnostics));
    assert.equal(response.summary.compiled, 1);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native compiler checks dirty overlays and reports more than the first error', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const sourcePath = path.join(fixtureDirectory, 'broken.nss');
  const response = await languageRequest(binding, 'checkNss', {
    paths: [sourcePath],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
    overlays: [{
      path: sourcePath,
      contents: 'void main()\n{\n    MissingOne;\n    MissingTwo;\n}\n',
    }],
  });

  assert.equal(response.summary.failed, 1);
  assert.equal(response.diagnostics.length, 2);
  assert.deepEqual(
    response.diagnostics.map((diagnostic: NativeCheckDiagnostic) => diagnostic.range.start.line),
    [2, 3],
  );
});

test('native workspace helpers deduplicate dependencies and return watch roots', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const includeRoot = path.join(repositoryRoot, 'include', 'nwnrs');
  const deduplicated = await languageRequest(binding, 'deduplicateProjectRoots', {
    roots: [moduleRoot, includeRoot],
  });
  const watched = await languageRequest(binding, 'resolveWatchRoots', {
    roots: [moduleRoot],
  });

  assert.deepEqual(deduplicated, [fs.realpathSync.native(moduleRoot)]);
  assert.ok(watched.includes(fs.realpathSync.native(moduleRoot)));
  assert.ok(watched.includes(fs.realpathSync.native(includeRoot)));
});

test('native project index includes package sources and editable dependencies', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const response = await languageRequest(binding, 'indexProject', {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
  });

  assert.deepEqual(response.warnings, []);
  assert.ok(response.documents.some((document: { readonly path: string }) =>
    document.path === path.join(moduleRoot, 'debug.nss')));
  assert.ok(response.documents.some((document: { readonly path: string }) =>
    document.path === path.join(repositoryRoot, 'include', 'nwnrs', 'nwnrs.nss')));
});

test('native references cross sibling scripts and preserve exact local bindings', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-references-'));
  const shared = path.join(root, 'shared.nss');
  const first = path.join(root, 'first.nss');
  const second = path.join(root, 'second.nss');
  fs.writeFileSync(shared, 'void SharedWork() {}\n');
  fs.writeFileSync(first, '#include "shared"\nvoid First() { SharedWork(); }\n');
  fs.writeFileSync(second, '#include "shared"\nvoid Second() { SharedWork(); }\n');
  try {
    const references: NativeReference[] = await languageRequest(binding, 'findReferences', {
      source_path: first,
      project_root: root,
      line: 2,
      column: 16,
    });
    assert.equal(references.length, 3);
    assert.equal(references.filter((reference) => reference.isDeclaration).length, 1);
    assert.deepEqual(
      new Set(references.filter((reference) => reference.container).map((reference) => reference.container)),
      new Set(['First', 'Second']),
    );
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native include candidates and source resolution use compiler precedence', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-includes-'));
  const main = path.join(root, 'main.nss');
  const helper = path.join(root, 'helper.nss');
  fs.writeFileSync(main, 'void main() { MissingHelper(); }\n');
  fs.writeFileSync(helper, 'void MissingHelper() {}\n');
  try {
    const request = {
      source_path: main,
      project_root: root,
      symbol: 'MissingHelper',
    };
    const candidates = await languageRequest(binding, 'findIncludeCandidates', request);
    assert.equal(candidates.length, 1);
    assert.equal(candidates[0].includeName, 'helper');

    const resolved = await languageRequest(binding, 'resolveSource', {
      ...request,
      resource: 'helper',
    });
    assert.equal(resolved.path, fs.realpathSync(helper));
    assert.equal(resolved.uri, undefined);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native manifest tooling validates syntax, paths, kinds, and dependencies', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-nwpkg-'));
  const manifestPath = path.join(root, 'nwpkg.toml');
  try {
    const dependencyRoot = path.join(root, 'dependency');
    fs.mkdirSync(dependencyRoot);
    fs.writeFileSync(
      path.join(dependencyRoot, 'nwpkg.toml'),
      '[project]\nname = "dependency"\nkind = "include"\n\n[source]\npath = "."\n',
    );
    fs.writeFileSync(path.join(dependencyRoot, 'dependency.nss'), 'int Dependency();\n');

    const valid = await languageRequest(binding, 'checkNwpkg', {
      path: manifestPath,
      contents: [
        '[project]',
        'name = "fixture"',
        'kind = "include"',
        '',
        '[source]',
        'path = "."',
        '',
        '[dependencies]',
        'dependency = { path = "dependency" }',
        '',
      ].join('\n'),
    });
    assert.deepEqual(valid.diagnostics, []);

    const invalid = await languageRequest(binding, 'checkNwpkg', {
      path: manifestPath,
      contents: '[project]\nname = "fixture"\nkind = "unknown"\n\n[source]\npath = "missing"\n',
    });
    assert.ok(invalid.diagnostics.length >= 1);
    assert.ok(invalid.diagnostics.every(
      (diagnostic: NativeCheckDiagnostic) => diagnostic.range.start.line >= 0,
    ));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('typed native dependency service locks local packages and exposes exact sources', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-dependencies-'));
  const app = path.join(root, 'app');
  const shared = path.join(root, 'shared');
  const cachePath = path.join(root, 'cache');
  fs.mkdirSync(path.join(app, 'src'), { recursive: true });
  fs.mkdirSync(path.join(shared, 'src'), { recursive: true });
  const manifestPath = path.join(app, 'nwpkg.toml');
  fs.writeFileSync(
    manifestPath,
    '[project]\nname = "app"\nkind = "include"\n\n[source]\npath = "src"\n\n'
      + '[dependencies]\nshared = { path = "../shared" }\n',
  );
  fs.writeFileSync(
    path.join(shared, 'nwpkg.toml'),
    '[project]\nname = "shared"\nkind = "include"\n\n[source]\npath = "src"\n',
  );
  const sharedSource = Buffer.from('int SharedValue = 42;\n', 'latin1');
  fs.writeFileSync(path.join(shared, 'src', 'shared.nss'), sharedSource);
  const service = new binding.LanguageService();
  const request = { manifestPath, cachePath };
  try {
    const statusResult = await service.fetchPackageDependencies(
      request,
      new AbortController().signal,
    );
    assert.equal(statusResult.error, undefined);
    const status = required(statusResult.value, 'dependency status result');
    assert.equal(status.length, 1);
    assert.equal(status[0]?.route, 'shared');
    assert.equal(status[0]?.sourceKind, 'path');
    assert.equal(status[0]?.state, 'live');
    assert.ok(fs.existsSync(path.join(app, 'nwpkg.lock')));

    const sourcesResult = await service.listPackageDependencySources(
      { ...request, route: 'shared' },
      new AbortController().signal,
    );
    assert.equal(sourcesResult.error, undefined);
    const sources = required(sourcesResult.value, 'dependency sources result');
    const source = required(
      sources.find((candidate: { readonly relativePath: string }) =>
        candidate.relativePath === 'src/shared.nss'),
      'shared dependency source',
    );
    const contentsResult = service.readPackageDependencySource({
      ...request,
      route: 'shared',
      relativePath: source.relativePath,
    });
    assert.equal(contentsResult.error, undefined);
    const contents = required(contentsResult.value, 'dependency source contents result');
    assert.equal(contents.encoding, 'windows1252');
    assert.deepEqual(Buffer.from(contents.contents), sharedSource);

    const dirtyManifest = Buffer.from(`${fs.readFileSync(manifestPath, 'utf8')}\n# dirty\n`);
    service.openDocument(manifestPath, 1, dirtyManifest);
    const dirtyResult = await service.fetchPackageDependencies(
      request,
      new AbortController().signal,
    );
    assert.equal(dirtyResult.value, undefined);
    assert.equal(dirtyResult.error?.code, 'nwpkg.manifest.unsaved');
    assert.equal(dirtyResult.error?.path, manifestPath);
    service.closeDocument(manifestPath);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('persistent worker isolates and invalidates indexed package sessions', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-worker-session-'));
  const source = path.join(root, 'main.nss');
  fs.writeFileSync(
    path.join(root, 'nwpkg.toml'),
    '[project]\nname = "worker-session"\nkind = "include"\n\n[source]\npath = "."\n',
  );
  fs.writeFileSync(source, 'void Original() {}\n');
  const worker = new Worker(path.resolve(__dirname, '..', 'dist', 'src', 'compiler-worker.js'), {
    workerData: { bindingPath, persistent: true },
  });
  let requestId = 0;
  interface WorkerResponse {
    readonly type: string;
    readonly id: number;
    readonly error?: string;
    readonly response?: unknown;
  }
  const request = (
    method: string,
    payload: unknown,
    sessionKey = root,
  ): Promise<WorkerIndexResponse> => new Promise((resolve, reject) => {
    const id = ++requestId;
    const onMessage = (message: WorkerResponse) => {
      if (message.type !== 'response' || message.id !== id) {
        return;
      }
      worker.off('message', onMessage);
      if (message.error) {
        reject(new Error(message.error));
      } else if (!isWorkerIndexResponse(message.response)) {
        reject(new Error('Worker returned a malformed project index response'));
      } else {
        resolve(message.response);
      }
    };
    worker.on('message', onMessage);
    worker.postMessage({ type: 'request', id, method, request: payload, sessionKey });
  });
  const invalidate = (): Promise<void> => new Promise((resolve, reject) => {
    const id = ++requestId;
    const onMessage = (message: WorkerResponse) => {
      if (message.type !== 'response' || message.id !== id) return;
      worker.off('message', onMessage);
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve();
    };
    worker.on('message', onMessage);
    worker.postMessage({
      type: 'request',
      id,
      method: 'invalidate',
      request: { path: source },
      sessionKey: root,
    });
  });
  const payload = {
    sourcePath: source,
    projectRoot: root,
    includeDirectories: [],
    maxIncludeDepth: 64,
    language: 'en',
    loadOvr: false,
  };
  try {
    const initial = await request('indexProject', payload);
    assert.equal(initial.documents[0]?.symbols[0]?.name, 'Original');
    fs.writeFileSync(source, 'void Updated() {}\n');
    const revalidated = await request('indexProject', payload);
    assert.equal(revalidated.documents[0]?.symbols[0]?.name, 'Updated');
    await invalidate();
    const updated = await request('indexProject', payload);
    assert.equal(updated.documents[0]?.symbols[0]?.name, 'Updated');
  } finally {
    await worker.terminate();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('persistent event analysis refreshes from package fingerprints without stale plans', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-event-session-'));
  const sourcePath = path.join(root, 'events.nss');
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const includeRoot = path.join(repositoryRoot, 'include', 'nwnrs');
  fs.writeFileSync(
    path.join(root, 'nwpkg.toml'),
    '[project]\nname = "events"\nkind = "mod"\n\n'
      + '[source]\npath = "."\n\n'
      + `[dependencies]\nnwnrs = { path = ${JSON.stringify(includeRoot)} }\n`,
  );
  fs.writeFileSync(
    sourcePath,
    '#[nwnrs::event::object_use::before]\nvoid Handle(json jEvent) {}\n',
  );
  const service = languageService(binding);
  service.addWorkspace(root);
  const packageConfiguration = testPackageConfiguration({
    source_path: sourcePath,
    project_root: root,
  });
  const signal = new AbortController().signal;
  try {
    const initial: NativeDocumentSymbol[] = await service.listDocumentSymbols(
      packageConfiguration,
      signal,
    );
    assert.match(initial[0]?.detail || '', /object_use::before/u);

    fs.writeFileSync(
      sourcePath,
      '#[nwnrs::event::object_lock::before]\nvoid Handle(json jEvent) {}\n',
    );
    service.invalidate(sourcePath);
    const refreshed: NativeDocumentSymbol[] = await service.listDocumentSymbols(
      packageConfiguration,
      signal,
    );
    assert.match(refreshed[0]?.detail || '', /object_lock::before/u);
    assert.doesNotMatch(refreshed[0]?.detail || '', /object_use::before/u);

    const partial = '#[nwnrs::event::object_lock::before]\n'
      + 'void Validate(json jEvent) {}\n'
      + '#[nwnrs::event::object_lock::before(after(Val))]\n'
      + 'void Handle(json jEvent) {}\n';
    const offset = partial.indexOf('after(Val)') + 'after(Val'.length;
    service.openDocument(sourcePath, 1, Buffer.from(partial, 'latin1'));
    const completions = await service.eventCompletionsAt({
      package: packageConfiguration,
      offset,
    }, signal);
    assert.deepEqual(
      completions.items.map((item: { readonly label: string }) => item.label),
      ['Validate'],
    );
    const recovered: NativeDocumentSymbol[] = await service.listDocumentSymbols(
      packageConfiguration,
      signal,
    );
    assert.match(
      required(
        recovered.find((symbol) => symbol.name === 'Handle'),
        'recovered handler is missing',
      ).detail || '',
      /event: object_lock::before/u,
    );
  } finally {
    service.closeDocument(sourcePath);
    service.releasePackage(root);
    service.removeWorkspace(root);
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('language worker restarts cleanly and accepts requests afterward', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const client = new LanguageWorkerClient(
    path.resolve(__dirname, '..', 'dist', 'src', 'compiler-worker.js'),
    bindingPath,
    { appendLine() {} },
  );
  try {
    const before = await client.request('deduplicateProjectRoots', { roots: [] });
    assert.deepEqual(before, []);
    await client.restart();
    const after = await client.request('deduplicateProjectRoots', { roots: [] });
    assert.deepEqual(after, []);
  } finally {
    client.dispose();
  }
});

test('viewer worker inspects nwpkg manifests and lazily catalogs winning resources', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const manifestPath = path.join(moduleRoot, 'nwpkg.toml');
  const client = new ViewerWorkerClient(
    path.resolve(__dirname, '..', 'dist', 'src', 'viewer-worker.js'),
    bindingPath,
    { appendLine() {} },
  );
  try {
    const packageInfo = await client.inspectPackage(manifestPath);
    assert.equal(packageInfo.name, 'nwnrs');
    assert.equal(packageInfo.kind, 'mod');
    assert.equal(path.resolve(packageInfo.root), moduleRoot);
    assert.equal(path.resolve(packageInfo.sourcePath), moduleRoot);
    assert.deepEqual(
      packageInfo.dependencies.map(({ name }: { readonly name: string }) => name),
      ['nwnrs'],
    );
    assert.ok(packageInfo.resourcePaths.some(
      (resourcePath: string) => path.resolve(resourcePath)
        === path.join(repositoryRoot, 'include', 'nwnrs'),
    ));
    const packageSource = await client.inspectPackageSource(manifestPath);
    assert.deepEqual(
      packageSource.areas.map(({ resref }: NativePackageSourceArea) => resref),
      ['start'],
    );
    assert.deepEqual(packageSource.areas[0]?.missing, []);
    assert.deepEqual(packageSource.dialogs, []);
    assert.ok(packageSource.code.some(
      ({ relativePath }: NativePackageSourceFile) => relativePath === 'debug.nss',
    ));

    const viewer = typedViewerLoadRequest({
      sessionKey: manifestPath,
      path: path.join(moduleRoot, '.nwnrs-resource-catalog'),
      projectRoot: moduleRoot,
      language: 'english',
      loadOvr: false,
      archives: [],
      includeProjectResources: true,
    });
    let layers: NativeResourceCatalogItem[];
    try {
      layers = (await client.listResources({ viewer, stage: 'layers' })).items;
    } catch (error) {
      if (/installation|root|language directory/iu.test(String(error))) {
        context.skip('Neverwinter Nights installation was not discovered');
        return;
      }
      throw error;
    }
    assert.ok(layers.some(({ layer, count }: NativeResourceCatalogItem) => layer === 'Workspace' && count > 0));
    assert.ok(layers.some(({ layer, count }: NativeResourceCatalogItem) => layer === 'Package Dependencies' && count > 0));
    assert.ok(layers.some(({ layer, count }: NativeResourceCatalogItem) => layer === 'Vanilla' && count > 1000));

    const families = (await client.listResources({
      viewer,
      stage: 'families',
      layer: 'Vanilla',
    })).items;
    assert.ok(families.some(({ family, count }: NativeResourceCatalogItem) => family === 'Models' && count > 0));
    const types = (await client.listResources({
      viewer,
      stage: 'types',
      layer: 'Vanilla',
      family: 'Models',
    })).items;
    assert.ok(types.some(({ extension, count }: NativeResourceCatalogItem) => extension === 'mdl' && count > 0));
    const names = (await client.listResources({
      viewer,
      stage: 'names',
      layer: 'Vanilla',
      family: 'Models',
      extension: 'mdl',
      prefix: 'c_bodak',
    })).items;
    assert.ok(names.some(({ resource }: NativeResourceCatalogItem) => resource === 'c_bodak.mdl'));

    const vanillaViewer = {
      ...viewer,
      sessionKey: `${manifestPath}:vanilla-only`,
      includeProjectResources: false,
    };
    const vanillaOnly = (await client.listResources({
      viewer: vanillaViewer,
      stage: 'layers',
    })).items;
    assert.deepEqual(
      vanillaOnly.map(({ layer }: NativeResourceCatalogItem) => layer),
      ['Vanilla'],
    );
  } finally {
    client.dispose();
  }
});

test('persistent viewer uses authoritative resources for authored areas and standalone assets', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const service = new binding.ViewerService();
  const request = {
    session_key: path.join(moduleRoot, 'nwpkg.toml'),
    path: path.join(moduleRoot, '.nwnrs-resource-catalog'),
    project_root: moduleRoot,
    area: null,
    root: null,
    user: null,
    language: 'english',
    load_ovr: false,
    archives: [],
    include_project_resources: true,
  };
  let authoredPacket: Buffer;
  try {
    authoredPacket = Buffer.from(await service.loadScene(typedViewerLoadRequest({
      ...request,
      path: path.join(moduleRoot, 'start.are'),
      authored_area: {
        resref: 'start',
        are: path.join(moduleRoot, 'start.are.json'),
        git: path.join(moduleRoot, 'start.git.json'),
        gic: path.join(moduleRoot, 'start.gic.json'),
      },
    })));
  } catch (error) {
    if (/installation|root|language directory/iu.test(String(error))) {
      context.skip('Neverwinter Nights installation was not discovered');
      return;
    }
    throw error;
  }

  const authoredManifestLength = authoredPacket.readUInt32LE(8);
  const authoredManifest: ScenePacketManifest = JSON.parse(
    authoredPacket.subarray(12, 12 + authoredManifestLength).toString('utf8'),
  );
  assert.equal(authoredManifest.source, 'area');
  assert.ok(authoredManifest.instances.some((entry) => entry.kind === 'tile'));
  assert.equal(authoredManifest.areaObjects.length, 6);
  assert.ok(authoredManifest.areaObjects.every((object) => object.kind === 'placeable'));
  assert.ok(authoredManifest.areaObjects.every((object) => authoredManifest.instances
    .some((instance) => instance.objectKey === object.key)));
  assert.ok(authoredManifest.instances
    .filter((instance) => instance.kind === 'collision' && instance.objectKey)
    .every((instance) => authoredManifest.areaObjects
      .some((object) => object.key === instance.objectKey)));
  assert.ok(authoredManifest.instances
    .filter((instance) => instance.objectKey && instance.model != null && instance.kind !== 'collision')
    .every((instance) => typeof instance.resource === 'string'
      && /\.mdl$/u.test(instance.resource)));
  assert.ok(authoredManifest.instances
    .filter((instance) => instance.objectKey && instance.kind === 'collision')
    .every((instance) => typeof instance.resource === 'string'
      && /\.(?:dwk|pwk|wok)$/u.test(instance.resource)));
  const selectedObject = authoredManifest.areaObjects[0];
  if (!selectedObject) throw new Error('Authored area has no selectable object');
  const inspectionRequest = {
    sessionKey: path.join(moduleRoot, 'nwpkg.toml'),
    assetKey: authoredManifest.assetKey,
    objectKey: selectedObject.key,
  };
  const inspection: AreaObjectInspection = await service.inspectAreaObject(inspectionRequest);
  assert.equal(inspection.schema, 'nwnrs.area-object-inspection');
  assert.equal(inspection.key, selectedObject.key);
  assert.equal(inspection.kind, 'placeable');
  const inspectionSource = required(inspection.sources[0], 'Instance inspection source is missing');
  assert.equal(inspectionSource.layer, 'instance');
  assert.equal(inspectionSource.resource, 'start.git');
  assert.ok(inspectionSource.data.fields.length > 30);
  const effectiveFields = inspection.sections.flatMap((section) => section.fields);
  assert.ok(effectiveFields.length >= inspectionSource.data.fields.length);
  assert.ok(inspectionSource.data.fields.every(
    (sourceField) => effectiveFields.some((field) => field.name === sourceField.name),
  ));
  assert.equal(
    effectiveFields.find((field) => field.name === 'Description')?.display,
    'This is the default nwnrs server module. You are seeing it because the server administrator did not provide another module.',
  );
  assert.equal(effectiveFields.find((field) => field.name === 'Appearance')?.lookup?.resource, 'placeables.2da');
  assert.deepEqual(
    await service.inspectAreaObject(inspectionRequest),
    inspection,
    'the second inspection should return the cached scene-owned payload',
  );
  const mistModel = authoredManifest.models.find((model) => model.name.toLowerCase() === 'tnp_gmist');
  assert.ok(mistModel, 'x3_plc_mist must resolve its installed tnp_gmist model');
  if (!mistModel) throw new Error('Mist model was not resolved');
  const mistEmitters = mistModel.nodes
    .map((node) => node.emitter)
    .filter((emitter): emitter is PacketEmitter => emitter !== null);
  assert.equal(mistEmitters.length, 6);
  const emitterText = (emitter: PacketEmitter, propertyName: string): string | undefined => {
    const value = emitter.properties
    .find(({ name }) => name.toLowerCase() === propertyName)
      ?.values.find(({ kind }) => kind === 'text')?.value;
    return typeof value === 'string' ? value.toLowerCase() : undefined;
  };
  assert.equal(
    mistEmitters.filter((emitter) => emitterText(emitter, 'render') === 'billboard_to_world_z').length,
    1,
  );
  assert.equal(
    mistEmitters.filter((emitter) => emitterText(emitter, 'render') === 'linked').length,
    5,
  );
  assert.ok(mistEmitters.every((emitter) => emitter.xSize === 1000));
  assert.ok(mistEmitters.every((emitter) => emitterText(emitter, 'texture') === 'fxpa_smoke01a'));
  const mistTextureBinding = mistModel.nodeTextures.find(
    ({ role, name }) => role === 'emitter' && name.toLowerCase() === 'fxpa_smoke01a',
  );
  const resolvedMistTexture = required(
    mistTextureBinding,
    'Mist emitter texture must have a node binding',
  );
  if (typeof resolvedMistTexture.texture !== 'number'
      || !Number.isInteger(resolvedMistTexture.texture)) {
    throw new Error('Mist emitter texture must resolve');
  }
  const mistTexture = required(
    authoredManifest.textures[resolvedMistTexture.texture],
    'Resolved mist emitter texture is missing',
  );
  assert.match(
    mistTexture.resource,
    /^fxpa_smoke01a\.(?:dds|tga|plt)$/u,
  );
  assert.deepEqual(authoredManifest.diagnostics.filter((entry) => entry.severity === 'error'), []);

  const modelPacket = Buffer.from(await service.loadScene(typedViewerLoadRequest({
    ...request,
    path: path.join(repositoryRoot, 'c_cat.mdl'),
  })));
  const modelManifestLength = modelPacket.readUInt32LE(8);
  const modelManifest: ScenePacketManifest = JSON.parse(
    modelPacket.subarray(12, 12 + modelManifestLength).toString('utf8'),
  );
  assert.equal(modelManifest.source, 'model');
  assert.equal(modelManifest.environment, 'studio');
  assert.ok(modelManifest.models.length > 0);
  assert.ok(modelManifest.textures.some((entry) => /^c_cat\.(?:dds|tga|plt)$/u.test(entry.resource)));
  assert.ok(modelManifest.models[0]?.resolvedMaterials.every(
    (material) => material.textures.some(
      (texture) => texture.role === 'diffuse' && texture.texture != null,
    ),
  ));
  assert.ok(modelManifest.instances.some((entry) => entry.kind === 'model'));
  assert.ok(modelManifest.assetKey);
  assert.ok(modelManifest.models[0]?.animations.every(
    (animation) => animation.tracksLoaded === false && animation.nodeTracks.length === 0,
  ));
  const animationPacket = Buffer.from(await service.loadAnimation({
    sessionKey: request.session_key,
    assetKey: modelManifest.assetKey,
    modelIndex: 0,
    animationIndex: 0,
  }));
  const animationManifestLength = animationPacket.readUInt32LE(8);
  assert.equal((12 + animationManifestLength) % 4, 0);
  const animationManifest: SceneAnimationPacketManifest = JSON.parse(
    animationPacket.subarray(12, 12 + animationManifestLength).toString('utf8'),
  );
  assert.equal(animationManifest.schema, 'nwnrs.scene.animation');
  assert.equal(animationManifest.assetKey, modelManifest.assetKey);
  assert.equal(animationManifest.animation.tracksLoaded, true);
  assert.ok(animationManifest.animation.nodeTracks.length > 0);
  const texturePacket = Buffer.from(await service.loadTexture({
    sessionKey: request.session_key,
    assetKey: modelManifest.assetKey,
    textureIndex: 0,
    preferCompressed: true,
  }));
  const textureManifestLength = texturePacket.readUInt32LE(8);
  assert.equal((12 + textureManifestLength) % 4, 0);
  const textureManifest: SceneTexturePacketManifest = JSON.parse(
    texturePacket.subarray(12, 12 + textureManifestLength).toString('utf8'),
  );
  assert.equal(textureManifest.schema, 'nwnrs.scene.texture');
  assert.equal(textureManifest.assetKey, modelManifest.assetKey);
  assert.equal(textureManifest.textureIndex, 0);
  if (textureManifest.compression) {
    assert.match(textureManifest.compression, /^dxt[15]$/u);
    assert.ok(textureManifest.mipLevels.length > 0);
    assert.ok(textureManifest.mipLevels.every((mip) => mip.data.byteLength > 0));
    assert.equal(textureManifest.rgba8, null);
  } else {
    assert.ok(required(textureManifest.rgba8, 'RGBA texture payload is missing').byteLength > 0);
  }
  const rgbaPacket = Buffer.from(await service.loadTexture({
    sessionKey: request.session_key,
    assetKey: modelManifest.assetKey,
    textureIndex: 0,
    preferCompressed: false,
  }));
  const rgbaManifestLength = rgbaPacket.readUInt32LE(8);
  const rgbaManifest: SceneTexturePacketManifest = JSON.parse(
    rgbaPacket.subarray(12, 12 + rgbaManifestLength).toString('utf8'),
  );
  assert.equal(rgbaManifest.compression, null);
  assert.ok(required(rgbaManifest.rgba8, 'Requested RGBA payload is missing').byteLength > 0);

  const walkmeshResource = authoredManifest.dependencies.nodes.find(
    (entry) => entry.state === 'resolved' && entry.resource.endsWith('.wok'),
  )?.resource;
  if (!walkmeshResource) {
    throw new Error('Authored area did not expose a resolved tile walkmesh');
  }
  const walkmeshPacket = Buffer.from(await service.loadScene(typedViewerLoadRequest({
    ...request,
    path: path.join(repositoryRoot, walkmeshResource),
  })));
  const walkmeshManifestLength = walkmeshPacket.readUInt32LE(8);
  const walkmeshManifest: ScenePacketManifest = JSON.parse(
    walkmeshPacket.subarray(12, 12 + walkmeshManifestLength).toString('utf8'),
  );
  assert.equal(walkmeshManifest.source, 'walkmesh');
  assert.ok(walkmeshManifest.instances.some((entry) => entry.kind === 'collision'));

  const resolved = await service.resolveResource(typedViewerLoadRequest({
    ...request,
    path: path.join(repositoryRoot, 'skyboxes.2da'),
    archives: [],
  }));
  assert.equal(resolved.resource, 'skyboxes.2da');
  assert.equal(resolved.filePath, undefined);
  assert.match(resolved.origin, /KeyTable/u);

  const overrideRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-viewer-origin-'));
  const overridePath = path.join(overrideRoot, 'c_rat.mdl');
  fs.writeFileSync(overridePath, 'newmodel c_rat\nsetsupermodel c_rat null\nbeginmodelgeom c_rat\nnode dummy c_rat\n parent null\nendnode\nendmodelgeom c_rat\ndonemodel c_rat\n');
  try {
    const override = await service.resolveResource(typedViewerLoadRequest({
      ...request,
      session_key: overrideRoot,
      path: overridePath,
      project_root: overrideRoot,
      archives: [],
      include_project_resources: true,
    }));
    assert.equal(path.resolve(override.filePath), path.resolve(overridePath));
  } finally {
    fs.rmSync(overrideRoot, { recursive: true, force: true });
  }
});

test('every c_bodak animation packet exposes aligned typed tracks', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const service = new binding.ViewerService();
  const request = typedViewerLoadRequest({
    sessionKey: `${repositoryRoot}/nwpkg.toml`,
    path: path.join(repositoryRoot, 'c_bodak.mdl'),
    projectRoot: repositoryRoot,
    language: 'english',
    loadOvr: false,
    archives: [],
  });
  let scenePacket;
  try {
    scenePacket = Buffer.from(await service.loadScene(typedViewerLoadRequest(request)));
  } catch (error) {
    if (/installation|root|language directory/iu.test(String(error))) {
      context.skip('Neverwinter Nights installation was not discovered');
      return;
    }
    throw error;
  }
  const scene = decodeViewerPacket(scenePacket);
  const animations = required(
    scene.manifest.models[0],
    'c_bodak scene has no root model',
  ).animations;
  const walkIndex = animations.findIndex(
    (animation) => animation.name.toLowerCase() === 'walk',
  );
  assert.notEqual(walkIndex, -1, 'c_bodak does not expose its walk animation');

  for (const [animationIndex, catalogAnimation] of animations.entries()) {
    const packet = Buffer.from(await service.loadAnimation({
      sessionKey: request.sessionKey,
      assetKey: scene.manifest.assetKey,
      modelIndex: 0,
      animationIndex,
    }));
    const animation = decodeViewerPacket(packet);
    assert.equal(
      animation.binaryStart % 4,
      0,
      `c_bodak animation ${animationIndex} (${catalogAnimation.name}) has an unaligned payload`,
    );
    assert.equal(animation.manifest.animation.name, catalogAnimation.name);
    assertViewerPacketTypedViewsAreConstructible(
      packet,
      animation.manifest,
      animation.binaryStart,
    );
  }
  assert.equal(
    required(animations[walkIndex], 'Walk animation is missing').name.toLowerCase(),
    'walk',
  );
});

test('viewer worker opens c_bodak KEY dependencies through transferable owned memory', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const client = new ViewerWorkerClient(
    path.resolve(__dirname, '..', 'dist', 'src', 'viewer-worker.js'),
    bindingPath,
    { appendLine() {} },
  );
  const request = typedViewerLoadRequest({
    sessionKey: `${repositoryRoot}/nwpkg.toml`,
    path: path.join(repositoryRoot, 'c_bodak.mdl'),
    projectRoot: repositoryRoot,
    language: 'english',
    loadOvr: false,
    archives: [],
  });
  try {
    let parentPacket;
    try {
      parentPacket = Buffer.from(await client.loadScene(request));
    } catch (error) {
      if (/installation|root|language directory/iu.test(String(error))) {
        context.skip('Neverwinter Nights installation was not discovered');
        return;
      }
      throw error;
    }
    assert.equal(decodeViewerPacket(parentPacket).manifest.name, 'c_bodak');

    const dependencyRequest = {
      ...request,
      path: path.join(repositoryRoot, 'a_ba.mdl'),
    };
    const resolved = await client.resolveResource(dependencyRequest);
    assert.equal(resolved.resource, 'a_ba.mdl');
    assert.equal(resolved.filePath, undefined);
    assert.match(resolved.origin, /KeyTable:.*\.key\(.*\.bif\)/u);

    const contents = await client.readResource(dependencyRequest);
    assert.ok(contents.byteLength > 1024 * 1024, 'a_ba.mdl must exercise a large native buffer');
    const childPacket = Buffer.from(await client.loadScene(dependencyRequest, contents));
    const child = decodeViewerPacket(childPacket);
    assert.equal(child.manifest.name, 'a_ba');
    assert.equal(child.manifest.source, 'model');
    assert.ok(child.manifest.models.length > 0);
  } finally {
    client.dispose();
  }
});
