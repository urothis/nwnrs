'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const extensionRoot = path.resolve(__dirname, '..');
const manifest = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));

test('nwnrs contributes an always-visible Activity Bar container with package and resource views', () => {
  assert.deepEqual(manifest.contributes.viewsContainers.activitybar, [{
    id: 'nwnrs', title: 'nwnrs', icon: 'images/activity-bar.svg',
  }]);
  assert.deepEqual(manifest.contributes.views.nwnrs, [
    { id: 'nwnrs.packages', name: 'Packages', visibility: 'visible' },
    { id: 'nwnrs.resources', name: 'Resources', visibility: 'collapsed' },
  ]);
  assert.deepEqual(
    manifest.contributes.viewsWelcome.map(({ view }) => view),
    ['nwnrs.packages', 'nwnrs.resources'],
  );
  assert.ok(manifest.contributes.viewsWelcome.every(({ contents }) => contents.length > 0));
  assert.ok(manifest.activationEvents.includes('onView:nwnrs.packages'));
  assert.ok(manifest.activationEvents.includes('onView:nwnrs.resources'));
  assert.equal(manifest.activationEvents.includes('onView:nwnrs.tools'), false);
  assert.ok(manifest.contributes.menus['view/title'].some(
    ({ command, when }) => command === 'nwnrs.sidebar.unpinPackage'
      && when.includes('nwnrs.packagePinned'),
  ));
  assert.ok(manifest.contributes.customEditors[0].selector.some(
    ({ filenamePattern }) => filenamePattern === '*.dlg.json',
  ));
  assert.ok(manifest.contributes.customEditors[0].selector.some(
    ({ filenamePattern }) => filenamePattern === '*.ncs',
  ));
  assert.ok(manifest.contributes.customEditors[0].selector.some(
    ({ filenamePattern }) => filenamePattern === '*.ndb',
  ));
});

test('Activity Bar artwork is a dedicated theme-aware monochrome SVG', () => {
  const iconPath = path.join(extensionRoot, 'images', 'activity-bar.svg');
  const icon = fs.readFileSync(iconPath, 'utf8');
  const source = fs.readFileSync(path.resolve(extensionRoot, '..', '..', 'assets', 'logo', 'icon.svg'), 'utf8');
  const paths = (svg) => [...svg.matchAll(/<path d="([\s\S]*?)"\/>/gu)]
    .map((match) => match[1].replace(/\s+/gu, ' ').trim());
  assert.match(icon, /viewBox="-25 -40 952 952"/u);
  assert.match(icon, /currentColor/u);
  assert.deepEqual(paths(icon), paths(source));
  assert.equal(paths(icon).length, 4);
  assert.doesNotMatch(icon, /stroke=|stroke-linecap|stroke-linejoin/u);
  assert.doesNotMatch(icon, /#[0-9a-f]{3,8}|rgb\(|style=/iu);
});
