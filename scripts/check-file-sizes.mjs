#!/usr/bin/env node
// Keep every source file short enough to read in one go: none grows past the limit, and none
// already past it grows at all.
//
//   node scripts/check-file-sizes.mjs            check
//   node scripts/check-file-sizes.mjs --shrink   lower or remove the baseline entries the tree has
//                                                outgrown, then check
//
// scripts/file-size-baseline.json names the files that were over the limit when the check landed,
// each at its length then. An entry only ever goes down: a file that shrinks has its entry lowered
// by --shrink, so the lines a commit removed cannot be spent again by the next one, and a file back
// under the limit leaves the list. Nothing here raises an entry. Growing a file past its entry is a
// hand edit to the JSON, and that line in the diff is the question for review.
//
// Run by scripts/gate.sh and CI's `lint` job; tested by check-file-sizes.test.mjs. Design:
// docs/specs/2026-10-13-files-that-fit-in-one-read-design.md

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

export const BASELINE = 'scripts/file-size-baseline.json';

const EXTENSIONS = /\.(rs|ts|tsx|js|mjs|css)$/;

// Generated (bindings/, .sqlx/), never edited once shipped (migrations), or data written as code
// (fixtures/). A declaration file is the compiler's, not a module anyone reads.
const EXCLUDED = [
  /^bindings\//,
  /(^|\/)\.sqlx\//,
  /(^|\/)migrations(-archive)?\//,
  /(^|\/)fixtures\//,
  /\.d\.ts$/,
];

const SHRINK = 'run node scripts/check-file-sizes.mjs --shrink';

export function isCounted(file) {
  return EXTENSIONS.test(file) && !EXCLUDED.some((pattern) => pattern.test(file));
}

/** Newline characters, which is what `wc -l` counts: a last line without one is not counted. */
export function countLines(text) {
  return text.split('\n').length - 1;
}

const number = (value) => value.toLocaleString('en-US');

/** Every way `sizes` breaks `baseline`, one sentence each, sorted. */
export function judge(sizes, baseline) {
  const { limit, files } = baseline;
  const errors = [];
  for (const [file, lines] of sizes) {
    if (lines > limit && !(file in files)) {
      errors.push(`${file}: is ${number(lines)} lines; the limit is ${number(limit)} — split it by capability (docs/standards/source-files.md)`);
    }
  }
  for (const [file, allowed] of Object.entries(files)) {
    const lines = sizes.get(file);
    if (lines === undefined || lines <= limit) {
      errors.push(`${file}: is under the limit now, or gone — ${SHRINK}`);
    } else if (lines > allowed) {
      errors.push(`${file}: is ${number(lines)} lines; the baseline allows ${number(allowed)} — it may not grow`);
    } else if (lines < allowed) {
      errors.push(`${file}: is ${number(lines)} lines; the baseline still says ${number(allowed)} — ${SHRINK}`);
    }
  }
  return errors.sort();
}

/** `baseline` with every entry lowered to its file's length, and the files under the limit gone. */
export function shrunk(sizes, baseline) {
  const files = {};
  for (const [file, allowed] of Object.entries(baseline.files)) {
    const lines = sizes.get(file);
    if (lines === undefined || lines <= baseline.limit) continue;
    files[file] = Math.min(lines, allowed);
  }
  return { limit: baseline.limit, files };
}

export function renderBaseline(baseline) {
  const files = Object.fromEntries(Object.keys(baseline.files).sort().map((file) => [file, baseline.files[file]]));
  return `${JSON.stringify({ limit: baseline.limit, files }, null, 2)}\n`;
}

function main(argv) {
  process.chdir(execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim());
  const files = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8', maxBuffer: 256 << 20 })
    .split('\0')
    .filter((file) => file && isCounted(file) && existsSync(file));
  const sizes = new Map(files.map((file) => [file, countLines(readFileSync(file, 'utf8'))]));
  let baseline = JSON.parse(readFileSync(BASELINE, 'utf8'));
  if (argv.includes('--shrink')) {
    baseline = shrunk(sizes, baseline);
    writeFileSync(BASELINE, renderBaseline(baseline));
  }
  const errors = judge(sizes, baseline);
  if (errors.length === 0) {
    console.log(`file sizes: ok (${Object.keys(baseline.files).length} over the limit, none grown)`);
    return;
  }
  console.error(errors.join('\n'));
  console.error(`\nfile sizes: ${errors.length} problem(s)`);
  process.exitCode = 1;
}

// By basename, as check-docs.mjs does: a drive letter's case can differ on Windows.
if (path.basename(process.argv[1] ?? '') === 'check-file-sizes.mjs') main(process.argv.slice(2));
