// Tests for scripts/check-task-citations.mjs:  node --test scripts/check-task-citations.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isChecked, findCitations } from './check-task-citations.mjs';

const tokens = (text) => findCitations(text).map(({ token }) => token);

test('source files are checked; the scripts that read the roadmap and generated files are not', () => {
  for (const file of ['crates/a/src/lib.rs', 'apps/desktop/src/App.tsx', 'apps/desktop/src/App.css',
    'scripts/check-file-sizes.mjs']) {
    assert.equal(isChecked(file), true, file);
  }
  for (const file of ['scripts/check-docs.mjs', 'scripts/check-docs.test.mjs', 'scripts/check-task-citations.mjs',
    'scripts/check-task-citations.test.mjs', 'bindings/Action.ts', 'crates/core/migrations/0001_initial.rs',
    'docs/roadmap/todo.md']) {
    assert.equal(isChecked(file), false, file);
  }
});

test('a task number is found with its letters, wherever it sits; a generic or a word is not', () => {
  assert.deepEqual(tokens('// T32 and T182f, then roadmap task T4.4\n'), ['T32', 'T182f', 'T4']);
  assert.deepEqual(tokens('it("reads a daemon from before T206d", () => {});'), ['T206d']);
  assert.deepEqual(tokens('fn f<T>(x: T) -> TCP { Task::T1x0 }'), []);
  assert.deepEqual(tokens('type Pair<A, B> = [A, B]; // ATT32 BT7'), []);
});

test('a text that cites nothing has no finding', () => {
  assert.deepEqual(findCitations(''), []);
  assert.deepEqual(findCitations('fn f<T>(x: T) {}\n// says the reason itself\n'), []);
});

test('each citation is one finding, on the line it is on', () => {
  const text = 'fn a() {}\n// see T182f\nfn b() {}\n// and roadmap task T32\n';
  assert.deepEqual(findCitations(text), [
    { line: 2, token: 'T182f' },
    { line: 4, token: 'T32' },
  ]);
});
