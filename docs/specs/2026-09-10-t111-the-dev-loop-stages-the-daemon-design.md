---
status: implemented
date: 2026-09-10
task: T111
---

# T111 — The dev loop stages the daemon beside the window

Roadmap task [T111](../roadmap/phase-12-one-product.md), on
[the desktop client design](2026-09-08-the-desktop-client-in-this-repository-design.md)'s D9 and on
[T107](2026-09-09-t107-where-the-daemon-and-the-window-are-design.md). 2026-09-10.

T107 gave the window one rule for finding `mixengined`: the running executable's own directory, then
the operating system's install location, then `PATH`. Every installer T105 produces satisfies the
first or the second step, so a person who installs MixEngine from the next release never sees the
*MixEngine was not found on this machine* gate. The one place the rule finds nothing is the one every
contributor sits in: `npm run dev:app` starts the window out of
`apps/desktop/src-tauri/target/debug/`, and nothing puts a daemon there.

This task is that one directory. **No API method changes, no packaging script changes, and a
release artifact is byte-for-byte what it was.** What changes is what `npm run dev:app` does before
`tauri dev`, and what the gate says when even that has found nothing.

## What is already true

Written down so nothing below is built twice:

- **The lookup is one function**, `mixengine_platform::install::program_path`, and its first step
  is the running executable's directory. `health.rs` asks it for `mixengined`; the daemon asks the
  same shape of question for the shim (`core::shims::source`, beside itself and nowhere else) and,
  in a build out of `cargo`, for the helper (`core::elevation::helper`'s fallback is the directory
  beside the program). So *a daemon beside the window* is not enough: it is four binaries or none.
- **The list of binaries has one home**, `MIX_BINARIES` and the index-aligned `MIX_CRATES` in
  `packaging/common.sh`, and `crates/mixengine-core/tests/packaging.rs` holds every Rust reader to
  it. `mix_headless_binaries` is that list minus `MIX_WINDOW`.
- **A build out of `cargo` is not a release** (ADR 0024): `mixengine_platform::RELEASE` is false in
  every binary this task stages and in the window `tauri dev` builds, so all five default to the
  `-dev` home. The root `.cargo/config.toml` additionally points whatever cargo runs at the
  repository's own `.mixengine-home`, and does not force it.
- **`tauri dev` runs the frontend first**: `beforeDevCommand` starts Vite, the CLI waits for
  `devUrl`, then builds and runs the crate. Anything that must be on disk before the window asks
  for it runs before `tauri dev`, not inside it.

## The bug this also closes

On a machine that has a release installed — the machine this product is written on — the dev
window finds no daemon at step 1 and finds the **release** daemon at step 2. The gate says *not
running*, its Start button runs that daemon with `--detach`, and that daemon opens the release home
while the window keeps dialling `MixEngine-dev`. The gate never leaves *not running*, and a release
daemon has been started by a development window. Staging the dev daemon at step 1 ends this by
construction: step 1 answers before step 2 is consulted, and it answers with a binary built from the
same tree as the window.

## D1 — A script stages the four headless binaries before `tauri dev`

`apps/desktop/scripts/stage-daemon.mjs`, which `npm run dev:app` becomes. The script stages and
then **starts `tauri dev` itself** rather than leaving that to `&&`: two commands joined by `&&`
are two processes, and the home D2 sets for the second would not survive the end of the first.
`--stage-only` stops after the copy, which is how the step is checked without a window. `npm run
dev` — the frontend alone, in a browser — is untouched: it has no window to put a daemon beside.

The script does four things, in order, the last being `tauri dev` with D2's environment:

1. **Reads the list.** It opens `packaging/common.sh` and takes `MIX_BINARIES` and `MIX_CRATES` off
   their two `name=(…)` lines, pairs them by index, and drops the `MIX_WINDOW` entry. It does not
   carry a list of its own: a fifth headless binary added to `common.sh` is staged on the next
   `npm run dev:app` with no edit here, and a script that named four binaries would be the second
   list `packaging.rs` exists to forbid.
2. **Builds them.** `cargo build -p <crate> …` at the repository root, one invocation for all four,
   in the debug profile the window itself is built in. The first run pays for the root workspace
   once; every run after that is cargo's own incremental answer, usually under a second.
3. **Copies them** from `target/debug/` to `apps/desktop/src-tauri/target/debug/`, with the
   platform's executable suffix, creating the directory when it is not there yet — the first
   `tauri dev` on a fresh clone has not created it. The copy is unconditional: a stale daemon beside
   a fresh window is exactly the mismatch this script exists to remove, and comparing timestamps
   to skip a copy that takes milliseconds buys nothing.

**Node and not bash**, although every other script in `packaging/` is bash. `npm run dev:app` is
typed into PowerShell on Windows, and on a Windows machine with WSL installed a bare `bash` resolves
to `System32\bash.exe`, which is WSL's — a script that then reads `C:\…` paths and runs a Linux
`cargo` is the failure mode. The other `packaging/` scripts are run from Git Bash on purpose;
`npm run` is not. The two scripts already under `apps/desktop/scripts/` are Node for the same
reason.

**Not Tauri's `externalBin`.** Tauri copies sidecars beside the executable during `tauri dev`, which
is the same effect — and it asks for the files to be named `<name>-<target-triple>` under
`src-tauri/binaries/`, declared in `tauri.conf.json`, and built by something else first. That is a
second list of binaries in a Tauri-shaped convention, for a build packaging already owns through
`stage.sh`, and it would still need a script to produce the files. The one thing it adds, a
`Command.sidecar` API, this crate does not use: `health.rs` spawns by path.

**Not a `build.rs` in the desktop crate.** A build script that runs `cargo build` on another
workspace runs inside a cargo lock, mixes two target directories' timestamps, and turns a
ten-second copy into a step every `cargo clippy` in `src-tauri` pays for.

## D2 — One home for the window and the daemon it starts

The script exports `MIXENGINE_HOME` to the repository's `.mixengine-home` — the same directory the
root `.cargo/config.toml` names — for the `tauri dev` it starts, **unless the variable is already
set**, in which case the person's own answer wins, exactly as it does for cargo.

Without this there are two development homes: `cargo run -p mixengine-daemon` from a terminal
lands in `.mixengine-home` through cargo's config, while a daemon the window starts with `--detach`
inherits the window's environment, and `tauri dev` is not `cargo run`. Whether the CLI passes
cargo's `[env]` through is a property of the Tauri CLI's process tree that this design does not want
to depend on; setting it once, here, makes the answer the same whichever way the daemon was started,
and a `mix status` in a terminal sees the daemon the window sees.

## D3 — The gate says where it looked

`mixengine_presence` today answers a bare string. It grows to an object:

```ts
type PresenceReport = { presence: Presence; searched: string[] };
```

`searched` is the directories `program_path` consulted, in order — the executable's own, then
`program_dirs()`, then `PATH`'s entries — and it is filled only when `presence` is `notInstalled`;
in the other three states it is empty, because nothing was searched. `mixengine_platform` gains the
one function that returns that list, and `program_path` is rewritten over it so the list the gate
shows is the list the lookup walked, not a second description of it.

The gate's copy changes with it. After ADR 0027 the *not installed* state has one meaning — the
install is incomplete — and *Install MixEngine* was the standalone client's sentence for a machine that had never
had it:

| Key | Was | Becomes |
| --- | --- | --- |
| `gate.notInstalled` | MixEngine was not found on this machine. | `mixengined` was not found beside MixLab. |
| `gate.lookedIn` | — | Looked in, in this order: |
| `gate.getIt` | Install MixEngine | Reinstall MixEngine |

The button still opens the install page for the current language; reinstalling is the only thing
that puts the four binaries back for a person who has a window and nothing beside it, and the page
is where the installers are. `vi.ts` gets all three lines. This is the one user-visible change of the
task and takes a line under `### Changed` in the root `CHANGELOG.md`.

**The directories are a list and not a clause inside that sentence**, which is what the first
build of this screen tried. A real machine's `PATH` carries dozens of entries — the machine this was
written on produced forty-five — so a centred paragraph of them filled the window and pushed the
*Reinstall* button off the bottom edge: the screen lost the one control it exists to offer. What is
drawn instead is the sentence, a quiet label, and a bounded, scrollable `<ul>` in the monospace face,
one directory per line. The first two lines are the ones a person can act on — beside the program,
then where this operating system's installer puts MixEngine — and the `PATH` tail is below the fold
rather than in the way. Keeping every entry rather than trimming to those two is deliberate: a
message that named two directories while the lookup had walked forty-seven would be a shorter lie.

**Why the list, and not a development-only hint.** The person who will read this line most is a
contributor whose `npm run dev:app` was bypassed — `tauri dev` run by hand, or a window started out
of `target/debug` by the file manager. A sentence for them alone would be a string that exists only
in a non-release build, which is a fourth state the frontend would have to draw. The searched list
serves both readers: a user sees a path they can open in a file manager and find empty, a
contributor sees `src-tauri/target/debug` and knows which command they skipped.

## Error handling

- **The copy is refused because the daemon is running.** On Windows a running executable cannot
  be overwritten, and the error is `os error 5`, which reads as a permissions problem. The daemon
  the last window started outlives it by design, so this is the ordinary second run, not an
  accident. On `EPERM`/`EBUSY` the script runs the freshly built `mix daemon stop` with the same
  `MIXENGINE_HOME` — services first, as a person would — and retries the copy for up to fifteen
  seconds, because the stop is answered before the process exits. A copy that is still refused is
  a binary something else is running, such as a daemon started against another home: the script
  names the binary, says to stop it, and exits non-zero, so `tauri dev` does not start a window
  beside a daemon of the wrong age.
- **`cargo build` fails.** The script exits with cargo's own status and adds nothing: cargo's
  message is the message.
- **`common.sh` has no `MIX_BINARIES` line, or the two arrays differ in length.** The script
  refuses with a sentence naming the file, rather than staging whatever it managed to parse.
  `packaging.rs` guards the same file from the Rust side; this is the same guard from the script's.
- **`searched` in a release build** never names `src-tauri/target/debug`, because the first entry
  is wherever the window is. Nothing in D3 is development-specific.

## Testing

- **The script is not unit-tested**: it is a build step, and the test that matters is `npm run
  dev:app` on a clone that has never built the root workspace, on each of the three systems —
  `(P)`, because `EXE_SUFFIX`, the copy error and the home path all differ. Its list-reading is
  held by a vitest case over a fixture of `common.sh`'s two lines, since parsing a bash array with a
  regular expression is the one part of it that can be wrong quietly.
- **`mixengine-platform`**: `program_path` and the new list function answer the same first hit on
  the same input; an empty `PATH` entry appears in neither.
- **`health.rs`**: the report is camel-cased for the shell (the existing test grows a field), and
  `searched` is empty for every state but `notInstalled`.
- **The frontend**: `api.ts`'s `presence()` returns the object, and the gate renders the joined
  list. No pure-logic module reads `presence` today, so no vitest case changes shape.
- **What stays green without changes**: `packaging.rs`, every packaging script, `feed-check.sh`,
  and `tests/layering.rs` — the script depends on nothing, and the crate gains no dependency.

## Out of scope

- **`npm run build:app`**, Tauri's own bundling, whose `bundle.targets` in `tauri.conf.json` still
  lists NSIS, DMG, AppImage and `.deb` from the standalone client. Packaging is `packaging/`'s, and pruning that
  list is its own small task.
- **Starting the daemon when the tab opens.** The gate's *Start* button stays a button, for the
  reason `MixEngineTab.tsx` gives: opening a tab is cheap, starting a daemon is not.
- **A release profile for the dev loop.** The window `tauri dev` builds is a debug build; the
  daemon beside it is too. A contributor measuring performance builds through `packaging/`.

## What lands where

- `apps/desktop/scripts/stage-daemon.mjs`, its parser `apps/desktop/scripts/packaging-lists.mjs`
  with a vitest case beside it, and `apps/desktop/package.json` (`dev:app`).
- `crates/mixengine-platform/src/install.rs`: the searched-list function, `program_path` over it.
- `apps/desktop/src-tauri/src/modules/mixengine/health.rs` and `commands.rs`: `PresenceReport`.
- `apps/desktop/src/modules/mixengine/api.ts`, `MixEngineTab.tsx`, `i18n/en.ts`, `i18n/vi.ts`.
- Documentation: the command table in `apps/desktop/CLAUDE.md`, *Local development* in
  `docs/operations/build-and-release.md`, T111 in `phase-12-one-product.md` before M12, one
  line in `CHANGELOG.md`.
