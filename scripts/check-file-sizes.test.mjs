// Tests for scripts/check-file-sizes.mjs:  node --test scripts/check-file-sizes.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { BASELINE, isCounted, countLines, judge, shrunk, renderBaseline } from './check-file-sizes.mjs';

const baseline = (files) => ({ limit: 1000, files });
const sizes = (entries) => new Map(Object.entries(entries));

test('source files are counted; generated, migrations, fixtures and declarations are not', () => {
  for (const file of ['crates/a/src/lib.rs', 'apps/desktop/src/App.tsx', 'apps/desktop/src/x.ts',
    'scripts/check-docs.mjs', 'apps/desktop/src/App.css', 'apps/desktop/vite.config.js']) {
    assert.equal(isCounted(file), true, file);
  }
  for (const file of ['bindings/Action.ts', 'crates/core/.sqlx/query-1.json', 'crates/core/.sqlx/x.rs',
    'crates/core/migrations/0001_initial.rs', 'crates/core/migrations-archive/0007.rs',
    'apps/desktop/demo/fixtures/db.ts', 'apps/desktop/src/vite-env.d.ts', 'docs/guide/en/cli.md',
    'Cargo.toml']) {
    assert.equal(isCounted(file), false, file);
  }
});

test('lines are counted the way wc -l counts them, CRLF included', () => {
  assert.equal(countLines(''), 0);
  assert.equal(countLines('one\ntwo\n'), 2);
  assert.equal(countLines('one\ntwo'), 1);
  assert.equal(countLines('one\r\ntwo\r\n'), 2);
});

test('a file at the limit is green and one past it, not in the baseline, is red', () => {
  assert.deepEqual(judge(sizes({ 'a.rs': 1000 }), baseline({})), []);
  const [error] = judge(sizes({ 'a.rs': 1612 }), baseline({}));
  assert.match(error, /^a\.rs: is 1,612 lines; the limit is 1,000 — split it by capability/);
});

test('a baselined file at its entry is green; grown is red; shrunk is red until --shrink', () => {
  assert.deepEqual(judge(sizes({ 'main.rs': 7651 }), baseline({ 'main.rs': 7651 })), []);
  assert.match(judge(sizes({ 'main.rs': 7660 }), baseline({ 'main.rs': 7651 }))[0],
    /is 7,660 lines; the baseline allows 7,651 — it may not grow/);
  assert.match(judge(sizes({ 'main.rs': 7600 }), baseline({ 'main.rs': 7651 }))[0],
    /the baseline still says 7,651 — run node scripts\/check-file-sizes\.mjs --shrink/);
});

test('a baselined file at or under the limit, or gone, is red until --shrink removes it', () => {
  assert.match(judge(sizes({ 'main.rs': 1000 }), baseline({ 'main.rs': 7651 }))[0], /under the limit now/);
  assert.match(judge(sizes({}), baseline({ 'gone.rs': 2000 }))[0], /^gone\.rs: .*--shrink/);
});

test('--shrink lowers and removes entries and never raises one', () => {
  const before = baseline({ 'grew.rs': 2000, 'shrank.rs': 3000, 'small.rs': 1600, 'gone.rs': 1700 });
  const after = shrunk(sizes({ 'grew.rs': 2100, 'shrank.rs': 2500, 'small.rs': 1000 }), before);
  assert.deepEqual(after, baseline({ 'grew.rs': 2000, 'shrank.rs': 2500 }));
  assert.match(judge(sizes({ 'grew.rs': 2100 }), after)[0], /may not grow/);
});

test('the baseline is written sorted, with a trailing newline', () => {
  const text = renderBaseline(baseline({ 'b.rs': 2000, 'a.rs': 3000 }));
  assert.equal(text, '{\n  "limit": 1000,\n  "files": {\n    "a.rs": 3000,\n    "b.rs": 2000\n  }\n}\n');
});

test('the checked-in baseline holds files to 1,000 lines and is written as --shrink writes it', () => {
  const text = readFileSync(new URL(`../${BASELINE}`, import.meta.url), 'utf8');
  const checkedIn = JSON.parse(text);
  assert.equal(checkedIn.limit, 1000);
  assert.equal(text, renderBaseline(checkedIn));
  for (const [file, lines] of Object.entries(checkedIn.files)) {
    assert.ok(lines > checkedIn.limit, `${file} is in the baseline at ${lines}, not past the limit`);
  }
});
