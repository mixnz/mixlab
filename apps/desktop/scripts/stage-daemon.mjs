#!/usr/bin/env node
/**
 * Stage the four headless binaries beside the window, then run `tauri dev`.
 *
 * The window looks for `mixengined` in its own directory first, and `tauri dev` starts it out of
 * src-tauri/target/debug/, where nothing else ever puts a daemon. So the MixEngine tab of a
 * development window either showed the *not installed* gate or, on a machine with a release
 * installed, found *that* daemon at the second step and started it against the wrong home — the
 * release default is `MixEngine`, a build out of cargo defaults to `MixEngine-dev` (ADR 0024), and
 * the gate then never leaves "not running".
 *
 * Four things, then `tauri dev`:
 *
 *   1. read MIX_BINARIES/MIX_CRATES out of packaging/common.sh, minus the window;
 *   2. `cargo build -p …` the four crates at the repository root, debug profile, like the window —
 *      cargo rebuilds only what changed, so an unchanged tree costs a second. A build that fails
 *      ends the script here, with the running daemon left alone;
 *   3. stop this checkout's daemon with `mix daemon stop`, **always** — it outlives its window by
 *      design, and a window that finds it still answering talks to the old code. Asking a home
 *      with no daemon to stop is a no-op;
 *   4. copy the four into src-tauri/target/debug/, unconditionally — a stale daemon beside a fresh
 *      window is the mismatch this script exists to remove, and comparing timestamps to skip a
 *      copy that takes milliseconds buys nothing.
 *
 * **The stop used to wait for the copy to fail**, which only Windows and Linux do — they refuse to
 * overwrite a running executable. macOS lets it happen, so there the old daemon was never stopped
 * and every `npm run dev:app` after a daemon change went on running the previous build (found
 * 2026-09-19). Stopping unconditionally is the same on all three systems.
 *
 * `tauri dev` is started from here rather than by `&&` in package.json so that one environment
 * reaches both it and `mix daemon stop`: two commands joined by `&&` are two processes, and an
 * environment set in the first does not survive into the second.
 *
 * **This script picks no home** — ADR 0040. It passes MIXENGINE_DEV_HOME, the same suggestion the
 * root .cargo/config.toml gives everything cargo runs, and the binaries decide: the checkout's
 * .mixengine-home, unless an elevated helper could not read it there (a checkout on an external
 * disk, on macOS), in which case the default home. Deciding here as well would be a second copy of
 * that rule, in a language the daemon does not speak. A MIXENGINE_HOME already set still wins, as
 * it does for cargo.
 *
 * Node and not bash: `npm run dev:app` is typed into PowerShell, and on a Windows machine with WSL
 * a bare `bash` is System32\bash.exe, which is WSL's. The other scripts in packaging/ are run from
 * Git Bash on purpose; `npm run` is not.
 *
 * Usage:
 *   npm run dev:app
 *   node scripts/stage-daemon.mjs --stage-only     # build and copy, no window
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { headlessBinaries } from "./packaging-lists.mjs";

const app = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(app, "..", "..");
const suffix = process.platform === "win32" ? ".exe" : "";
const stageOnly = process.argv.includes("--stage-only");

// The suggestion, never the home — see the header. `mix daemon stop` below runs outside cargo, so
// it would not otherwise see what .cargo/config.toml gives the window.
const environment = {
  ...process.env,
  MIXENGINE_DEV_HOME: process.env.MIXENGINE_DEV_HOME || join(root, ".mixengine-home"),
};

// 1. The list, from the one file that declares it.
const binaries = headlessBinaries(readFileSync(join(root, "packaging", "common.sh"), "utf8"));

// 2. Build. One cargo invocation for all four; cargo's own output is the report, and its own
//    status is the exit code when it fails.
const build = spawnSync("cargo", ["build", ...binaries.flatMap(({ crate }) => ["-p", crate])], {
  cwd: root,
  stdio: "inherit",
});
if (build.error) {
  console.error(`could not run cargo: ${build.error.message}`);
  process.exit(1);
}
if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

// The daemon a previous `npm run dev:app` started outlives its window by design. It is this home's
// daemon, stopped the way a person would stop it: through `mix`, services first, with the `mix` just
// built. `--no-autostart` is `mix daemon stop`'s own behaviour and not a flag here: a stop never
// starts a daemon in order to stop it.
//
// **Quiet when there was nothing to stop**, which is most runs: `mix` says so on stderr and exits 1,
// and a line reading `Error:` on every clean start is noise somebody learns to ignore — and then
// ignores the one that mattered. Any other failure is printed, and its exit status is still not the
// verdict: a copy that fails afterwards is.
const NOTHING_TO_STOP = "no MixEngine daemon is listening";

function stopTheDevDaemon() {
  const mix = join(root, "target", "debug", `mix${suffix}`);
  const stop = spawnSync(mix, ["daemon", "stop"], {
    cwd: root,
    env: environment,
    encoding: "utf8",
  });
  if (stop.error) {
    console.error(`could not run ${mix}: ${stop.error.message}`);
    return;
  }
  if (stop.status === 0) {
    console.log("stopped this checkout's daemon; the window will start the one just built");
    return;
  }
  if (!stop.stderr.includes(NOTHING_TO_STOP)) {
    process.stderr.write(stop.stdout);
    process.stderr.write(stop.stderr);
  }
}

// 3. Stop this checkout's daemon, whatever it is running: see the header.
stopTheDevDaemon();

// 4. Copy. The directory is created because the first `tauri dev` on a fresh clone has not made it
//    yet.
const destination = join(app, "src-tauri", "target", "debug");
mkdirSync(destination, { recursive: true });

// How long the daemon just asked to stop is given to let go of its executable. `mix daemon stop` is
// answered before the process exits, so on Windows and Linux the copy is retried rather than tried
// once.
const RELEASE_WITHIN_MS = 15_000;
const RETRY_EVERY_MS = 250;

// A running executable cannot be replaced on Windows, and the error reads as a permission problem
// (os error 5).
function isHeldByARunningProcess(error) {
  return error.code === "EPERM" || error.code === "EBUSY" || error.code === "ETXTBSY";
}

// Whether the copy happened; false only when a running process holds the target.
//
// **On macOS the old file is removed first, so the copy lands on a new inode.** macOS lets a
// running executable be overwritten in place, and the kernel keeps the code signature it validated
// for that inode while anything maps it: every later start of the rewritten file is then killed
// with SIGKILL before it prints a word — `Could not start MixEngine`, found 2026-09-19 with a
// daemon from the last window still running. Unlinking leaves that daemon on its own inode and
// gives the new binary a fresh one; it is Apple's own advice for replacing a signed binary. Windows
// and Linux refuse the overwrite instead (os error 5, ETXTBSY) until the stopped daemon has exited,
// which is what the retry below waits out.
function tryCopy(source, target) {
  try {
    if (process.platform === "darwin") {
      rmSync(target, { force: true });
    }
    copyFileSync(source, target);
    return true;
  } catch (error) {
    if (isHeldByARunningProcess(error)) {
      return false;
    }
    throw error;
  }
}

for (const { binary } of binaries) {
  const file = `${binary}${suffix}`;
  const source = join(root, "target", "debug", file);
  const target = join(destination, file);

  if (tryCopy(source, target)) {
    continue;
  }

  let copied = false;
  const deadline = Date.now() + RELEASE_WITHIN_MS;
  while (!copied && Date.now() < deadline) {
    await sleep(RETRY_EVERY_MS);
    copied = tryCopy(source, target);
  }

  // Still held: a process this home's `mix daemon stop` does not reach — a daemon started against
  // another home from the same binary. Say what to do; do not start a window beside a daemon of
  // the wrong age.
  if (!copied) {
    console.error(
      `cannot replace ${target}: it is still running.\n` +
        `Stop whatever is running it — \`mix daemon stop\` with the MIXENGINE_HOME it was started ` +
        `with, or the window's own Stop button — and run this again.`,
    );
    process.exit(1);
  }
}
console.log(`staged ${binaries.map(({ binary }) => binary).join(", ")} in ${destination}`);

if (stageOnly) {
  process.exit(0);
}

// 4. The window, with the environment the daemon beside it will resolve its home from.
//    `node tauri.js` rather than the `tauri` command: on Windows that command is a .cmd shim,
//    which spawnSync cannot start without a shell.
const tauri = join(app, "node_modules", "@tauri-apps", "cli", "tauri.js");
const dev = spawnSync(process.execPath, [tauri, "dev"], {
  cwd: app,
  stdio: "inherit",
  env: environment,
});
process.exit(dev.status ?? 1);
