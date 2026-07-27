'use strict';

import type { NativeEventCatalogEntry } from '../src/generated/native';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const {
  buildEventCatalogDocument,
  eventHoverMarkdown,
}: typeof import('../src/event-catalog') = require('../dist/src/event-catalog');

const moduleLoad: NativeEventCatalogEntry = {
  subject: 'module_load',
  name: 'module.load',
  phase: 'before',
  skippable: false,
  resultKind: 'none',
  control: 'observe only',
  source: 'native detour',
  frequency: 'normal',
  filter: 'subscription only',
};

test('event catalog virtual document exposes stable route lines and runtime semantics', () => {
  const generated = buildEventCatalogDocument([moduleLoad]);
  const line = generated.routeLines.get('module_load::before');
  assert.equal(typeof line, 'number');
  assert.equal(generated.contents.split('\n')[line || 0], '## module_load::before');
  assert.match(generated.contents, /Runtime event: `module\.load`/u);
  assert.match(eventHoverMarkdown(moduleLoad), /Control: observe only/u);
});

test('extension event semantics have no TypeScript parser or workspace event scan', () => {
  const sourceRoot = path.resolve(__dirname, '..', 'src');
  const extension = fs.readFileSync(path.join(sourceRoot, 'extension.ts'), 'utf8');
  const catalog = fs.readFileSync(path.join(sourceRoot, 'event-catalog.ts'), 'utf8');
  for (const removedRecognizer of [
    'eventRegistrations',
    'eventCompletionAt',
    'eventRouteAt',
    'eventLaneHandlers',
  ]) {
    assert.equal(extension.includes(removedRecognizer), false);
    assert.equal(catalog.includes(removedRecognizer), false);
  }
  const eventProviders = extension.slice(
    extension.indexOf('async provideEventCompletions('),
    extension.indexOf('async provideEventCatalogDocument('),
  );
  assert.doesNotMatch(
    eventProviders,
    /workspace\.findFiles/u,
    'event registration discovery must not scan arbitrary workspace NSS files',
  );
});
