#!/usr/bin/env node
/**
 * Converts CRLF to LF in the files git tracks.
 *
 * It does **not** touch the content git stores: the repo's `.gitattributes` is
 * `* text=auto eol=lf`, so git already normalises to LF on the way into the index. What this script
 * fixes is the **working tree** — the files on disk — so editors and every tool reading files
 * directly see the same thing, and so git stops warning "CRLF will be replaced by LF" on every
 * `git add`.
 *
 * CRLF gets in mostly through tools writing files in text mode on Windows — Python's
 * `open(p, "w")` is the classic example; it translates every `\n` into `\r\n` without a word.
 *
 * Working out which files need fixing is left entirely to git through `git ls-files --eol`, instead
 * of guessing:
 *
 *     i/lf    w/crlf  attr/text=auto eol=lf   src/a.ts     <- fix
 *     i/-text w/-text attr/text=auto eol=lf   logo.png     <- binary, skip
 *     i/lf    w/lf    attr/text=auto eol=lf   src/b.ts     <- already right
 *
 * That way binary files exclude themselves, and a file `.gitattributes` marks `-text` is respected
 * rather than fixed at random.
 *
 * Usage:
 *   node scripts/normalise-eol.mjs              # the whole repo
 *   node scripts/normalise-eol.mjs src scripts  # only within these paths
 *   node scripts/normalise-eol.mjs --check      # list only, fix nothing; exit 1 if any file is off
 */
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const args = process.argv.slice(2);
const checkOnly = args.includes("--check");
const paths = args.filter((arg) => arg !== "--check");

const git = (...rest) =>
  execFileSync("git", rest, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });

/** Files git tracks whose working tree copy is CRLF or a mix of both. */
function crlfFiles() {
  return git("ls-files", "--eol", "--", ...paths)
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      // `i/… w/… attr/…`, then a TAB, then the path. `attr/` may contain spaces, so split at the
      // TAB rather than at whitespace.
      const tab = line.indexOf("\t");
      return { flags: line.slice(0, tab), path: line.slice(tab + 1) };
    })
    .filter(({ flags }) => /\bw\/(crlf|mixed)\b/.test(flags))
    .map(({ path }) => path);
}

/** Removes every CR standing right before an LF. Done at the byte level so no encoding gets in
 *  between — the file may be UTF-8, and a round trip to a string and back is a chance to break. */
function stripCr(buffer) {
  const out = Buffer.allocUnsafe(buffer.length);
  let n = 0;
  for (let i = 0; i < buffer.length; i++) {
    if (buffer[i] === 0x0d && buffer[i + 1] === 0x0a) continue;
    out[n++] = buffer[i];
  }
  return out.subarray(0, n);
}

const files = crlfFiles();

if (files.length === 0) {
  console.log("Mọi file đều đã là LF.");
  process.exit(0);
}

if (checkOnly) {
  console.log(`${files.length} file còn CRLF:`);
  for (const file of files) console.log(`  ${file}`);
  process.exit(1);
}

for (const file of files) writeFileSync(file, stripCr(readFileSync(file)));

console.log(`Đã đổi ${files.length} file sang LF:`);
for (const file of files) console.log(`  ${file}`);
