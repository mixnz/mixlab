---
status: draft
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

`runtime.install`, `runtime.uninstall` and an install that completes itself (T185a) call
`Shims::refresh` once they have written their rows. The start-up refresh and the two-second
`bin_scan` stay, so a home changed while the daemon was stopped is still right after its next start.

**A name removed while in use.** On Windows a trampoline that is running cannot be deleted. The sweep
already renames such a file aside (`MOVED_ASIDE`) and removes it at the next refresh. That path is
reused as it is, and a test holds it for a runtime's names.

## D3 — The shim hands over when MixEngine has nothing to say

The shim resolves as it does today. **Only when the directory asked for nothing** does it look
further:

| What resolution answered | What the shim does |
| --- | --- |
| a version | runs it, as today |
| `RuntimeUnresolved` (a pin nothing installed matches) | refuses, as today. Running another version than the pin asked for is worse than an error |
| `NoDefaultRuntime` (nothing pinned, no default) | **hands over** to the next program of that name on the PATH, if there is one; otherwise refuses as today |
| no install of the kind at all (the window between an uninstall and a refresh) | **hands over**, same rule |

"The next program on the PATH" is the first match for the invoked name in `PATH` **with every entry
that is this home's `bin/` removed**, compared after `paths::in_full`. On Windows the lookup uses
`PATHEXT`, as `runnable` already does for a bindir. A file that is a MixEngine shim or trampoline
(another home's `bin/`) is skipped by name and size, the check `is_current` already makes.

The handover is the existing `become_program` with the program's arguments and **the environment
untouched**: no ini set, no PATH change, nothing of MixEngine's. On Unix that is an `exec`; on
Windows the Job Object child the shim already starts.

`MIXENGINE_SHIM_HANDED_OVER=1` is set on the handed-over child. A shim started with it set never
hands over again and refuses instead, which ends any loop two homes' `bin/` directories could make
between them.

## D4 — `mix doctor` names a program `bin/` is hiding

A new row: for each runtime name in `bin/`, if a program of the same name exists further down the
PATH, say so once, as information and not as a problem:

```
node   bin/ runs MixEngine's node 24.19.0; C:\Program Files\nodejs\node.exe is further down the PATH
```

Otherwise a person who installed Node twice has no way to see which one a terminal runs.

## D5 — The decision, written down

A new ADR, **0057 — A runtime's commands are in `bin/` only while it is installed**, supersedes point
1 of ADR 0033's decision and records D1 and D3. ADR 0033 gets the one-line `superseded in part by`
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

## Tests

- `core/tests/shims.rs`: an empty install set writes no runtime names; installing Node writes exactly
  the `kind: node` rows; Composer needs both kinds; a kind with no default keeps its names; removing
  the last version removes them, with a held file on Windows renamed aside.
- Shim: `NoDefaultRuntime` with a program further down a test PATH hands over to it, with its
  arguments and exit code; with none, refuses with 127 as today; `RuntimeUnresolved` never hands
  over; a PATH whose only other match is another home's shim refuses; `MIXENGINE_SHIM_HANDED_OVER`
  stops a second handover. Written with the `tests-that-say-why` skill, since each starts a real
  program.
- Daemon: `runtime.install` and `runtime.uninstall` leave `bin/` matching the rows without waiting
  for `bin_scan`.
- By hand on this Windows machine: with a user-level Python and no MixEngine Python, `python
  --version` in a new terminal prints the user's Python.
