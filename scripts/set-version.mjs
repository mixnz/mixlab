#!/usr/bin/env node
// Set the workspace version, and nothing else.
//
//   node scripts/set-version.mjs 0.0.1
//   node scripts/set-version.mjs 0.0.1 --dry-run
//
// **There is one place the version is written**, `[workspace.package]` in the root `Cargo.toml`.
// Every crate takes `version.workspace = true`, and everything in `packaging/` reads it back with
// `mix_version()` in `packaging/common.sh`. So this script edits one line, and then lets the two
// tools that own the rest write it out: cargo for `Cargo.lock`, and `mix` itself for the committed
// command reference, whose first paragraph states the version it was generated from.
//
// **What it must not do is replace the old version wherever it appears.** Six blueprints in
// `crates/mixengine-core/src/blueprints/gallery/` and one CLI fixture carry a
// `[blueprint.created_on] version`, which is the MixEngine a blueprint was *captured on* — a fact
// about the past. Rewriting those would make six documents claim they were captured on a release
// that did not exist when they were written. They are neither rewritten nor searched.
//
// **What it does do is carry the version to the two other kinds of place that hold one**: it stops
// the bump when `packaging/` or `.github/` types the version out instead of deriving it, it
// rewrites the handbook pages that name the current release in prose, and it writes MixLab's own
// four copies under `apps/desktop/`, which live outside this workspace. That is why this is a script
// and not a one-line edit — a version that has to be retyped by hand somewhere else is a version
// that will one day be retyped in only one of them, which is how `docs/guide/*/for-agents.md` came
// to claim `0.1.0` through three bumps.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST = join(ROOT, "Cargo.toml");
const GUIDE = join(ROOT, "docs", "guide");

/** Semantic versions, with an optional pre-release and build metadata. */
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

function fail(message) {
  console.error(`set-version: ${message}`);
  process.exit(1);
}

const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const wanted = args.find((arg) => !arg.startsWith("-"));

if (!wanted) {
  fail("usage: node scripts/set-version.mjs <version> [--dry-run]");
}
if (!SEMVER.test(wanted)) {
  fail(`${wanted} is not a semantic version — e.g. 0.1.0, or 0.0.1-beta.1`);
}

// Replace the `version` line inside `[workspace.package]` and inside no other table. A file-wide
// regex would also match `[package]`, `[dependencies]` entries and `rust-version`.
const manifest = readFileSync(MANIFEST, "utf8");
const lines = manifest.split("\n");

let inWorkspacePackage = false;
let at = -1;
let current = null;

for (const [index, line] of lines.entries()) {
  if (line.startsWith("[")) {
    inWorkspacePackage = line.trim() === "[workspace.package]";
    continue;
  }
  if (!inWorkspacePackage) continue;

  const found = line.match(/^version = "(.*)"$/);
  if (found) {
    [at, current] = [index, found[1]];
    break;
  }
}

if (at === -1) {
  fail(`no version under [workspace.package] in ${relative(ROOT, MANIFEST)}`);
}
if (current === wanted) {
  fail(`the workspace version is already ${wanted}`);
}

// **Two kinds of place hold a version, and they get two different answers.**
//
// `packaging/` and `.github/` *derive* it: `mix_version()` in `packaging/common.sh` reads it back
// out of `Cargo.toml`. One typed out there is a bug, so it stops the bump before anything is
// written — a refusal leaves the tree exactly as it was found, rather than half-bumped with a
// message about it.
//
// `docs/guide/` *names* it in prose. The install page says which release to fetch by hand until the
// permanent links go live, and the agent page prints a sample of the published manifest, whose
// `version` is `mixengine_docs::VERSION`. Neither is a bug and neither is history: they are claims
// about the current release, so the bump rewrites them.
//
// Everywhere else an old version is deliberate — a test needs an older release to compare itself
// against, a design document records what was true when it was written, a blueprint records what
// captured it — and is neither searched nor touched. So there is no list of hits for this script to
// tell the reader to ignore, which is a habit worth not starting.
//
// **A version token, not a substring.** `--fixed-strings` was the original spelling, and it matched
// `0.0.1` inside `127.0.0.1` — which appears on four handbook pages, enough noise to bury the one
// real hit under it. The rule is written twice below because `git grep` takes a POSIX regex and has
// no lookbehind: not preceded by a digit or a dot, not followed by a digit. It still finds
// `v0.0.1`, `mixlab_0.0.1-1_amd64.deb` and `"version": "0.0.1"`.
const escaped = current.replace(/[.+]/g, "\\$&");
const OUTGOING = `(^|[^0-9.])${escaped}([^0-9]|$)`;
const outgoing = new RegExp(`(?<![0-9.])${escaped}(?![0-9])`, "g");

/** Which tracked files under `pathspecs` still name the outgoing version. */
function naming(pathspecs) {
  try {
    const listed = execFileSync("git", ["grep", "-l", "-E", OUTGOING, "--", ...pathspecs], {
      cwd: ROOT,
      encoding: "utf8",
    });
    return listed.split("\n").filter(Boolean);
  } catch {
    // `git grep` exits non-zero when it matches nothing, which is the answer we want.
    return [];
  }
}

// `common.sh` and the packaging README walk `0.0.1-beta.1` through Debian's and RPM's version
// ordering as a worked example, and `macos/uninstall-check.md` records which released `.pkg` the
// removal was walked over by hand (T182a). Both are prose about one particular past tag, and the
// reason they are named here rather than discovered again every release.
const typedOut = naming([
  "packaging",
  ".github",
  ":!packaging/common.sh",
  ":!packaging/README.md",
  ":!packaging/macos/uninstall-check.md",
]);

if (typedOut.length > 0) {
  console.error(`set-version: ${current} is typed out in these, which are supposed to derive it:`);
  for (const path of typedOut) console.error(`  ${path}`);
  fail("fix them first — nothing has been written");
}

// `docs/guide/en/cli.md` is regenerated from the binary further down, so it names the old version
// right up until it does not; rewriting it here would only be undone in the same run.
const prose = naming(["docs/guide", ":!docs/guide/en/cli.md"]);

console.log(`${relative(ROOT, MANIFEST)}: ${current} -> ${wanted}`);

for (const path of prose) {
  console.log(`${path}: names ${current}${dryRun ? " (would be rewritten)" : ""}`);
}

if (dryRun) {
  console.log("--dry-run: nothing written");
} else {
  lines[at] = `version = "${wanted}"`;
  writeFileSync(MANIFEST, lines.join("\n"));

  // Cargo owns `Cargo.lock`. `--workspace` re-resolves the members only, so a version bump does not
  // quietly pull in new dependency versions along with it.
  try {
    execFileSync("cargo", ["update", "--workspace", "--quiet"], {
      cwd: ROOT,
      stdio: "inherit",
    });
    console.log("Cargo.lock: refreshed by cargo update --workspace");
  } catch {
    fail("cargo update --workspace failed — Cargo.lock still names the old version");
  }

  // **MixLab carries the workspace's version too, in four files of its own** (T104): the window
  // reports it in Settings, sends it to the sync server, and stamps it into its bundle. It sits in a
  // Cargo workspace and an npm project this manifest cannot reach, so nothing above carried it, and
  // v0.0.7 found MixLab still saying 0.0.6. Each `version` is the first one at a known indent — the
  // package's own, never a dependency's.
  for (const [path, patterns] of [
    ["apps/desktop/package.json", [/^( {2}"version": ")[^"]*(")/m]],
    ["apps/desktop/package-lock.json", [/^( {2}"version": ")[^"]*(")/m, /^( {6}"version": ")[^"]*(")/m]],
    ["apps/desktop/src-tauri/tauri.conf.json", [/^( {2}"version": ")[^"]*(")/m]],
    ["apps/desktop/src-tauri/Cargo.toml", [/^(version = ")[^"]*(")/m]],
  ]) {
    const file = join(ROOT, path);
    let text = readFileSync(file, "utf8");
    for (const pattern of patterns) {
      if (!pattern.test(text)) fail(`no version in ${path} where one was expected`);
      text = text.replace(pattern, `$1${wanted}$2`);
    }
    writeFileSync(file, text);
    console.log(`${path}: ${wanted}`);
  }

  // Its own `Cargo.lock` names MixLab and the two crates it borrows from this workspace, all three
  // of which just changed version; CI builds it `--locked`.
  try {
    execFileSync("cargo", ["update", "--workspace", "--quiet"], {
      cwd: join(ROOT, "apps", "desktop", "src-tauri"),
      stdio: "inherit",
    });
    console.log("apps/desktop/src-tauri/Cargo.lock: refreshed by cargo update --workspace");
  } catch {
    fail("cargo update --workspace failed in apps/desktop/src-tauri — its Cargo.lock is stale");
  }

  // **The committed command reference carries the version in its own first paragraph**, so a bump
  // makes it stale and the `docs` job fails on the diff. Leaving that to a warning is what cost a
  // CI run on v0.0.1-beta.1.
  //
  // This is `packaging/docs.sh --reference`'s one command rather than the script, because `bash` on
  // Windows is often WSL's, which has no cargo — and capturing the bytes here is safer than that
  // script's redirect anyway: it writes `cli.md` only once cargo has produced all of it, where a
  // redirect truncates the page *before* the build that compiles it into the binary.
  const REFERENCE = join(GUIDE, "en", "cli.md");
  try {
    const generated = execFileSync(
      "cargo",
      ["run", "--quiet", "-p", "mixengine-cli", "--", "docs", "--reference"],
      { cwd: ROOT, encoding: "buffer", maxBuffer: 32 * 1024 * 1024 },
    );
    writeFileSync(REFERENCE, generated);
    console.log(`${relative(ROOT, REFERENCE)}: regenerated`);
  } catch {
    fail("could not regenerate the command reference — it still names the old version");
  }

  // Last, so that a failure above leaves the handbook alone. The read and the write are both UTF-8
  // strings and the pattern cannot match a newline, so a page keeps the line endings it had.
  for (const path of prose) {
    const page = join(ROOT, path);
    writeFileSync(page, readFileSync(page, "utf8").replace(outgoing, wanted));
    console.log(`${path}: rewritten to ${wanted}`);
  }

  // **And restamp the translations that rewrite just invalidated.** Every Vietnamese page carries
  // the SHA-256 of the English page it was made from, and `mixengine-docs`'s
  // `a_translation_names_the_version_it_was_made_from` fails the build when the two disagree.
  // Rewriting an English page changes that hash, so a bump that stopped at the rewrite would be a
  // bump that reddens CI — which is what the first one did.
  //
  // **This records that somebody looked, and nothing more**, exactly as `packaging/docs.sh
  // --restamp` says of itself: the change being carried is a version number, which is the one edit
  // to an English page that needs no translating because both pages already spell it the same way.
  // Any other edit still has to be translated by hand before that script is run.
  //
  // It is spelled again here rather than shelled out to for the same reason as the reference above:
  // `docs.sh` needs `bash` *and* `cargo`, and `bash` on Windows is often WSL's, which has neither.
  const STAMP = /^source_sha256 = ".*"$/m;
  for (const name of readdirSync(join(GUIDE, "vi"))) {
    if (!name.endsWith(".md")) continue;

    const page = join(GUIDE, "vi", name);
    const translation = readFileSync(page, "utf8");

    // `cli.md` is the one page with no stamp: it is generated from the binary's English help and is
    // published untranslated, which its own `untranslated_reason` states.
    if (!STAMP.test(translation)) continue;

    const digest = createHash("sha256").update(readFileSync(join(GUIDE, "en", name))).digest("hex");
    const stamped = translation.replace(STAMP, `source_sha256 = "${digest}"`);
    if (stamped === translation) continue;

    writeFileSync(page, stamped);
    console.log(`docs/guide/vi/${name}: restamped against en/${name}`);
  }

  // **And record the helper this release ships as the next baseline** — roadmap task T182b, D1.
  // From here on, `helper-lock --check` asks for a bump only when the helper changes again. Node
  // and not the `.sh` beside it, for the `bash` reason above.
  execFileSync("node", [join(ROOT, "packaging", "helper-lock.mjs"), "--release"], {
    cwd: ROOT,
    stdio: "inherit",
  });
}

console.log(
  "\nNext: commit Cargo.toml, Cargo.lock, crates/mixengine-elevate/helper.lock, MixLab's files, " +
    "cli.md and the handbook pages above, " +
    "then tag — see " +
    "docs/operations/releasing.md",
);
