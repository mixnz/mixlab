#!/usr/bin/env node
// Check the documentation: every link into it resolves, every spec opens with a status header that
// agrees with the roadmap, docs/specs/README.md is what this script generates, and no document
// lives under .claude/ or docs/superpowers/.
//
//   node scripts/check-docs.mjs                  check everything
//   node scripts/check-docs.mjs --only=links     links and layout only
//   node scripts/check-docs.mjs --only=specs     spec headers and the index only
//   node scripts/check-docs.mjs --write-index    regenerate docs/specs/README.md, then check
//
// Run by the `lint` CI job; tested by check-docs.test.mjs. Design:
// docs/specs/2026-09-19-t169-one-home-for-the-documentation-design.md

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const P = path.posix;

export const STATUSES = ['draft', 'approved', 'implemented', 'superseded', 'abandoned'];
const FIELDS = new Set(['status', 'date', 'task', 'superseded_by']);
const TEXT_EXTENSIONS = new Set([
  '.md', '.rs', '.ts', '.tsx', '.js', '.mjs', '.cjs', '.json', '.toml', '.yml', '.yaml', '.sh',
  '.ps1', '.txt', '.css', '.html', '.sql',
]);

// Paths that look like this repository's documentation and are not: mixengine-packages' own
// docs/, which the runtime-packaging pages, reviews and specs cite by its repository-rooted path.
export const IGNORED_MENTION_PREFIXES = ['docs/packages/'];
export const IGNORED_MENTIONS = new Set([
  'docs/building-from-source.md',
  'docs/adding-a-version.md',
  'docs/the-archive.md',
  'docs/roadmap.md',
  // The standalone client's own tree, cited by the records written there (a desktop decision and a review).
  'docs/RELEASING.md',
]);

// Files that name the layout before T169 on purpose, and this check with its examples.
export const IGNORED_FILES = new Set([
  'scripts/check-docs.mjs',
  'scripts/check-docs.test.mjs',
  'docs/specs/2026-09-19-t169-one-home-for-the-documentation-design.md',
  'docs/decisions/0043-documentation-lives-under-docs.md',
]);

// **A shipped migration is never read.** sqlx checksums the whole file, comments included, and every
// user's database holds that checksum: T169 rewrote a path in the comments of two of them, and a
// daemon on an existing home refused to start. A path in one of these is a record of when it was
// written, not a link to keep current. The same goes for `migrations-archive/`, which holds the ones
// v0.0.7 folded into one: kept as they were, for the reasoning in them.
const IGNORED_FILE_PATTERNS = [/^crates\/[^/]+\/migrations(-archive)?\/[^/]+\.sql$/];

export function isIgnoredFile(file) {
  return IGNORED_FILES.has(file) || IGNORED_FILE_PATTERNS.some((pattern) => pattern.test(file));
}

// A mention holding one of these is a pattern, not a path.
const PLACEHOLDER = /YYYY|NNNN|\.\.\.\./;

export function parseFrontMatter(text) {
  if (!text.startsWith('---\n')) return null;
  const end = text.indexOf('\n---\n', 3);
  if (end < 0) return { error: 'front-matter is not closed by a `---` line' };
  const fields = {};
  let listKey = null;
  for (const line of text.slice(4, end).split('\n')) {
    if (line.trim() === '') continue;
    const item = line.match(/^\s*-\s+(.+)$/);
    if (item && listKey) {
      fields[listKey].push(item[1].trim());
      continue;
    }
    const pair = line.match(/^([a-z_]+):\s*(.*)$/);
    if (!pair) return { error: `cannot read front-matter line \`${line}\`` };
    const [, key, value] = pair;
    if (value.trim() === '') {
      fields[key] = [];
      listKey = key;
    } else {
      fields[key] = value.trim();
      listKey = null;
    }
  }
  return { fields, body: text.slice(end + 5) };
}

export function checkSpecHeader(name, text, specNames, adrNumbers) {
  const parsed = parseFrontMatter(text);
  if (!parsed) return [`${name}: no front-matter; a spec opens with its status and date`];
  if (parsed.error) return [`${name}: ${parsed.error}`];
  const { fields } = parsed;
  const errors = [];
  for (const key of Object.keys(fields)) {
    if (!FIELDS.has(key)) errors.push(`${name}: unknown field \`${key}\``);
  }
  if (!STATUSES.includes(fields.status)) {
    errors.push(`${name}: status must be one of ${STATUSES.join(', ')}; found \`${fields.status ?? ''}\``);
  }
  const dateInName = name.slice(0, 10);
  if (fields.date !== dateInName) {
    errors.push(`${name}: date \`${fields.date ?? ''}\` differs from the filename's ${dateInName}`);
  }
  const by = fields.superseded_by;
  if (fields.status === 'superseded') {
    const adr = typeof by === 'string' ? by.match(/^ADR (\d{4})$/) : null;
    if (!by) errors.push(`${name}: superseded without superseded_by`);
    else if (adr ? !adrNumbers.has(adr[1]) : !specNames.has(by)) {
      errors.push(`${name}: superseded_by names \`${by}\`, which does not exist`);
    }
  } else if (by) {
    errors.push(`${name}: superseded_by on a spec whose status is ${fields.status}`);
  }
  return errors;
}

// The first day a spec had to say what MixLab does. MixLab is the product (CLAUDE.md, ADR 0056), and
// a design that answers only for the daemon and `mix` ships a feature a person in the window cannot
// reach — T182f did, with `mix runtime adopt` and no button. Earlier specs are left as they are.
const MIXLAB_SECTION_FROM = '2026-09-27';

/** A spec from `MIXLAB_SECTION_FROM` on that is not yet built must have a `## MixLab` section. */
export function checkMixLabSection(name, text) {
  const parsed = parseFrontMatter(text);
  if (!parsed || parsed.error) return [];
  const { fields, body } = parsed;
  if ((fields.date ?? '') < MIXLAB_SECTION_FROM) return [];
  if (!['draft', 'approved'].includes(fields.status)) return [];
  if (/^## MixLab\b/m.test(body)) return [];
  return [
    `${name}: no \`## MixLab\` section — say which screen does this and how, or why the window has no part in it`,
  ];
}

export function roadmapTicks(texts) {
  const ticks = new Map();
  for (const text of texts) {
    for (const [, mark, id] of text.matchAll(/^\s*- \[([ x~])\] \*\*(T\d+[a-z]?)\*\*/gm)) ticks.set(id, mark);
  }
  return ticks;
}

// A task is its exact line when there is one; otherwise every lettered sub-task (T168a, T168b…).
export function taskState(id, ticks) {
  const exact = ticks.get(id);
  const marks = exact !== undefined
    ? [exact]
    : [...ticks].filter(([key]) => key.length === id.length + 1 && key.startsWith(id) && /[a-z]$/.test(key))
      .map(([, mark]) => mark);
  if (marks.length === 0) return 'unknown';
  return marks.every((mark) => mark === 'x') ? 'done' : 'open';
}

export function checkSpecAgainstRoadmap(name, fields, ticks) {
  const tasks = [].concat(fields.task ?? []);
  if (tasks.length === 0) return [];
  const states = tasks.map((id) => taskState(id, ticks));
  const unknown = tasks.filter((_, i) => states[i] === 'unknown');
  if (unknown.length) return [`${name}: task ${unknown.join(', ')} is on no roadmap line`];
  const open = tasks.filter((_, i) => states[i] === 'open');
  if ((fields.status === 'draft' || fields.status === 'approved') && open.length === 0) {
    return [`${name}: every task is ticked, so the status is implemented, not ${fields.status}`];
  }
  if (fields.status === 'implemented' && open.length) {
    return [`${name}: status implemented, but ${open.join(', ')} is still open on the roadmap`];
  }
  return [];
}

export function stripCode(text) {
  return text.replace(/^```[\s\S]*?^```/gm, '').replace(/`[^`\n]*`/g, '');
}

export function linkTargets(text) {
  const targets = [];
  for (const [, target] of text.matchAll(/\]\(\s*([^)\s]+)(?:\s+"[^"]*")?\s*\)/g)) {
    if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#') || target.startsWith('/')) continue;
    targets.push(target);
  }
  return targets;
}

export function targetPath(raw) {
  return decodeURI(raw.replace(/#.*$/, '').replace(/:\d+(?:-\d+)?$/, ''));
}

export function rootedMentions(text) {
  const found = [];
  for (const [mention] of text.matchAll(/(?<![\w./-])(?:docs|\.claude)\/[\w./-]+\.md(?![\w-])/g)) {
    if (PLACEHOLDER.test(mention) || IGNORED_MENTIONS.has(mention)) continue;
    if (IGNORED_MENTION_PREFIXES.some((prefix) => mention.startsWith(prefix))) continue;
    found.push(mention);
  }
  return found;
}

export function renderSpecIndex(entries) {
  const cell = (text) => text.replace(/\|/g, '\\|');
  const lines = [
    '# Specs',
    '',
    'Every design, oldest first, with the status its header states. Generated by',
    "`node scripts/check-docs.mjs --write-index`: edit a spec's header, not this file. What each",
    'status means: [plans-and-specs.md](../standards/plans-and-specs.md).',
    '',
    '| Date | Spec | Task | Status |',
    '| --- | --- | --- | --- |',
  ];
  for (const entry of entries) {
    const by = !entry.supersededBy ? ''
      : /^ADR /.test(entry.supersededBy) ? ` by ${entry.supersededBy}`
        : ` by [${entry.supersededBy}](${entry.supersededBy})`;
    lines.push(`| ${entry.name.slice(0, 10)} | [${cell(entry.title)}](${entry.name}) | ${entry.task} | ${entry.status}${by} |`);
  }
  return `${lines.join('\n')}\n`;
}

function isText(file) {
  return TEXT_EXTENSIONS.has(P.extname(file));
}

function checkLayoutAndLinks(files) {
  const errors = [];
  for (const file of files) {
    if (file.startsWith('docs/superpowers/')) {
      errors.push(`${file}: specs go to docs/specs/ and plans to docs/plans/ (docs/standards/plans-and-specs.md)`);
    }
    if (/^\.claude\/.+\.md$/.test(file) && !/^\.claude\/(README\.md|commands\/.+|skills\/.+)$/.test(file)) {
      errors.push(`${file}: documentation lives under docs/, not .claude/ (ADR 0043)`);
    }
    if (!isText(file) || isIgnoredFile(file)) continue;
    const raw = readFileSync(file, 'utf8');
    const isMarkdown = file.endsWith('.md');
    for (const target of linkTargets(isMarkdown ? stripCode(raw) : raw)) {
      let resolved;
      try {
        resolved = P.normalize(P.join(P.dirname(file), targetPath(target))).replace(/\/$/, '');
      } catch {
        errors.push(`${file}: unreadable link ${target}`);
        continue;
      }
      if (resolved === '..' || resolved.startsWith('../')) continue;
      if (!isMarkdown && !/^(docs|\.claude)(\/|$)/.test(resolved)) continue;
      if (/^docs\/plans\/./.test(resolved)) errors.push(`${file}: links into docs/plans/, which exists on one machine only (${target})`);
      else if (!existsSync(resolved)) errors.push(`${file}: dead link ${target}`);
    }
    for (const mention of rootedMentions(raw)) {
      if (mention.startsWith('docs/plans/')) errors.push(`${file}: names ${mention}, which exists on one machine only`);
      else if (!existsSync(mention)) errors.push(`${file}: names ${mention}, which does not exist`);
    }
  }
  return errors;
}

function checkSpecs(files, writeIndex) {
  const specNames = files
    .filter((file) => /^docs\/specs\/\d{4}-\d{2}-\d{2}-.+\.md$/.test(file))
    .map((file) => P.basename(file))
    .sort();
  const nameSet = new Set(specNames);
  const adrNumbers = new Set(files.map((file) => file.match(/^docs\/decisions\/(\d{4})-/)?.[1]).filter(Boolean));
  const ticks = roadmapTicks(files.filter((file) => /^docs\/roadmap\/.+\.md$/.test(file)).map((file) => readFileSync(file, 'utf8')));
  const errors = [];
  const entries = [];
  for (const name of specNames) {
    const text = readFileSync(`docs/specs/${name}`, 'utf8');
    const headerErrors = checkSpecHeader(name, text, nameSet, adrNumbers);
    errors.push(...headerErrors);
    if (headerErrors.length) continue;
    const { fields, body } = parseFrontMatter(text);
    errors.push(...checkSpecAgainstRoadmap(name, fields, ticks));
    errors.push(...checkMixLabSection(name, text));
    entries.push({
      name,
      title: body.match(/^# (.+)$/m)?.[1] ?? name,
      status: fields.status,
      task: [].concat(fields.task ?? []).join(', '),
      supersededBy: fields.superseded_by,
    });
  }
  const indexPath = 'docs/specs/README.md';
  const index = renderSpecIndex(entries);
  if (writeIndex) writeFileSync(indexPath, index);
  else if (!existsSync(indexPath) || readFileSync(indexPath, 'utf8') !== index) {
    errors.push(`${indexPath}: out of date; run node scripts/check-docs.mjs --write-index`);
  }
  return errors;
}

function main(argv) {
  const only = argv.find((arg) => arg.startsWith('--only='))?.slice('--only='.length);
  const writeIndex = argv.includes('--write-index');
  process.chdir(execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim());
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { encoding: 'utf8', maxBuffer: 256 << 20 })
    .split('\0')
    .filter((file) => file && existsSync(file));
  // Specs first: --write-index has to write docs/specs/README.md before links to it are resolved.
  const specErrors = only === 'links' ? [] : checkSpecs(files, writeIndex);
  const linkErrors = only === 'specs' ? [] : checkLayoutAndLinks(files);
  const errors = [...linkErrors, ...specErrors];
  if (errors.length === 0) {
    console.log('docs: ok');
    return;
  }
  console.error(errors.join('\n'));
  console.error(`\ndocs: ${errors.length} problem(s)`);
  process.exitCode = 1;
}

// By basename, not by URL: a drive letter's case can differ between the two on Windows, and a
// check that silently does not run would pass everything. The test file is check-docs.test.mjs.
if (path.basename(process.argv[1] ?? '') === 'check-docs.mjs') main(process.argv.slice(2));
