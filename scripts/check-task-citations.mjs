#!/usr/bin/env node
// Keep roadmap task numbers (`T182f`, "roadmap task T32") out of source files: no file that cites
// none starts to, and none that already does cites more.
//
//   node scripts/check-task-citations.mjs            check
//   node scripts/check-task-citations.mjs --shrink   lower or remove the baseline entries the tree
//                                                    has outgrown, then check
//
// A number in a comment sends the reader to another file, and goes wrong when the task is split,
// lettered or moved; the rule is in docs/standards/rust.md, *Comments*. The citations that predate
// it are not rewritten in bulk. They go when the code around them next changes, and --shrink writes
// that down in scripts/task-citation-baseline.json, so a citation one commit removed cannot be
// spent again by the next. Nothing here raises an entry: citing more is a hand edit to the JSON,
// and that line in the diff is the question for review.
//
// Every occurrence in the file is counted, not only those in comments: a task number in a test's
// name is the same pointer elsewhere, and telling a comment from a string in five languages is a
// parser this check does not need.
//
// Run by scripts/gate.sh and CI's `lint` job; tested by check-task-citations.test.mjs.

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { isCounted } from './check-file-sizes.mjs';

export const BASELINE = 'scripts/task-citation-baseline.json';

// The scripts whose job is to read the roadmap, where a task number is data and not a citation.
const READERS_OF_THE_ROADMAP = new Set([
  'scripts/check-docs.mjs',
  'scripts/check-docs.test.mjs',
  'scripts/check-task-citations.mjs',
  'scripts/check-task-citations.test.mjs',
]);

// `T` and a number, with the letters a split task takes: T32, T182f, T4 of "T4.4".
const CITATION = /\bT\d+[a-z]*\b/g;

const SHRINK = 'run node scripts/check-task-citations.mjs --shrink';

export function isChecked(file) {
  return isCounted(file) && !READERS_OF_THE_ROADMAP.has(file);
}

export function countCitations(text) {
  return text.match(CITATION)?.length ?? 0;
}

const total = (files) => Object.values(files).reduce((sum, count) => sum + count, 0);
const number = (value) => value.toLocaleString('en-US');
const citations = (count) => `${number(count)} task citation${count === 1 ? '' : 's'}`;

/** Every way `counts` breaks `baseline`, one sentence each, sorted. */
export function judge(counts, baseline) {
  const { files } = baseline;
  const errors = [];
  for (const [file, count] of counts) {
    if (count > 0 && !(file in files)) {
      errors.push(`${file}: has ${citations(count)} and the baseline allows none — say the reason itself, or link the ADR or spec (docs/standards/rust.md, *Comments*)`);
    }
  }
  for (const [file, allowed] of Object.entries(files)) {
    const count = counts.get(file) ?? 0;
    if (count > allowed) {
      errors.push(`${file}: has ${citations(count)}; the baseline allows ${number(allowed)} — it may not grow`);
    } else if (count < allowed) {
      errors.push(`${file}: has ${citations(count)}; the baseline still says ${number(allowed)} — ${SHRINK}`);
    }
  }
  return errors.sort();
}

/** `baseline` with every entry lowered to its file's count, and the files that cite nothing gone. */
export function shrunk(counts, baseline) {
  const files = {};
  for (const [file, allowed] of Object.entries(baseline.files)) {
    const count = Math.min(counts.get(file) ?? 0, allowed);
    if (count > 0) files[file] = count;
  }
  return { files };
}

export function renderBaseline(baseline) {
  const files = Object.fromEntries(Object.keys(baseline.files).sort().map((file) => [file, baseline.files[file]]));
  return `${JSON.stringify({ files }, null, 2)}\n`;
}

function main(argv) {
  process.chdir(execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim());
  const files = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8', maxBuffer: 256 << 20 })
    .split('\0')
    .filter((file) => file && isChecked(file) && existsSync(file));
  const counts = new Map(files.map((file) => [file, countCitations(readFileSync(file, 'utf8'))]));
  let baseline = JSON.parse(readFileSync(BASELINE, 'utf8'));
  if (argv.includes('--shrink')) {
    baseline = shrunk(counts, baseline);
    writeFileSync(BASELINE, renderBaseline(baseline));
  }
  const errors = judge(counts, baseline);
  if (errors.length === 0) {
    const entries = Object.keys(baseline.files).length;
    console.log(`task citations: ok (${number(total(baseline.files))} in ${number(entries)} files, none grown)`);
    return;
  }
  console.error(errors.join('\n'));
  console.error(`\ntask citations: ${errors.length} problem(s)`);
  process.exitCode = 1;
}

// By basename, as check-docs.mjs does: a drive letter's case can differ on Windows.
if (path.basename(process.argv[1] ?? '') === 'check-task-citations.mjs') main(process.argv.slice(2));
