'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const Module = require('node:module');
const path = require('node:path');
const test = require('node:test');

interface TestPackage {
  readonly name?: string;
  readonly root: string;
  readonly manifestPath?: string;
}

interface TestResourceLayer {
  readonly label: string;
  readonly layer: string;
}

interface TestSourceFile {
  readonly path: string;
  readonly relativePath: string;
  readonly kind: string;
}

interface TestAreaObject {
  readonly key: string;
  readonly kind: string;
  readonly label: string;
  readonly sourceIndex: number;
  readonly tag?: string;
  readonly templateResref?: string;
}

interface TestSourceArea {
  readonly resref: string;
  readonly registered: boolean;
  readonly missing: readonly string[];
  readonly conflicts?: readonly string[];
  readonly files: readonly TestSourceFile[];
  readonly objects?: readonly TestAreaObject[];
}

interface TestTreeNode {
  readonly kind: string;
  readonly label: string;
  readonly layer?: string;
  readonly children?: TestTreeNode[];
  readonly package?: TestPackage;
  readonly area?: TestSourceArea;
  readonly object?: TestAreaObject;
}

interface SidebarTestModule {
  buildSourceFileTree(files: readonly TestSourceFile[]): TestTreeNode[];
  childResourceQuery(node: Readonly<Record<string, string>>): Readonly<Record<string, string>>;
  owningPackage(filePath: string, packages: readonly TestPackage[]): TestPackage | undefined;
  sourceSections(
    source: {
      readonly areas: readonly TestSourceArea[];
      readonly dialogs: readonly TestSourceFile[];
      readonly code: readonly TestSourceFile[];
    },
    packageInfo?: TestPackage,
  ): TestTreeNode[];
  sortResourceLayers(items: readonly TestResourceLayer[]): TestResourceLayer[];
}

function loadSidebarWithoutVsCodeHost(): SidebarTestModule {
  const originalLoad = Module._load;
  try {
    Module._load = function load(
      request: string,
      parent: NodeModule | null | undefined,
      isMain: boolean,
    ) {
      if (request === 'vscode') return {};
      return originalLoad.call(this, request, parent, isMain);
    };
    const modulePath = require.resolve('../dist/src/sidebar');
    delete require.cache[modulePath];
    return require(modulePath);
  } finally {
    Module._load = originalLoad;
  }
}

const {
  buildSourceFileTree,
  childResourceQuery,
  owningPackage,
  sourceSections,
  sortResourceLayers,
}: SidebarTestModule = loadSidebarWithoutVsCodeHost();

function required<Value>(value: Value | null | undefined, label: string): Value {
  assert.ok(value, label);
  if (value == null) throw new Error(label);
  return value;
}

test('Source is an in-sidebar tree and never redirects to Explorer', () => {
  const implementation = fs.readFileSync(
    path.resolve(__dirname, '..', 'dist', 'src', 'sidebar.js'),
    'utf8',
  );
  assert.doesNotMatch(implementation, /revealInExplorer/u);
  assert.match(implementation, /nwnrs\.sidebar\.openSourceFile/u);
});

test('active package ownership selects the deepest nested nwpkg root', () => {
  const root = path.resolve('/workspace');
  const nested = path.join(root, 'packages', 'module');
  const packages = [
    { name: 'root', root },
    { name: 'module', root: nested },
  ];

  assert.equal(owningPackage(path.join(nested, 'src', 'main.nss'), packages)?.name, 'module');
  assert.equal(owningPackage(path.join(root, 'shared', 'types.nss'), packages)?.name, 'root');
  assert.equal(owningPackage('/somewhere/else/main.nss', packages), undefined);
});

test('resource layers preserve resolver precedence instead of alphabetical order', () => {
  const sorted = sortResourceLayers([
    { label: 'Vanilla', layer: 'Vanilla' },
    { label: 'Archives', layer: 'Archives' },
    { label: 'Workspace', layer: 'Workspace' },
    { label: 'User Override', layer: 'User Override' },
    { label: 'Package Dependencies', layer: 'Package Dependencies' },
  ]);
  assert.deepEqual(sorted.map(({ layer }) => layer), [
    'Workspace',
    'Package Dependencies',
    'User Override',
    'Archives',
    'Vanilla',
  ]);
});

test('lazy resource branches retain every parent filter', () => {
  assert.deepEqual(childResourceQuery({ kind: 'layer', layer: 'Vanilla' }), {
    stage: 'families', layer: 'Vanilla',
  });
  assert.deepEqual(childResourceQuery({
    kind: 'family', layer: 'Vanilla', family: 'Models',
  }), {
    stage: 'types', layer: 'Vanilla', family: 'Models',
  });
  assert.deepEqual(childResourceQuery({
    kind: 'prefix', layer: 'Vanilla', family: 'Models', extension: 'mdl', prefix: 'c_',
  }), {
    stage: 'names', layer: 'Vanilla', family: 'Models', extension: 'mdl', prefix: 'c_',
  });
});

test('source sections hide empty categories and preserve physical directories', () => {
  const sections = sourceSections({
    areas: [],
    dialogs: [],
    code: [
      { path: '/workspace/code/main.nss', relativePath: 'code/main.nss', kind: 'nss' },
      { path: '/workspace/code/shared/types.nss', relativePath: 'code/shared/types.nss', kind: 'nss' },
    ],
  });
  assert.deepEqual(sections.map(({ label }) => label), ['Code']);
  const section = required(sections[0], 'Code section is missing');
  const code = required(section.children?.[0], 'Code directory is missing');
  const shared = required(code.children?.[0], 'Shared directory is missing');
  assert.equal(code.label, 'code');
  assert.equal(shared.label, 'shared');
  assert.equal(shared.children?.[0]?.label, 'types.nss');
  assert.equal(code.children?.[1]?.label, 'main.nss');
});

test('source areas follow IFO registration and isolate unregistered bundles', () => {
  const packageInfo = { root: '/workspace', manifestPath: '/workspace/nwpkg.toml' };
  const sections = sourceSections({
    areas: [
      {
        resref: 'start', registered: true, missing: ['GIC'],
        files: [
          { path: '/workspace/areas/start.are.json', relativePath: 'areas/start.are.json', kind: 'are' },
          { path: '/workspace/areas/start.git.json', relativePath: 'areas/start.git.json', kind: 'git' },
        ],
      },
      {
        resref: 'orphan', registered: false, missing: ['ARE', 'GIT'],
        files: [
          { path: '/workspace/areas/orphan.gic.json', relativePath: 'areas/orphan.gic.json', kind: 'gic' },
        ],
      },
    ],
    dialogs: [{
      path: '/workspace/dialogs/intro.dlg.json',
      relativePath: 'dialogs/intro.dlg.json',
      kind: 'dlgJson',
    }],
    code: [],
  }, packageInfo);
  assert.deepEqual(sections.map(({ label }) => label), ['Areas', 'Dialogs']);
  const areaSection = required(sections[0], 'Areas section is missing');
  const areasFolder = required(
    areaSection.children?.find(({ label }) => label === 'areas'),
    'Registered areas folder is missing',
  );
  const registeredArea = required(areasFolder.children?.[0], 'Registered area is missing');
  assert.equal(registeredArea.area?.resref, 'start');
  assert.equal(registeredArea.package, packageInfo);
  assert.equal(Object.hasOwn(registeredArea, 'children'), false);
  const unregistered = areaSection.children?.find(({ kind }) => kind === 'sourceUnregistered');
  assert.ok(unregistered);
  assert.equal(unregistered?.children?.[0]?.children?.[0]?.area?.resref, 'orphan');
});

test('source file trees retain compound dialog filenames', () => {
  const tree = buildSourceFileTree([{
    path: '/workspace/dialogs/intro.dlg.json',
    relativePath: 'dialogs/intro.dlg.json',
    kind: 'dlgJson',
  }]);
  assert.equal(tree[0]?.label, 'dialogs');
  assert.equal(tree[0]?.children?.[0]?.label, 'intro.dlg.json');
});

test('area sources expose only non-empty authored-object categories', () => {
  const packageInfo = { root: '/workspace', manifestPath: '/workspace/nwpkg.toml' };
  const [areas] = sourceSections({
    areas: [{
      resref: 'start', registered: true, missing: ['GIC'], conflicts: [],
      files: [
        { path: '/workspace/start.are', relativePath: 'start.are', kind: 'are' },
        { path: '/workspace/start.git', relativePath: 'start.git', kind: 'git' },
      ],
      objects: [
        { key: 'creature:0', kind: 'creature', label: 'Bodak', sourceIndex: 0, tag: 'bodak' },
        { key: 'placeable:0', kind: 'placeable', label: 'Chest', sourceIndex: 0, templateResref: 'plc_chest1' },
      ],
    }],
    dialogs: [],
    code: [],
  }, packageInfo);
  const area = required(areas?.children?.[0], 'Area node is missing');
  assert.equal(area.kind, 'sourceArea');
  assert.deepEqual(area.children?.map(({ label }) => label), ['Creatures', 'Placeables']);
  assert.equal(area.children?.[0]?.children?.[0]?.object?.key, 'creature:0');
  assert.equal(area.children?.[1]?.children?.[0]?.package, packageInfo);
});
