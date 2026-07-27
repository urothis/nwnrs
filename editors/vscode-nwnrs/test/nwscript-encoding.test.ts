'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const {
  unrepresentableUtf16Ranges,
}: typeof import('../src/nwscript-encoding') = require('../dist/src/nwscript-encoding');

test('reports exact UTF-16 ranges for every unrepresentable sequence', () => {
  const source = 'ok 😀漢 middle € end 🧪';
  const decoded = 'ok ?? middle € end ?';
  const ranges = unrepresentableUtf16Ranges(source, decoded);

  assert.deepEqual(ranges, [
    { start: 3, end: 6 },
    { start: 20, end: 22 },
  ]);
  const first = ranges[0]!;
  const second = ranges[1]!;
  assert.equal(source.slice(first.start, first.end), '😀漢');
  assert.equal(source.slice(second.start, second.end), '🧪');
});

test('accepts every representable Windows-1252 character unchanged', () => {
  const source = 'café — “quoted” €';
  assert.deepEqual(unrepresentableUtf16Ranges(source, source), []);
});
