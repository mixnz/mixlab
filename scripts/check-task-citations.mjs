#!/usr/bin/env node
// Keep roadmap task numbers (`T182f`, "roadmap task T32") out of source files.
//
//   node scripts/check-task-citations.mjs
//
// A number in a comment sends the reader to another file, and goes wrong when the task is split,
// lettered or moved; the rule is in docs/standards/rust.md, *Comments*. Every one fails the check,
// printed as `file:line: T182f`, and the way out is to say the reason itself.
//
// Every occurrence in the file is counted, not only those in comments: a task number in a test's
// name is the same pointer elsewhere, and telling a comment from a string in five languages is a
// parser this check does not need.
//
// Run by scripts/gate.sh, .githooks/pre-commit and CI's `lint` job; tested by
// check-task-citations.test.mjs.

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { isCounted } from './check-file-sizes.mjs';

// The scripts whose job is to read the roadmap, where a task number is data and not a citation.
const READERS_OF_THE_ROADMAP = new Set([
  'scripts/check-docs.mjs',
  'scripts/check-docs.test.mjs',
  'scripts/check-task-citations.mjs',
  'scripts/check-task-citations.test.mjs',
]);

// `T` and a number, with the letters a split task takes: T32, T182f, T4 of "T4.4".
const CITATION = /\bT\d+[a-z]*\b/g;

export function isChecked(file) {
  return isCounted(file) && !READERS_OF_THE_ROADMAP.has(file);
}

/** Every citation in `text`, in order, with the line (from 1) it sits on. */
export function findCitations(text) {
  return text.split('\n').flatMap((content, index) =>
    [...content.matchAll(CITATION)].map(([token]) => ({ line: index + 1, token })));
}

function main() {
  process.chdir(execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim());
  const files = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8', maxBuffer: 256 << 20 })
    .split('\0')
    .filter((file) => file && isChecked(file) && existsSync(file));
  const findings = files.flatMap((file) =>
    findCitations(readFileSync(file, 'utf8')).map(({ line, token }) => `${file}:${line}: ${token}`));
  if (findings.length === 0) {
    console.log('task citations: ok');
    return;
  }
  console.error(findings.join('\n'));
  console.error('\nsay the reason itself, or link the ADR or spec (docs/standards/rust.md, *Comments*)');
  console.error(`task citations: ${findings.length.toLocaleString('en-US')} found`);
  process.exitCode = 1;
}

// By basename, as check-docs.mjs does: a drive letter's case can differ on Windows.
if (path.basename(process.argv[1] ?? '') === 'check-task-citations.mjs') main();
