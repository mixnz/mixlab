# 0057. A runtime's commands are in `bin/` only while it is installed

**Status**: Accepted. It supersedes point 1 of
[0033](0033-bin-is-a-projection-of-what-is-installed.md)'s decision (*"`shims::COMMANDS`, unchanged
and still independent of what is installed"*), and 0033's two-second poll.
**Date**: 2026-09-27

## Context

`<root>/bin` is put **first** on the person's PATH, so that it beats another copy of a language. Until
now it held every name in `shims::COMMANDS` — `php`, `node`, `npm`, `python`, `pip`, `ruby` and the
rest — whether or not anything of that language was installed. ADR 0033 kept that on purpose: a shim
that says *which command to type* seemed a better answer than `node: command not found`.

A person using the finished product reported what that cost:

1. **`which node` said yes on a machine with no Node.js.** Nothing failed until the program ran, and
   by then the person believed MixEngine had given them every language.
2. **It broke a Node.js or Python they had installed themselves.** The shim never handed over to the
   next program on the PATH, so a Node from nvm-windows, pyenv, Homebrew or a per-user Python
   installer was hidden behind a shim that exited 127. On the machine it was reported from, `python`
   in Git Bash answered *no python version is installed as the default*.

A third thing followed from the first two once `bin/` had to be exact. ADR 0033 kept `bin/` current
with a **two-second poll**: a `stat` of every runtime's bindir, for as long as the daemon ran. It
woke an idle machine for ever, and it left the daemon's own changes a tick behind — uninstalling the
last Node that had `yarn` answered with `bin/yarn` still there.

Design: [T185b](../specs/2026-09-27-t185b-bin-fronts-only-what-is-installed-design.md).

## Decision

1. **A `COMMANDS` row is in `bin/` only while its kind has a version installed.** A row run by another
   kind (`composer`, run by PHP) needs both. The daemon re-scans `bin/` at every start and after every
   runtime installed, removed, adopted or restored, before it answers.
2. **The invariant is the whole rule: no version of a language installed, no command of it in
   `bin/`.** That covers the compiled rows and every tool installed into a runtime (`npm install -g
   yarn`): the refresh leaves a tool out when its runtime is gone, whatever the table of discovered
   tools still lists.
3. **A shim never runs a program MixEngine did not install.** Where a language is installed, its shim
   resolves one of MixEngine's versions or refuses with the command that fixes it — a hand-over to the
   next program on the PATH was designed and dropped: with the invariant above there is no shim in
   front of a language MixEngine does not provide, so there is nothing for it to hand over to.
4. **`mix doctor` names a language `bin/` is hiding**, so a person with two Nodes can see which one a
   terminal runs.
5. **Nothing polls. A tool installed behind the daemon's back is heard, not looked for.** The daemon
   watches each installed runtime's bindir with the kernel's own notification —
   `ReadDirectoryChangesW`, FSEvents, inotify, through the `notify` crate behind
   `mixengine-platform::watch` — not recursively, and a bindir that does not exist yet (a fresh
   Python's `Scripts`) through its parent. An event waits 250 ms for the rest of its burst, then the
   watch is re-armed and the bindirs re-scanned, in that order, so a file written between the two is
   either read or heard. The watch follows the rows: every runtime installed, removed, adopted or
   restored re-arms it. `mix path rescan` stays for a bindir the system would not let the daemon
   watch.
6. **`[bin] rescan_seconds` does nothing, and is still read.** It leaves the template, but
   `config.toml` is written once and never rewritten, and the template of 0.0.7 to 0.0.9 has a
   `[bin]` line that is not commented out: every home first run by one of them holds it. So `[bin]`
   is a retired section — read, ignored, and logged when the key is set — and every template a
   release has shipped is a test fixture that must still load, with its keys commented and
   uncommented. Dropping the section outright stopped the daemon, and the uninstaller that asks it
   what it would remove, on a file nobody had edited.

## Consequences

**`bin/` now changes with a runtime install**, which ADR 0033 had already accepted for service clients
and global tools; the three sources of `bin/` now all follow what is installed.

**The first start after this update removes names**: a home with no Node loses its `node`, `npm` and
`npx`. A terminal that already ran `node` may remember the old path — `hash -r` in bash, or a new
terminal. The user guide says so.

**A person who installs a language with MixEngine and also has their own copy** gets MixEngine's in
every terminal, since `bin/` is first on the PATH. That is the product's promise for what it installs,
and `mix doctor` says which copy is hidden.

**Two versions of a language, only one with a tool**, keep the tool's name in `bin/` while that one
is installed. Which Node a directory means is only known when the command runs, and `bin/` is one
directory for the whole machine, so the shim answers it: a directory on the Node that has `yarn`
runs it; one on the other is told which version it resolves to and `npm install -g yarn`. It never
runs another version's copy. Removing the last Node with `yarn` removes `yarn` in the same call.

**The daemon takes a dependency for the watch**: `notify`, one crate with a small tree, behind
`mixengine-platform` and its `host` feature, so `mixengine-elevate` does not gain it. A backend
that drops events (an inotify queue overflow) reports an error, and an error is answered as a change:
a re-scan.

## Alternatives considered

**Keep the poll, and re-scan after the daemon's own changes as well.** That fixes the tick-behind
uninstall and keeps the idle cost it was meant to remove. Rejected.

**Have the `npm`, `pip` and `gem` shims wait for the program and then ask for a re-scan.** No
watch at all, but it misses `python -m pip`, `corepack enable` and `node npm-cli.js`, and it makes
the shim a parent that has to pass on Ctrl+C and the exit code instead of becoming the program.
Rejected.

**Hand over to the next program on the PATH when the directory asks for no version.** Designed and
dropped, per point 3.
