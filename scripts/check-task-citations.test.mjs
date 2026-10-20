// Tests for scripts/check-task-citations.mjs:  node --test scripts/check-task-citations.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { BASELINE, isChecked, countCitations, judge, shrunk, renderBaseline } from './check-task-citations.mjs';

const baseline = (files) => ({ files });
const counts = (entries) => new Map(Object.entries(entries));

test('source files are checked; the scripts that read the roadmap and generated files are not', () => {
  for (const file of ['crates/a/src/lib.rs', 'apps/desktop/src/App.tsx', 'apps/desktop/src/App.css',
    'scripts/check-file-sizes.mjs']) {
    assert.equal(isChecked(file), true, file);
  }
  for (const file of ['scripts/check-docs.mjs', 'scripts/check-docs.test.mjs', 'scripts/check-task-citations.mjs',
    'bindings/Action.ts', 'crates/core/migrations/0001_initial.rs', 'docs/roadmap/todo.md']) {
    assert.equal(isChecked(file), false, file);
  }
});

test('a task number is counted with its letters, wherever it sits; a generic or a word is not', () => {
  assert.equal(countCitations('// T32 and T182f, then roadmap task T4.4\n'), 3);
  assert.equal(countCitations('it("reads a daemon from before T206d", () => {});'), 1);
  assert.equal(countCitations('fn f<T>(x: T) -> TCP { Task::T1x0 }'), 0);
  assert.equal(countCitations('type Pair<A, B> = [A, B]; // ATT32 BT7'), 0);
  assert.equal(countCitations(''), 0);
});

test('a file that cites nothing is green, and one that starts to is red', () => {
  assert.deepEqual(judge(counts({ 'a.rs': 0 }), baseline({})), []);
  const [error] = judge(counts({ 'a.rs': 1 }), baseline({}));
  assert.match(error, /^a\.rs: has 1 task citation and the baseline allows none — say the reason itself/);
});

test('a baselined file at its entry is green; grown is red; shrunk or gone is red until --shrink', () => {
  assert.deepEqual(judge(counts({ 'main.rs': 40 }), baseline({ 'main.rs': 40 })), []);
  assert.match(judge(counts({ 'main.rs': 41 }), baseline({ 'main.rs': 40 }))[0],
    /has 41 task citations; the baseline allows 40 — it may not grow/);
  assert.match(judge(counts({ 'main.rs': 39 }), baseline({ 'main.rs': 40 }))[0],
    /the baseline still says 40 — run node scripts\/check-task-citations\.mjs --shrink/);
  assert.match(judge(counts({}), baseline({ 'gone.rs': 3 }))[0], /^gone\.rs: has 0 task citations; .*--shrink/);
});

test('--shrink lowers and removes entries and never raises one', () => {
  const before = baseline({ 'grew.rs': 5, 'shrank.rs': 9, 'clean.rs': 2, 'gone.rs': 4 });
  const after = shrunk(counts({ 'grew.rs': 7, 'shrank.rs': 6, 'clean.rs': 0, 'new.rs': 3 }), before);
  assert.deepEqual(after, baseline({ 'grew.rs': 5, 'shrank.rs': 6 }));
  assert.match(judge(counts({ 'grew.rs': 7 }), after)[0], /may not grow/);
});

test('the baseline is written sorted, with a trailing newline', () => {
  const text = renderBaseline(baseline({ 'b.rs': 2, 'a.rs': 3 }));
  assert.equal(text, '{\n  "files": {\n    "a.rs": 3,\n    "b.rs": 2\n  }\n}\n');
});

test('the checked-in baseline holds only files that cite something and is written as --shrink writes it', () => {
  const text = readFileSync(new URL(`../${BASELINE}`, import.meta.url), 'utf8');
  const checkedIn = JSON.parse(text);
  assert.equal(text, renderBaseline(checkedIn));
  for (const [file, count] of Object.entries(checkedIn.files)) {
    assert.ok(Number.isInteger(count) && count > 0, `${file} is in the baseline at ${count}`);
    assert.ok(isChecked(file), `${file} is in the baseline but is not a file the check reads`);
  }
});
