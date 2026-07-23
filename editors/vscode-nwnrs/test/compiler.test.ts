'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const {
  buildCheckRequest,
  buildDefinitionRequest,
  buildDocumentSymbolsRequest,
  diagnosticRange,
  expandPathVariables,
  findProjectRoot,
  formatHoverDocumentation,
  isNssPath,
  nativeBindingPath,
  selectHoverDefinition,
}: typeof import('../src/compiler') = require('../dist/src/compiler');

function required<Value>(value: Value | null | undefined, label: string): Value {
  assert.ok(value, label);
  if (value == null) throw new Error(label);
  return value;
}

test('recognizes NSS paths case-insensitively', () => {
  assert.equal(isNssPath('/module/startup.nss'), true);
  assert.equal(isNssPath('/module/STARTUP.NSS'), true);
  assert.equal(isNssPath('/module/startup.ncs'), false);
});

test('finds the nearest owning nwpkg manifest', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-vscode-project-'));
  const project = path.join(root, 'module');
  const nested = path.join(project, 'scripts', 'encounters');
  fs.mkdirSync(nested, { recursive: true });
  fs.writeFileSync(path.join(project, 'nwpkg.toml'), '[project]\nname = "fixture"\n');
  const source = path.join(nested, 'ambush.nss');
  fs.writeFileSync(source, 'void main() {}\n');
  try {
    assert.equal(findProjectRoot(source), project);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('accepts a package directory when finding its nwpkg manifest', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nwnrs-vscode-project-root-'));
  fs.writeFileSync(path.join(root, 'nwpkg.toml'), '[project]\nname = "fixture"\n');
  try {
    assert.equal(findProjectRoot(root), root);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('builds an in-process compiler request', () => {
  assert.deepEqual(
    buildCheckRequest(['/module/startup.nss'], {
      includeDirectories: ['/shared/include'],
      overlays: [{ path: '/module/startup.nss', contents: 'void main() {}' }],
      langspecPath: '/game/nwscript.nss',
      maxIncludeDepth: 32,
      noEntrypointCheck: true,
      rootPath: '/game',
      userPath: '/user',
      language: 'english',
      loadOvr: true,
    }),
    {
      paths: ['/module/startup.nss'],
      no_entrypoint_check: true,
      langspec: '/game/nwscript.nss',
      include_dirs: ['/shared/include'],
      overlays: [{ path: '/module/startup.nss', contents: 'void main() {}' }],
      max_include_depth: 32,
      max_diagnostics_per_input: 50,
      recurse: false,
      root: '/game',
      user: '/user',
      language: 'english',
      load_ovr: true,
    },
  );
});

test('selects the bundled macOS arm64 compiler', () => {
  assert.equal(
    nativeBindingPath('/extension', 'darwin', 'arm64'),
    path.join('/extension', 'native', 'nwnrs-vscode.darwin-arm64.node'),
  );
  assert.throws(
    () => nativeBindingPath('/extension', 'linux', 'x64'),
    /does not yet support linux-x64/u,
  );
});

test('builds a project-aware definition request', () => {
  assert.deepEqual(
    buildDefinitionRequest('/module/startup.nss', 'NWNRS_Log', {
      projectRoot: '/module',
      includeDirectories: ['/shared/include'],
      qualifier: 'logging',
      overlays: [{ path: '/module/startup.nss', contents: 'void main() {}' }],
      langspecPath: '/game/nwscript.nss',
      maxIncludeDepth: 32,
      rootPath: '/game',
      userPath: '/user',
      language: 'german',
      loadOvr: true,
    }),
    {
      source_path: '/module/startup.nss',
      symbol: 'NWNRS_Log',
      qualifier: 'logging',
      project_root: '/module',
      include_dirs: ['/shared/include'],
      overlays: [{ path: '/module/startup.nss', contents: 'void main() {}' }],
      langspec: '/game/nwscript.nss',
      max_include_depth: 32,
      root: '/game',
      user: '/user',
      language: 'german',
      load_ovr: true,
    },
  );
});

test('builds a project-aware document Outline request', () => {
  assert.deepEqual(
    buildDocumentSymbolsRequest('/module/startup.nss', {
      resource: 'nwscript',
      projectRoot: '/module',
      includeDirectories: ['/shared/include'],
      overlays: [{ path: '/module/startup.nss', contents: 'void main() {}' }],
      langspecPath: '/game/nwscript.nss',
      maxIncludeDepth: 32,
      rootPath: '/game',
      userPath: '/user',
      language: 'german',
      loadOvr: true,
    }),
    {
      source_path: '/module/startup.nss',
      resource: 'nwscript',
      project_root: '/module',
      include_dirs: ['/shared/include'],
      overlays: [{ path: '/module/startup.nss', contents: 'void main() {}' }],
      langspec: '/game/nwscript.nss',
      max_include_depth: 32,
      root: '/game',
      user: '/user',
      language: 'german',
      load_ovr: true,
    },
  );
});

test('selects a documented implementation for hover information', () => {
  const declaration = {
    signature: 'void NWNRS_Log(string message);',
    documentation: 'Logs one message.',
    is_implementation: false,
  };
  const implementation = {
    signature: 'void NWNRS_Log(string message)',
    documentation: 'Logs one message.\n@param message Message to emit.',
    is_implementation: true,
  };
  assert.equal(selectHoverDefinition([declaration, implementation]), implementation);
  assert.equal(selectHoverDefinition([]), undefined);
});

test('formats NSS documentation tags for a VS Code hover', () => {
  assert.equal(
    formatHoverDocumentation(
      'Sends a structured log message.\n'
      + '@param sMessage Message to emit.\n'
      + '@param nLevel One of the log-level constants.\n'
      + '@return Whether the message was accepted.\n'
      + '@private',
    ),
    'Sends a structured log message.\n\n'
      + '**Parameters**\n\n'
      + '- `sMessage` — Message to emit.\n'
      + '- `nLevel` — One of the log-level constants.\n\n'
      + '**Returns**\n\n'
      + 'Whether the message was accepted.\n\n'
      + '_Internal API._',
  );
});

test('formats vanilla nwscript comments without a documentation sidecar', () => {
  assert.equal(
    formatHoverDocumentation(
      'The action subject will move to lDestination.\n'
      + '- lDestination: The object will move to this location. If the location is\n'
      + '  invalid or a path cannot be found to it, the command does nothing.\n'
      + '- bRun: If this is TRUE, the action subject will run rather than walk\n'
      + '* No return value, but if an error occurs the log file will contain\n'
      + '  "MoveToPoint failed."',
    ),
    'The action subject will move to lDestination.\n\n'
      + '**Parameters**\n\n'
      + '- `lDestination` — The object will move to this location. If the location is '
      + 'invalid or a path cannot be found to it, the command does nothing.\n'
      + '- `bRun` — If this is TRUE, the action subject will run rather than walk\n\n'
      + '**Returns**\n\n'
      + 'No return value, but if an error occurs the log file will contain '
      + '"MoveToPoint failed."',
  );
});

test('preserves vanilla notes and unrecognized documentation lines', () => {
  assert.equal(
    formatHoverDocumentation('Does work.\nNote: This is expensive.\n  Use sparingly.\n@since vanilla'),
    'Does work. @since vanilla\n\n**Notes**\n\nThis is expensive. Use sparingly.',
  );
});

test('converts one-based compiler spans to VS Code ranges', () => {
  assert.deepEqual(
    diagnosticRange({
      start_line: 3,
      start_column: 5,
      end_line: 3,
      end_column: 10,
    }),
    { startLine: 2, startColumn: 4, endLine: 2, endColumn: 9 },
  );
  assert.deepEqual(diagnosticRange({}), {
    startLine: 0,
    startColumn: 0,
    endLine: 0,
    endColumn: 1,
  });
});

test('expands workspace, project, and file path variables', () => {
  assert.equal(
    expandPathVariables('${workspaceFolder}/include:${projectRoot}:${fileDirname}', {
      workspaceFolder: '/workspace',
      projectRoot: '/workspace/module',
      fileDirname: '/workspace/module/scripts',
    }),
    '/workspace/include:/workspace/module:/workspace/module/scripts',
  );
});

test('grammar covers compiler extensions without advertising unsupported directives', () => {
  const grammar: {
    repository: Record<string, {
      patterns: Array<{ match: string; name?: string; patterns?: Array<{ match: string }> }>;
    }>;
  } = JSON.parse(fs.readFileSync(
    path.join(__dirname, '..', 'syntaxes', 'nwscript.tmLanguage.json'),
    'utf8',
  ));
  const preprocessorEntry = required(
    grammar.repository.preprocessor?.patterns[0],
    'Preprocessor grammar is missing',
  );
  const macroEntries = required(grammar.repository.macros?.patterns, 'Macro grammar is missing');
  const typeEntries = required(grammar.repository.types?.patterns, 'Type grammar is missing');
  const attributeEntry = required(
    grammar.repository.compilerAttributes?.patterns[0]?.patterns?.[0],
    'Compiler attribute grammar is missing',
  );
  const preprocessor = new RegExp(preprocessorEntry.match, 'u');
  const macroPatterns = macroEntries.map((entry) => new RegExp(entry.match, 'u'));
  const typePattern = new RegExp(
    typeEntries.find((entry) => entry.name === 'storage.type.nwscript')?.match ?? '',
    'u',
  );
  const attributePattern = new RegExp(
    attributeEntry.match,
    'u',
  );

  assert.equal(preprocessor.test('#include "nwnrs"'), true);
  assert.equal(preprocessor.test('#define LEVEL 2'), true);
  assert.equal(preprocessor.test('#ifdef WINDOWS'), false);
  assert.equal(macroPatterns.some((pattern) => pattern.test('macro_rules! handler')), true);
  assert.equal(macroPatterns.some((pattern) => pattern.test('proc_macro! project::events')), true);
  assert.equal(macroPatterns.some((pattern) => pattern.test('quote! { $handler }')), true);
  assert.equal(macroPatterns.some((pattern) => pattern.test('$handler:expr')), true);
  assert.equal(macroPatterns.some((pattern) => pattern.test('$($handler),*')), true);
  assert.equal(typePattern.test('tokenstream_list'), true);
  assert.equal(typePattern.test('quote_bindings'), true);
  assert.equal(attributePattern.test('default'), true);
  assert.equal(attributePattern.test('before'), false);
});
