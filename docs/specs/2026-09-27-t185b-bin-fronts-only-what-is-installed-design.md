---
status: implemented
date: 2026-09-27
task: T185b
---

# T185b — `bin/` fronts only what is installed

Roadmap task T185b, in [phase 7](../roadmap/phase-7-efficiency.md), after T185a. 2026-09-27.

## The problem

`<root>/bin` holds a shim for every name in `shims::COMMANDS` — `php`, `node`, `npm`, `python`,
`pip`, `ruby`, `composer` and the rest — whether or not anything of that language is installed.
[ADR 0033](../decisions/0033-bin-is-a-projection-of-what-is-installed.md) kept that on purpose for
runtimes: a shim that says *which command to type* seemed a better answer than
`node: command not found`.

Two things, reported by someone using the finished product, show that it is the wrong trade:

1. **`which node` says yes on a machine with no Node.js.** So does `which python`. Nothing fails
   until the program runs, and by then the person believes MixEngine gave them every language. A
   `bin/` that looks complete is a promise the install never made.
2. **It breaks a Node.js or Python the person installed themselves.** `bin/` is *prepended* to the
   user's PATH (`windows/path.rs`, the rc block on Unix) so that it beats other copies. The shim
   never hands over to the next program on the PATH. So a Node from nvm-windows, pyenv, Homebrew or
   the python.org per-user installer is hidden behind a shim that exits 127. Installing MixLab
   stopped a working `node`. This machine shows it today: `python` in Git Bash answers
   *no python version is installed as the default*.

## D1 — A runtime's names are in `bin/` only while one of its versions is installed

`shims::refresh` takes the set of installed runtime kinds and writes a `COMMANDS` row only when its
kind is in that set. For a row with a `via` (Composer, run by PHP), **both** kinds have to be
installed. The rest of the projection is unchanged: service clients (T130) and discovered global
tools (T131) already appear only with what provides them.

Grouping follows the table rather than a second list: `node`, `npm`, `npx` and `corepack` are all
`kind: node`, so they come and go together. The same holds for `python`/`pip` and for
`ruby`/`gem`/`bundle`.

A kind that has versions installed but **no default** keeps its names. That is a state a person
can fix with one command, and the shim says which one.

## D2 — `bin/` is refreshed when a runtime comes or goes

`runtime.install`, `runtime.uninstall`, an adopted or restored runtime and an install that
completes itself (T185a) call `Shims::runtimes_changed` once they have written their rows: a
re-scan of every bindir, not only a refresh, since the last Node that had `yarn` takes `yarn` with
it. A start re-scans too, so a home changed while the daemon was stopped is right as it starts.

**No poll** (changed while building). The two-second `bin_scan` loop is gone. A tool installed
with `npm install -g` is heard by a watch on each runtime's bindir — the kernel's notification,
through `notify` in `mixengine-platform` — so an idle machine pays nothing. `[bin]
rescan_seconds` goes with it; ADR 0057 records it.

**A name removed while in use.** On Windows a trampoline that is running cannot be deleted. The sweep
already renames such a file aside (`MOVED_ASIDE`) and removes it at the next refresh. That path is
reused as it is, and a test holds it for a runtime's names.

## D3 — No hand-over (dropped while building)

A shim that handed over to the next program on the PATH when the directory asked for nothing was
designed here and dropped before it shipped. D1 is the whole answer: no version of a language
installed means no command of it in `bin/` — compiled rows and tools installed into a runtime alike —
so there is never a shim in front of a language MixEngine does not provide, and nothing to hand over
to. Where a language is installed, its shim resolves one of MixEngine’s versions or refuses with the
command that fixes it, as before.

## D4 — `mix doctor` names a program `bin/` is hiding

A new row: for each runtime name in `bin/`, if a program of the same name exists further down the
PATH, say so once, as information and not as a problem:

```
node   bin/ runs MixEngine's node 24.19.0; C:\Program Files\nodejs\node.exe is further down the PATH
```

Otherwise a person who installed Node twice has no way to see which one a terminal runs.

## D5 — The decision, written down

A new ADR, **0057 — A runtime's commands are in `bin/` only while it is installed**, supersedes point
1 of ADR 0033's decision and its poll, and records D1, D2's watch and D3. ADR 0033 gets the one-line `superseded in part by`
pointer its README asks for. The module comment in `crates/mixengine-core/src/shims.rs` ("It does
not depend on what is installed") and `docs/features/runtime-versions.md` ("refresh shims" as the
last step of an install, which becomes true) are corrected in the same change.

## What the person sees

- Before anything is installed, `bin/` holds nothing of the four languages, and `which node` finds
  their own Node or nothing.
- After `mix runtime install node 24`, `node`, `npm`, `npx` and `corepack` appear within the same
  call.
- After the last Node is uninstalled, they go, and the person's own Node is back.
- Desktop copy that promises `php`, `node`, `python` on the PATH (the PATH switch in Settings, the
  Dashboard reminder, the guide) is reworded to *the languages you install with MixEngine*.

## MixLab

No new screen, and nothing the window has to decide: which names `bin/` holds is the daemon's.
The PATH switch in Settings and the Dashboard's *Use php, node and python in any terminal* card are
reworded to *the languages you install with MixEngine*, since a fresh install no longer fronts all
four. D4's doctor row appears in Settings → Doctor like every other check.

## Tests

- `core/tests/shims.rs`: an empty install set writes no runtime names; installing Node writes exactly
  the `kind: node` rows; Composer needs both kinds; a kind with no default keeps its names; removing
  the last version removes them, with a held file on Windows renamed aside.
- `core/tests/shims.rs` also: a tool installed into a runtime (`yarn` into Node) leaves `bin/` with
  its runtime, whatever the table of discovered tools still lists.
- Daemon: `runtime.install` and `runtime.uninstall` leave `bin/` matching the rows before they
  answer; two Nodes where only one has `yarn` lose `yarn` with that one and keep `node`; a tool
  written into a bindir appears in `bin/` unasked, and goes when it is removed.
- Platform: a watched directory's new file is heard, a missing directory is heard being created,
  and a dropped watch hears nothing.
- By hand on this Windows machine: with a user-level Python and no MixEngine Python, `python
  --version` in a new terminal prints the user's Python.
