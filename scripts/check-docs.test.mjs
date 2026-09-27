// Tests for scripts/check-docs.mjs:  node --test scripts/check-docs.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  parseFrontMatter, checkSpecHeader, roadmapTicks, taskState, checkSpecAgainstRoadmap,
  stripCode, linkTargets, targetPath, rootedMentions, renderSpecIndex, isIgnoredFile,
  checkMixLabSection,
} from './check-docs.mjs';

const specs = new Set(['2026-08-22-t40a-elevation-design.md']);
const adrs = new Set(['0043']);
const header = (lines) => `---\n${lines.join('\n')}\n---\n\n# Title\n`;

test('front-matter: fields, a list, and the body after it', () => {
  const parsed = parseFrontMatter('---\nstatus: implemented\ndate: 2026-08-25\ntask:\n  - T51\n  - T52\n---\n\n# Title\n');
  assert.deepEqual(parsed.fields, { status: 'implemented', date: '2026-08-25', task: ['T51', 'T52'] });
  assert.equal(parsed.body, '\n# Title\n');
});

test('front-matter: absent is null, unclosed is an error', () => {
  assert.equal(parseFrontMatter('# Title\n'), null);
  assert.match(parseFrontMatter('---\nstatus: draft\n# Title\n').error, /not closed/);
});

test('a valid header has no errors', () => {
  const text = header(['status: implemented', 'date: 2026-08-25', 'task: T51']);
  assert.deepEqual(checkSpecHeader('2026-08-25-t51-x-design.md', text, specs, adrs), []);
});

test('a missing header, a bad status, a wrong date and an unknown field are errors', () => {
  const name = '2026-08-25-t51-x-design.md';
  assert.match(checkSpecHeader(name, '# Title\n', specs, adrs)[0], /no front-matter/);
  assert.match(checkSpecHeader(name, header(['status: done', 'date: 2026-08-25']), specs, adrs)[0], /status must be one of/);
  assert.match(checkSpecHeader(name, header(['status: draft', 'date: 2026-08-26']), specs, adrs)[0], /differs from the filename/);
  assert.match(checkSpecHeader(name, header(['status: draft', 'date: 2026-08-25', 'owner: me']), specs, adrs)[0], /unknown field `owner`/);
});

test('superseded needs a replacement that exists', () => {
  const name = '2026-08-22-t40-elevate-design.md';
  const check = (extra) => checkSpecHeader(name, header(['status: superseded', 'date: 2026-08-22', ...extra]), specs, adrs);
  assert.match(check([])[0], /without superseded_by/);
  assert.match(check(['superseded_by: 2026-01-01-nothing-design.md'])[0], /does not exist/);
  assert.match(check(['superseded_by: ADR 0099'])[0], /does not exist/);
  assert.deepEqual(check(['superseded_by: 2026-08-22-t40a-elevation-design.md']), []);
  assert.deepEqual(check(['superseded_by: ADR 0043']), []);
  const notSuperseded = header(['status: implemented', 'date: 2026-08-22', 'superseded_by: ADR 0043']);
  assert.match(checkSpecHeader(name, notSuperseded, specs, adrs)[0], /superseded_by on a spec whose status is implemented/);
});

const ticks = roadmapTicks([
  '- [x] **T51** a\n- [ ] **T33b** b\n- [x] **T33** c\n- [x] **T168a** d\n- [x] **T168b** e\n',
  '- [~] **T86a** f\n  - [x] **T90** indented\n- [x] **T12** g\n',
]);

test('ticks are read, indented or not', () => {
  assert.equal(ticks.get('T51'), 'x');
  assert.equal(ticks.get('T86a'), '~');
  assert.equal(ticks.get('T90'), 'x');
});

test('task state: the exact id first, then its lettered sub-tasks', () => {
  assert.equal(taskState('T33', ticks), 'done'); // an open T33b does not reopen T33
  assert.equal(taskState('T168', ticks), 'done');
  assert.equal(taskState('T86a', ticks), 'open');
  assert.equal(taskState('T1', ticks), 'unknown'); // T12 is not a sub-task of T1
  assert.equal(taskState('T999', ticks), 'unknown');
});

test('status and roadmap must agree', () => {
  const agree = (fields) => checkSpecAgainstRoadmap('s.md', fields, ticks);
  assert.deepEqual(agree({ status: 'implemented', task: 'T51' }), []);
  assert.deepEqual(agree({ status: 'implemented', task: ['T51', 'T168'] }), []);
  assert.match(agree({ status: 'approved', task: 'T51' })[0], /is implemented, not approved/);
  assert.match(agree({ status: 'implemented', task: 'T86a' })[0], /T86a is still open/);
  assert.match(agree({ status: 'approved', task: 'T999' })[0], /T999 is on no roadmap line/);
  assert.deepEqual(agree({ status: 'approved', task: 'T86a' }), []);
  assert.deepEqual(agree({ status: 'superseded', task: 'T86a' }), []);
  assert.deepEqual(agree({ status: 'implemented' }), []);
});

test('code is stripped before links are read', () => {
  assert.equal(stripCode('a `[x](y)` b\n```\n[z](w)\n```\nc'), 'a  b\n\nc');
});

test('link targets skip schemes, bare anchors and absolute paths, and drop titles', () => {
  const text = '[a](../x.md#h) [b](https://e.com) [c](#top) [d](mailto:a@b) [f](/abs) [g](k.md "title")';
  assert.deepEqual(linkTargets(text), ['../x.md#h', 'k.md']);
});

test('a target loses its anchor and its line suffix', () => {
  assert.equal(targetPath('../a.rs:120'), '../a.rs');
  assert.equal(targetPath('../a.rs:311-323'), '../a.rs');
  assert.equal(targetPath('b.md#part'), 'b.md');
  assert.equal(targetPath('c%20d.md'), 'c d.md');
});

test('rooted mentions: backticks and sentence ends; not relative paths, home paths or placeholders', () => {
  const text = 'see `.claude/features/tls.md`. And docs/specs/a-design.md. Not ../docs/x.md, '
    + '~/.claude/y.md, docs/specs/YYYY-MM-DD-slug-design.md, docs/specs/2026-09-05-....md '
    + 'docs/packages/redis-memcached.md or docs/the-archive.md.';
  assert.deepEqual(rootedMentions(text), ['.claude/features/tls.md', 'docs/specs/a-design.md']);
});

test('the index lists each spec with its task and status, escaping pipes', () => {
  const out = renderSpecIndex([
    { name: '2026-08-22-t40-e-design.md', title: 'T40', status: 'superseded', task: 'T40', supersededBy: '2026-08-22-t40a-elevation-design.md' },
    { name: '2026-08-25-t51-x-design.md', title: 'T51 — a | b', status: 'implemented', task: 'T51' },
    { name: '2026-08-26-y-design.md', title: 'Y', status: 'superseded', task: '', supersededBy: 'ADR 0043' },
  ]);
  assert.match(out, /^\| 2026-08-25 \| \[T51 — a \\\| b\]\(2026-08-25-t51-x-design\.md\) \| T51 \| implemented \|$/m);
  assert.match(out, /superseded by \[2026-08-22-t40a-elevation-design\.md\]\(2026-08-22-t40a-elevation-design\.md\)/);
  assert.match(out, /\| superseded by ADR 0043 \|$/m);
  assert.ok(out.endsWith('\n'));
});

test('a shipped migration is never read: its bytes are a checksum every user database holds', () => {
  assert.equal(isIgnoredFile('crates/mixengine-core/migrations/0001_initial.sql'), true);
  assert.equal(isIgnoredFile('crates/mixengine-daemon/migrations/0002_x.sql'), true);
  assert.equal(isIgnoredFile('crates/mixengine-core/migrations-archive/0007_x.sql'), true);
  assert.equal(isIgnoredFile('crates/mixengine-core/src/store.rs'), false);
  assert.equal(isIgnoredFile('docs/architecture/data-model.md'), false);
  assert.equal(isIgnoredFile('scripts/check-docs.mjs'), true);
});
test('a spec from the MixLab rule on must say what the window does', () => {
  const name = '2026-09-28-t190-x-design.md';
  const without = header(['status: draft', 'date: 2026-09-28']);
  assert.match(checkMixLabSection(name, without)[0], /## MixLab/);

  const withIt = header(['status: draft', 'date: 2026-09-28']) + '\n## MixLab\n\nThe Sites screen gains a Stop button.\n';
  assert.deepEqual(checkMixLabSection(name, withIt), []);
});

test('an older spec, or one already implemented, is not asked', () => {
  assert.deepEqual(checkMixLabSection('2026-09-20-t180-x-design.md', header(['status: draft', 'date: 2026-09-20'])), []);
  assert.deepEqual(
    checkMixLabSection('2026-09-28-t190-x-design.md', header(['status: implemented', 'date: 2026-09-28'])),
    [],
  );
});
