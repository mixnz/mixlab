// The privileged helper's fingerprint, and when HELPER_VERSION has to move. The rule is D1 of
// docs/specs/2026-09-25-t182b-a-helper-that-keeps-up-and-an-uninstall-that-finishes-design.md.
//
//   node packaging/helper-lock.mjs --check     fail if the helper changed since the last release
//                                              and HELPER_VERSION did not
//   node packaging/helper-lock.mjs --bump      raise HELPER_VERSION one patch past the last release
//   node packaging/helper-lock.mjs --release   record this helper as the one the release ships
//
// **What the compiler builds into the helper, not a list of directories.** The sources are every
// file the dep-info of the helper's workspace crates names, for all three targets, so a module that
// only one system compiles is still in; the dependencies are every external crate in its closure.
// A list somebody had to keep current would miss the `elevated` modules of mixengine-platform, and a
// `windows-sys` bump that changes the binary without touching a line here.
//
// **Compared with the last release, not the last commit** (D1): a person receives one release, so
// the helper needs one bump per release that changes it. `baseline` in helper.lock is what the last
// release shipped; `--release` moves it and nobody runs that by hand.
//
// Exit status: 0 fine, 1 the helper changed and was not bumped, 2 this machine cannot answer (a
// missing rustup target, a cargo that failed), 64 a misuse.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";

const TARGETS = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin"];

const ROOT = process.cwd();
const LOCK = join(ROOT, "crates", "mixengine-elevate", "helper.lock");
const CONSTANT = join(ROOT, "crates", "mixengine-proto", "src", "privileged.rs");
const PATTERN = /pub const HELPER_VERSION: &str = "([^"]+)";/;

function fail(code, message) {
  process.stderr.write(`${message}\n`);
  process.exit(code);
}

function cargo(args) {
  try {
    return execFileSync("cargo", args, {
      cwd: ROOT,
      // CI sets CARGO_TERM_COLOR=always, and `cargo tree` then colours its `(*)` markers: the
      // lines no longer match the filter below and the fingerprint differs from a terminal's.
      env: { ...process.env, CARGO_TERM_COLOR: "never" },
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    const said = `${error.stderr ?? ""}`;
    const target = TARGETS.find((name) => said.includes(name) && said.includes("target may not be installed"));
    if (target) {
      fail(2, `this machine has no standard library for ${target}: rustup target add ${target}`);
    }
    fail(2, `cargo ${args.join(" ")} failed:\n${said}`);
  }
}

// A path inside the repository, spelled with forward slashes, or null for anything outside it.
// **One spelling on every machine** — a Windows dep-info says `crates\x`, a Linux one `crates/x`,
// and a fingerprint that differed by separator would call every checkout a change.
function inside(path) {
  const absolute = isAbsolute(path) ? path : resolve(ROOT, path);
  const within = relative(ROOT, absolute);
  if (within.startsWith("..") || isAbsolute(within)) return null;
  return within.split(sep).join("/");
}

// The first rule of a Makefile-style dep-info file: `target: dep dep …`, spaces escaped as `\ `.
function dependenciesIn(depInfo) {
  const text = readFileSync(depInfo, "utf8");
  const firstRule = text.split(/\r?\n/).find((line) => line.includes(": "));
  if (!firstRule) return [];
  const body = firstRule.slice(firstRule.indexOf(": ") + 2);
  return body
    .split(/(?<!\\) /)
    .map((part) => part.replace(/\\ /g, " ").trim())
    .filter(Boolean);
}

function sources() {
  const found = new Set();
  for (const target of TARGETS) {
    const out = cargo(["check", "-p", "mixengine-elevate", "--target", target, "--message-format=json"]);
    for (const line of out.split("\n")) {
      if (!line.startsWith("{")) continue;
      const message = JSON.parse(line);
      if (message.reason !== "compiler-artifact") continue;
      if (!/[\/\\]crates[\/\\]mixengine-(elevate|platform|proto)[#\/\\ ]/.test(message.package_id)) continue;
      for (const file of message.filenames ?? []) {
        const base = file.split(/[\/\\]/).pop();
        const stem = base.replace(/^lib/, "").replace(/\.(rmeta|rlib|exe|d)$/, "").replace(/\.exe$/, "");
        const depInfo = join(dirname(file), `${stem}.d`);
        if (!existsSync(depInfo)) continue;
        for (const dependency of dependenciesIn(depInfo)) {
          const kept = inside(dependency);
          if (kept && !kept.startsWith("target/")) found.add(kept);
        }
      }
    }
  }
  return [...found].sort();
}

function dependencies() {
  const found = new Set();
  for (const target of TARGETS) {
    const out = cargo([
      "tree", "-p", "mixengine-elevate", "-e", "normal", "--prefix", "none",
      "--format", "{p}", "--target", target,
    ]);
    for (const raw of out.split(/\r?\n/)) {
      const line = raw.replace(/ \(\*\)$/, "").replace(/ \(proc-macro\)$/, "").trim();
      if (!line) continue;
      // A workspace crate carries its path in parentheses. Its sources are counted above, and its
      // version moves with every release, so it is left out here.
      if (line.includes(" (")) continue;
      found.add(line);
    }
  }
  return [...found].sort();
}

function fingerprint(files) {
  const hash = createHash("sha256");
  for (const file of files) {
    // CRLF → LF, so a Windows checkout with autocrlf hashes like a Linux one.
    const bytes = readFileSync(join(ROOT, file)).toString("latin1").replace(/\r\n/g, "\n");
    const own = createHash("sha256").update(bytes, "latin1").digest("hex");
    hash.update(`${file}\n${own}\n`);
  }
  hash.update("--\n");
  for (const dependency of dependencies()) hash.update(`${dependency}\n`);
  return hash.digest("hex");
}

function helperVersion() {
  if (existsSync(CONSTANT)) {
    const found = readFileSync(CONSTANT, "utf8").match(PATTERN);
    if (found) return found[1];
  }
  // Before HELPER_VERSION existed the helper said the product's version. Only a baseline taken of
  // such a tree reads this.
  const workspace = readFileSync(join(ROOT, "Cargo.toml"), "utf8").match(/^version = "([^"]+)"/m);
  if (!workspace) fail(2, "no HELPER_VERSION and no workspace version to fall back on");
  return workspace[1];
}

function readLock() {
  if (!existsSync(LOCK)) fail(2, `${LOCK} is missing: run node packaging/helper-lock.mjs --release`);
  return JSON.parse(readFileSync(LOCK, "utf8"));
}

function writeLock(lock) {
  writeFileSync(LOCK, `${JSON.stringify(lock, null, 2)}\n`);
}

function nextPatch(version) {
  const [major, minor, patch] = version.split(".").map((part) => Number.parseInt(part, 10));
  return `${major}.${minor}.${patch + 1}`;
}

function check() {
  const lock = readLock();
  const version = helperVersion();
  const files = sources();
  const now = fingerprint(files);

  if (now !== lock.baseline.fingerprint && version === lock.baseline.version) {
    fail(1, `the helper changed since v${lock.baseline.version}: run \`bash packaging/helper-lock.sh --bump\``);
  }

  // Kept current so the pre-commit hook knows which staged files concern the helper.
  if (JSON.stringify(files) !== JSON.stringify(lock.files) || lock.version !== version) {
    writeLock({ ...lock, version, files });
  }
}

function bump() {
  const lock = readLock();
  const wanted = nextPatch(lock.baseline.version);
  const text = readFileSync(CONSTANT, "utf8");
  if (!PATTERN.test(text)) fail(2, `no HELPER_VERSION in ${CONSTANT}`);
  if (helperVersion() !== wanted) {
    writeFileSync(CONSTANT, text.replace(PATTERN, `pub const HELPER_VERSION: &str = "${wanted}";`));
  }
  writeLock({ ...lock, version: wanted, files: sources() });
  process.stdout.write(`HELPER_VERSION is ${wanted}\n`);
}

function release() {
  const files = sources();
  const version = helperVersion();
  writeLock({ version, baseline: { version, fingerprint: fingerprint(files) }, files });
  process.stdout.write(`the helper baseline is now v${version}\n`);
}

const commands = { "--check": check, "--bump": bump, "--release": release };
const command = commands[process.argv[2]];
if (!command) fail(64, "usage: helper-lock.mjs --check | --bump | --release");
command();
