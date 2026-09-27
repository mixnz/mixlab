#!/usr/bin/env node
// Finds Rust string literals whose `\` line continuation went missing.
//
//   node scripts/check-strings.mjs
//
// A string written across two lines as
//
//     "... it has no line \
//      in the hosts file"
//
// reads with one space. Lose the `\` (or have it turn into a literal `\n`) and the two lines become
// one, carrying the second line's indentation into the message: "it has no line              in the
// hosts file". rustfmt keeps such a line as it is and no compiler warns, so it reaches the person
// reading the error. A dozen had, by the time this check was written.
//
// **What counts:** a line longer than rustfmt's `max_width` (100), where a string literal holds a run
// of six or more spaces between two words — optionally after a literal `\n`. The length is the
// tell: a message rustfmt was happy with fits in 100 columns until two of its lines are joined.
// Columns aligned on purpose in a line that long say so with the comment `aligned on purpose`, on
// that line or on the line above it (a line inside a continued string has nowhere else to put it).
//
// Exit status: 0 when nothing is found, 1 when something is, naming each file and line.

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_WIDTH = 100;
const ROOTS = ['crates', 'apps/desktop/src-tauri/src', 'server/native'];
const SKIP = new Set(['target', 'node_modules', '.git']);

// A word's last character, an optional literal `\n`, six or more spaces, then a word's first. A
// format placeholder counts as a word on either side.
const JOINED = /[A-Za-z0-9,;:.'`)}—](?: \\n)? {6,}[A-Za-z`({]/;

// The contents of every string literal on a line, including one the line ends inside.
const LITERALS = /"((?:[^"\\]|\\.)*)"?/g;

/**
 * Whether `line` looks like two source lines of one string joined by a lost continuation.
 * `previous` is the line above it, where the `aligned on purpose` marker may also go.
 */
export function lostContinuation(line, previous = '') {
  if (
    line.length <= MAX_WIDTH ||
    line.includes('aligned on purpose') ||
    previous.includes('aligned on purpose')
  ) {
    return false;
  }

  // The line after a `\` is inside the string that line continues, with no opening quote of its own.
  const scanned = previous.trimEnd().endsWith('\\') ? `"${line.trimStart()}` : line;

  for (const [, contents] of scanned.matchAll(LITERALS)) {
    if (JOINED.test(contents)) {
      return true;
    }
  }

  return false;
}

function* rustFiles(dir) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return;
  }

  for (const name of entries) {
    if (SKIP.has(name)) continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      yield* rustFiles(path);
    } else if (name.endsWith('.rs')) {
      yield path;
    }
  }
}

function main() {
  const root = join(fileURLToPath(import.meta.url), '..', '..');
  const found = [];

  for (const top of ROOTS) {
    for (const file of rustFiles(join(root, top))) {
      const lines = readFileSync(file, 'utf8')
        .split('\n')
        .map((line) => line.replace(/\r$/, ''));

      lines.forEach((line, index) => {
        if (lostContinuation(line, lines[index - 1] ?? '')) {
          found.push(`${relative(root, file).replaceAll('\\', '/')}:${index + 1}`);
        }
      });
    }
  }

  if (found.length === 0) {
    console.log('strings: ok');
    return;
  }

  for (const place of found) {
    console.error(`${place}: a string literal whose \`\\\` line continuation went missing`);
  }
  console.error(`strings: ${found.length} problem(s) — put the \`\\\` back at the end of the first line`);
  process.exit(1);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === join(process.argv[1])) {
  main();
}
