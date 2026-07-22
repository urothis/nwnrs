'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { Worker } = require('node:worker_threads');
const { LanguageWorkerClient } = require('../src/language-worker-client');

const bindingPath = path.resolve(
  __dirname,
  '..',
  'native',
  'nwnrs-vscode.darwin-arm64.node',
);

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
  const worker = new Worker(path.resolve(__dirname, '..', 'src', 'compiler-worker.js'), {
    workerData: {
      bindingPath,
      request: {
        paths: [path.join(fixtureDirectory, 'broken.nss')],
        langspec: path.join(fixtureDirectory, 'nwscript.nss'),
      },
    },
  });
  const result = await new Promise((resolve, reject) => {
    worker.once('message', resolve);
    worker.once('error', reject);
  });
  assert.equal(result.error, undefined);
  assert.equal(result.response.summary.failed, 1);
  assert.equal(result.response.diagnostics[0].code, -622);
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
}, (context) => {
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
    const symbols = JSON.parse(binding.listDocumentSymbols(JSON.stringify({
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

    const structure = symbols.find((symbol) => symbol.name === 'Stats');
    assert.equal(structure.kind, 'struct');
    assert.deepEqual(structure.children.map((child) => child.name), ['score', 'label']);
    assert.deepEqual(structure.children.map((child) => child.kind), ['field', 'field']);

    const enumeration = symbols.find((symbol) => symbol.name === 'Mode');
    assert.equal(enumeration.kind, 'enum');
    assert.deepEqual(enumeration.children.map((child) => child.name), ['Ready', 'Running']);

    const functionSymbol = symbols.find((symbol) => symbol.name === 'Authored');
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
}, (context) => {
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
  const symbols = JSON.parse(binding.listDocumentSymbols(JSON.stringify({
    source_path: request.source_path,
    project_root: moduleRoot,
    resource: definitions[0].resource,
  })));
  const action = symbols.find((symbol) => symbol.name === 'ActionMoveToLocation');
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
  assert.deepEqual(response.diagnostics.map((diagnostic) => diagnostic.start_line), [3, 4]);
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
  assert.ok(response.documents.some((document) =>
    document.path === path.join(moduleRoot, 'debug.nss')));
  assert.ok(response.documents.some((document) =>
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
    const references = JSON.parse(binding.findReferences(JSON.stringify({
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
    assert.ok(invalid.diagnostics.every((diagnostic) => diagnostic.start_line >= 1));
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
  const worker = new Worker(path.resolve(__dirname, '..', 'src', 'compiler-worker.js'), {
    workerData: { bindingPath, persistent: true },
  });
  let requestId = 0;
  const request = (method, payload, sessionKey = root) => new Promise((resolve, reject) => {
    const id = ++requestId;
    const onMessage = (message) => {
      if (message.type !== 'response' || message.id !== id) {
        return;
      }
      worker.off('message', onMessage);
      if (message.error) {
        reject(new Error(message.error));
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
    assert.equal(initial.documents[0].symbols[0].name, 'Original');
    fs.writeFileSync(source, 'void Updated() {}\n');
    const revalidated = await request('indexProject', payload);
    assert.equal(revalidated.documents[0].symbols[0].name, 'Updated');
    worker.postMessage({ type: 'invalidate', sessionKey: root, changedPath: source });
    const updated = await request('indexProject', payload);
    assert.equal(updated.documents[0].symbols[0].name, 'Updated');
  } finally {
    await worker.terminate();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('language worker restarts cleanly and accepts requests afterward', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const client = new LanguageWorkerClient(
    path.resolve(__dirname, '..', 'src', 'compiler-worker.js'),
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
