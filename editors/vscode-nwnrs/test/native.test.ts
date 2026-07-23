'use strict';

import type {
  NativeAreaObject,
  NativeCheckDiagnostic,
  NativeDocumentSymbol,
  NativePackageInfo,
  NativePackageSourceArea,
  NativePackageSourceFile,
  NativeReference,
  NativeResourceCatalogItem,
} from '../src/native-types';
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

interface NativeResourceService {
  execute(method: string, request: string): Promise<string>;
}

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
) {
  return JSON.parse(await service.execute(method, JSON.stringify(request)));
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
    contents: ncs.toString('base64'),
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
    ndb: ndb.toString('base64'),
    sources: { helper: Buffer.from('void helper() { return; }\n').toString('base64') },
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
    assert.deepEqual(Buffer.from(opened.data.rgba, 'base64'), Buffer.from([255, 0, 0, 255]));
    const changed = await resourceRequest(service, 'applyEdit', {
      documentId: 'texture',
      edit: {
        action: 'replaceTexture', width: 1, height: 1,
        rgba: Buffer.from([0, 255, 0, 255]).toString('base64'),
      },
    });
    assert.equal(changed.inverse.action, 'restoreTextureBytes');
    assert.deepEqual(
      Buffer.from(changed.inverse.contents, 'base64'),
      Buffer.concat([header, Buffer.from([0, 0, 255, 255])]),
    );
    const exported = await resourceRequest(service, 'exportDocument', { documentId: 'texture' });
    const bytes = Buffer.from(exported.contents, 'base64');
    assert.deepEqual(bytes.subarray(18, 22), Buffer.from([0, 255, 0, 255]));
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
    assert.deepEqual(Buffer.from(exported.contents, 'base64'), original);
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
}, () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const response = JSON.parse(binding.checkNss(JSON.stringify({
    paths: [path.join(fixtureDirectory, 'broken.nss')],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  })));

  assert.equal(response.summary.failed, 1);
  assert.equal(response.diagnostics.length, 1);
  assert.equal(response.diagnostics[0].code, -622);
  assert.equal(response.diagnostics[0].start_line, 3);
  assert.equal(response.diagnostics[0].start_column, 17);
});

test('compiler worker keeps the native check off the extension thread', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const worker = new Worker(path.resolve(__dirname, '..', 'dist', 'src', 'compiler-worker.js'), {
    workerData: {
      bindingPath,
      request: {
        paths: [path.join(fixtureDirectory, 'broken.nss')],
        langspec: path.join(fixtureDirectory, 'nwscript.nss'),
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
}, () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const response = JSON.parse(binding.checkNss(JSON.stringify({
    paths: [path.join(fixtureDirectory, 'missing-semicolon.nss')],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  })));

  assert.equal(response.diagnostics[0].code, -573);
  assert.equal(response.diagnostics[0].start_line, 3);
  assert.equal(response.diagnostics[0].start_column, 5);
  assert.equal(response.diagnostics[0].end_line, 3);
  assert.equal(response.diagnostics[0].end_column, 8);
});

test('native EOF diagnostic underlines visible source instead of a following blank line', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const response = JSON.parse(binding.checkNss(JSON.stringify({
    paths: [path.join(fixtureDirectory, 'eof-error.nss')],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  })));

  assert.equal(response.summary.failed, 1);
  assert.equal(response.diagnostics[0].start_line, 1);
  assert.equal(response.diagnostics[0].start_column, 10);
  assert.equal(response.diagnostics[0].end_line, 1);
  assert.equal(response.diagnostics[0].end_column, 11);
});

test('native project check reports canonical event-macro errors at the attribute identity', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const projectDirectory = path.join(fixtureDirectory, 'project-invalid-event');
  const eventPath = path.join(projectDirectory, 'events.nss');
  const response = JSON.parse(binding.checkNss(JSON.stringify({
    paths: [eventPath],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
  })));

  assert.equal(response.summary.failed, 1);
  assert.equal(response.diagnostics.length, 1);
  assert.equal(response.diagnostics[0].file, eventPath);
  assert.match(response.diagnostics[0].message, /unsupported nwnrs event identity/u);
  assert.equal(response.diagnostics[0].start_line, 1);
  assert.equal(response.diagnostics[0].start_column, 17);
  assert.equal(response.diagnostics[0].end_column, 33);
});

test('native definition lookup follows the real local nwpkg include dependency', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const response = JSON.parse(binding.findDefinitions(JSON.stringify({
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
    symbol: 'NWNRS_Log',
  })));

  assert.ok(response.length >= 2);
  assert.equal(response[0].is_implementation, true);
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

test('native definition lookup prefers an editable workspace langspec override', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
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
    const response = JSON.parse(binding.findDefinitions(JSON.stringify(request)));
    assert.equal(response.length, 1);
    assert.equal(response[0].kind, 'builtinFunction');
    assert.equal(response[0].path, path.join(moduleRoot, 'nwscript.nss'));
    assert.equal(response[0].uri, null);
    assert.equal(response[0].resource, null);
    assert.match(response[0].signature, /^void ActionMoveToLocation\(string fakeParameter/u);
    assert.match(response[0].documentation, /test file must never shadow/u);
  } finally {
    fs.rmSync(moduleRoot, { recursive: true, force: true });
  }
});

test('native definition lookup exposes the packed game fallback read-only', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, (context: TestContext) => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const request = {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
    symbol: 'ActionMoveToLocation',
  };
  const response = JSON.parse(binding.findDefinitions(JSON.stringify(request)));
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

  const source = JSON.parse(binding.readVirtualSource(JSON.stringify({
    ...request,
    resource: response[0].resource,
  })));
  assert.equal(source.uri, response[0].uri);
  assert.match(source.contents, /void ActionMoveToLocation\(location/u);
  assert.ok(source.contents.length > 500_000);
});

test('native symbol lookup exposes strong enums, variants, and compatibility aliases', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const baseRequest = {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
  };

  const enumType = JSON.parse(binding.findDefinitions(JSON.stringify({
    ...baseRequest,
    symbol: 'NwnrsLogLevel',
  })));
  assert.equal(enumType[0].kind, 'enum');
  assert.match(enumType[0].documentation, /Severity used by the runtime/u);

  const variant = JSON.parse(binding.findDefinitions(JSON.stringify({
    ...baseRequest,
    symbol: 'Info',
  })));
  assert.equal(variant[0].kind, 'enumVariant');
  assert.match(variant[0].signature, /^NwnrsLogLevel::Info/u);

  const alias = JSON.parse(binding.findDefinitions(JSON.stringify({
    ...baseRequest,
    symbol: 'NWNRS_LOG_LEVEL_INFO',
  })));
  assert.equal(alias[0].kind, 'constant');
  assert.match(alias[0].signature, /NwnrsLogLevel::Info/u);
});

test('native document Outline is hierarchical, overlay-aware, and source-only', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-outline-'));
  const sourcePath = path.join(root, 'outline.nss');
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
    + '#[nwnrs::events(module_load)]\n'
    + 'void Authored(int value) {}\n';
  try {
    const symbols: NativeDocumentSymbol[] = JSON.parse(binding.listDocumentSymbols(JSON.stringify({
      source_path: sourcePath,
      project_root: root,
      overlays: [{ path: sourcePath, contents }],
    })));
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
    assert.equal(functionSymbol.detail, 'event: module_load · void Authored(int value)');
    assert.equal(functionSymbol.range.start_line, 9);
    assert.deepEqual(functionSymbol.selection_range, {
      start_line: 10,
      start_column: 6,
      end_line: 10,
      end_column: 14,
    });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native document Outline indexes a packed game script read-only', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, (context: TestContext) => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const request = {
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
    symbol: 'ActionMoveToLocation',
  };
  const definitions = JSON.parse(binding.findDefinitions(JSON.stringify(request)));
  if (definitions.length === 0 || !definitions[0].resource) {
    context.skip('Neverwinter Nights installation was not discovered');
    return;
  }
  const symbols: NativeDocumentSymbol[] = JSON.parse(binding.listDocumentSymbols(JSON.stringify({
    source_path: request.source_path,
    project_root: moduleRoot,
    resource: definitions[0].resource,
  })));
  const action = required(
    symbols.find((symbol) => symbol.name === 'ActionMoveToLocation'),
    'ActionMoveToLocation outline symbol is missing',
  );
  assert.equal(action.kind, 'function');
  assert.match(action.detail, /^void ActionMoveToLocation\(location/u);
});

test('native compiler accepts the real nwnrs include with strong enums', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-real-include-'));
  const source = path.join(root, 'main.nss');
  fs.writeFileSync(
    source,
    '#include "nwnrs"\nvoid main() { NWNRS_Log("ready", NwnrsLogLevel::Info); }\n',
  );
  try {
    const response = JSON.parse(binding.checkNss(JSON.stringify({
      paths: [source],
      include_dirs: [path.join(repositoryRoot, 'include', 'nwnrs')],
    })));

    assert.equal(response.summary.failed, 0, JSON.stringify(response.diagnostics));
    assert.equal(response.summary.compiled, 1);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native compiler checks dirty overlays and reports more than the first error', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const fixtureDirectory = path.resolve(__dirname, 'fixtures');
  const sourcePath = path.join(fixtureDirectory, 'broken.nss');
  const response = JSON.parse(binding.checkNss(JSON.stringify({
    paths: [sourcePath],
    langspec: path.join(fixtureDirectory, 'nwscript.nss'),
    overlays: [{
      path: sourcePath,
      contents: 'void main()\n{\n    MissingOne;\n    MissingTwo;\n}\n',
    }],
  })));

  assert.equal(response.summary.failed, 1);
  assert.equal(response.diagnostics.length, 2);
  assert.deepEqual(
    response.diagnostics.map((diagnostic: NativeCheckDiagnostic) => diagnostic.start_line),
    [3, 4],
  );
});

test('native workspace helpers deduplicate dependencies and return watch roots', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const includeRoot = path.join(repositoryRoot, 'include', 'nwnrs');
  const deduplicated = JSON.parse(binding.deduplicateProjectRoots(JSON.stringify({
    roots: [moduleRoot, includeRoot],
  })));
  const watched = JSON.parse(binding.resolveWatchRoots(JSON.stringify({
    roots: [moduleRoot],
  })));

  assert.deepEqual(deduplicated, [fs.realpathSync.native(moduleRoot)]);
  assert.ok(watched.includes(fs.realpathSync.native(moduleRoot)));
  assert.ok(watched.includes(fs.realpathSync.native(includeRoot)));
});

test('native project index includes package sources and editable dependencies', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const repositoryRoot = path.resolve(__dirname, '..', '..', '..');
  const moduleRoot = path.join(repositoryRoot, 'module');
  const response = JSON.parse(binding.indexProject(JSON.stringify({
    source_path: path.join(moduleRoot, 'debug.nss'),
    project_root: moduleRoot,
  })));

  assert.deepEqual(response.warnings, []);
  assert.ok(response.documents.some((document: { readonly path: string }) =>
    document.path === path.join(moduleRoot, 'debug.nss')));
  assert.ok(response.documents.some((document: { readonly path: string }) =>
    document.path === path.join(repositoryRoot, 'include', 'nwnrs', 'nwnrs.nss')));
});

test('native references cross sibling scripts and preserve exact local bindings', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-references-'));
  const shared = path.join(root, 'shared.nss');
  const first = path.join(root, 'first.nss');
  const second = path.join(root, 'second.nss');
  fs.writeFileSync(shared, 'void SharedWork() {}\n');
  fs.writeFileSync(first, '#include "shared"\nvoid First() { SharedWork(); }\n');
  fs.writeFileSync(second, '#include "shared"\nvoid Second() { SharedWork(); }\n');
  try {
    const references: NativeReference[] = JSON.parse(binding.findReferences(JSON.stringify({
      source_path: first,
      project_root: root,
      symbol: 'SharedWork',
      line: 2,
      column: 16,
    })));
    assert.equal(references.length, 3);
    assert.equal(references.filter((reference) => reference.is_declaration).length, 1);
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
}, () => {
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
    const candidates = JSON.parse(binding.findIncludeCandidates(JSON.stringify(request)));
    assert.equal(candidates.length, 1);
    assert.equal(candidates[0].include_name, 'helper');

    const resolved = JSON.parse(binding.resolveSource(JSON.stringify({
      ...request,
      resource: 'helper',
    })));
    assert.equal(resolved.path, helper);
    assert.equal(resolved.uri, null);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('native manifest tooling validates syntax, paths, kinds, and dependencies', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, () => {
  const binding = require(bindingPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-native-nwpkg-'));
  const manifestPath = path.join(root, 'nwpkg.toml');
  try {
    const valid = JSON.parse(binding.checkNwpkg(JSON.stringify({
      path: manifestPath,
      contents: '[project]\nname = "fixture"\nkind = "include"\n\n[source]\npath = "."\n',
    })));
    assert.deepEqual(valid.diagnostics, []);

    const invalid = JSON.parse(binding.checkNwpkg(JSON.stringify({
      path: manifestPath,
      contents: '[project]\nname = "fixture"\nkind = "unknown"\n\n[source]\npath = "missing"\n',
    })));
    assert.ok(invalid.diagnostics.length >= 1);
    assert.ok(invalid.diagnostics.every(
      (diagnostic: { readonly start_line: number }) => diagnostic.start_line >= 1,
    ));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('persistent worker isolates and invalidates indexed package sessions', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-worker-session-'));
  const source = path.join(root, 'main.nss');
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
  const payload = { source_path: source, project_root: root };
  try {
    const initial = await request('indexProject', payload);
    assert.equal(initial.documents[0]?.symbols[0]?.name, 'Original');
    fs.writeFileSync(source, 'void Updated() {}\n');
    const revalidated = await request('indexProject', payload);
    assert.equal(revalidated.documents[0]?.symbols[0]?.name, 'Updated');
    worker.postMessage({ type: 'invalidate', sessionKey: root, changedPath: source });
    const updated = await request('indexProject', payload);
    assert.equal(updated.documents[0]?.symbols[0]?.name, 'Updated');
  } finally {
    await worker.terminate();
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

    const request = {
      session_key: manifestPath,
      path: path.join(moduleRoot, '.nwnrs-resource-catalog'),
      project_root: moduleRoot,
      root: null,
      user: null,
      language: 'english',
      load_ovr: false,
      archives: [],
    };
    let layers: NativeResourceCatalogItem[];
    try {
      layers = (await client.listResources({ ...request, stage: 'layers' })).items;
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
      ...request,
      stage: 'families',
      layer: 'Vanilla',
    })).items;
    assert.ok(families.some(({ family, count }: NativeResourceCatalogItem) => family === 'Models' && count > 0));
    const types = (await client.listResources({
      ...request,
      stage: 'types',
      layer: 'Vanilla',
      family: 'Models',
    })).items;
    assert.ok(types.some(({ extension, count }: NativeResourceCatalogItem) => extension === 'mdl' && count > 0));
    const names = (await client.listResources({
      ...request,
      stage: 'names',
      layer: 'Vanilla',
      family: 'Models',
      extension: 'mdl',
      prefix: 'c_bodak',
    })).items;
    assert.ok(names.some(({ resource }: NativeResourceCatalogItem) => resource === 'c_bodak.mdl'));

    const vanillaOnly = (await client.listResources({
      ...request,
      session_key: `${manifestPath}:vanilla-only`,
      include_project_resources: false,
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
    authoredPacket = Buffer.from(await service.loadScene(JSON.stringify({
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
  const inspection: AreaObjectInspection = JSON.parse(
    await service.inspectAreaObject(JSON.stringify(inspectionRequest)),
  );
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
    JSON.parse(await service.inspectAreaObject(JSON.stringify(inspectionRequest))),
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

  const modelPacket = Buffer.from(await service.loadScene(JSON.stringify({
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
  const animationPacket = Buffer.from(await service.loadAnimation(JSON.stringify({
    sessionKey: request.session_key,
    assetKey: modelManifest.assetKey,
    modelIndex: 0,
    animationIndex: 0,
  })));
  const animationManifestLength = animationPacket.readUInt32LE(8);
  assert.equal((12 + animationManifestLength) % 4, 0);
  const animationManifest: SceneAnimationPacketManifest = JSON.parse(
    animationPacket.subarray(12, 12 + animationManifestLength).toString('utf8'),
  );
  assert.equal(animationManifest.schema, 'nwnrs.scene.animation');
  assert.equal(animationManifest.assetKey, modelManifest.assetKey);
  assert.equal(animationManifest.animation.tracksLoaded, true);
  assert.ok(animationManifest.animation.nodeTracks.length > 0);
  const texturePacket = Buffer.from(await service.loadTexture(JSON.stringify({
    sessionKey: request.session_key,
    assetKey: modelManifest.assetKey,
    textureIndex: 0,
    preferCompressed: true,
  })));
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
  const rgbaPacket = Buffer.from(await service.loadTexture(JSON.stringify({
    sessionKey: request.session_key,
    assetKey: modelManifest.assetKey,
    textureIndex: 0,
    preferCompressed: false,
  })));
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
  const walkmeshPacket = Buffer.from(await service.loadScene(JSON.stringify({
    ...request,
    path: path.join(repositoryRoot, walkmeshResource),
  })));
  const walkmeshManifestLength = walkmeshPacket.readUInt32LE(8);
  const walkmeshManifest: ScenePacketManifest = JSON.parse(
    walkmeshPacket.subarray(12, 12 + walkmeshManifestLength).toString('utf8'),
  );
  assert.equal(walkmeshManifest.source, 'walkmesh');
  assert.ok(walkmeshManifest.instances.some((entry) => entry.kind === 'collision'));

  const resolved = JSON.parse(await service.resolveResource(JSON.stringify({
    ...request,
    path: path.join(repositoryRoot, 'skyboxes.2da'),
    archives: [],
  })));
  assert.equal(resolved.resource, 'skyboxes.2da');
  assert.equal(resolved.file_path, null);
  assert.match(resolved.origin, /KeyTable/u);

  const overrideRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-viewer-origin-'));
  const overridePath = path.join(overrideRoot, 'c_rat.mdl');
  fs.writeFileSync(overridePath, 'newmodel c_rat\nsetsupermodel c_rat null\nbeginmodelgeom c_rat\nnode dummy c_rat\n parent null\nendnode\nendmodelgeom c_rat\ndonemodel c_rat\n');
  try {
    const override = JSON.parse(await service.resolveResource(JSON.stringify({
      ...request,
      session_key: overrideRoot,
      path: overridePath,
      project_root: overrideRoot,
      archives: [],
      include_project_resources: true,
    })));
    assert.equal(path.resolve(override.file_path), path.resolve(overridePath));
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
  const request = {
    session_key: `${repositoryRoot}/nwpkg.toml`,
    path: path.join(repositoryRoot, 'c_bodak.mdl'),
    project_root: repositoryRoot,
    root: null,
    user: null,
    language: 'english',
    load_ovr: false,
    archives: [],
  };
  let scenePacket;
  try {
    scenePacket = Buffer.from(await service.loadScene(JSON.stringify(request)));
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
    const packet = Buffer.from(await service.loadAnimation(JSON.stringify({
      sessionKey: request.session_key,
      assetKey: scene.manifest.assetKey,
      modelIndex: 0,
      animationIndex,
    })));
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
  const request = {
    session_key: `${repositoryRoot}/nwpkg.toml`,
    path: path.join(repositoryRoot, 'c_bodak.mdl'),
    project_root: repositoryRoot,
    root: null,
    user: null,
    language: 'english',
    load_ovr: false,
    archives: [],
  };
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
    assert.equal(resolved.file_path, null);
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
