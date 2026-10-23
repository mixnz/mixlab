# 0051. An uninstall ends what it undoes, and is the uninstaller's rather than the window's

**Status**: Accepted. It amends the T87 design's D7 (the order of an uninstall) and removes the
in-app uninstall that [ADR 0029](0029-every-install-format-carries-a-helper-to-install-from.md)
was written against.
**Amended by** [0066](0066-what-a-home-applied-is-that-homes-to-undo.md): what another home on the
machine still uses is kept.
**Date**: 2026-09-24

## Context

T87 gave MixEngine a complete uninstall, `daemon.uninstall`, and MixLab offered it as a section of
its Settings with *keep my databases* ticked by default. With `keep_home` the daemon did not exit.
So the ordinary way to press that button removed the privileged helper, the certificate authority,
the hosts block and the resolver wiring, and left the window running on top of a machine that could
no longer elevate. On macOS the only way back was reinstalling the `.pkg`. Nobody wants that state:
a person who uninstalls wants MixEngine and MixLab gone, with an honest choice about their data.

Meanwhile the Windows uninstaller, which runs without an administrator, removed only its own files.
Removing MixLab there left the certificate authority trusted in every store, a hosts block, and an
NRPT rule routing `.test` to a resolver that no longer existed.

## Decision

1. **A finished uninstall ends the daemon, whatever it kept.** `keep_home` means *leave the data
   where it is* and nothing more. An unfinished one (a declined prompt, a row that is still there)
   leaves the daemon up, so the same command run again finishes it.
2. **Taking MixEngine off a machine is not a screen of a graphical client.** MixLab loses its
   Uninstall section on every system. The Windows uninstaller calls `mix uninstall`; on macOS and
   Linux, `mix uninstall` is the step before removing the package until those formats have an
   uninstall path of their own (T182a). `daemon.uninstall_plan` and `daemon.uninstall` stay in the
   API for them, so *no client-only capability* is untouched.
3. **Privileged first.** The prompt is raised before `PATH`, the login entry and the browsers are
   touched, and a declined or failed prompt drops what the run queued. T87's D7 cleaned those three
   first so a declined grant still left them clean. That trade was right for a button in a window a
   person would go on using. For a removal that must finish or change nothing, a half-clean machine
   after a declined prompt is the state to avoid.

The reasoning, and what else T182 changed (the relocated directories as a second choice, a process
in the way refusing the run, a directory renamed before it is deleted), is in the
[T182 design](../specs/2026-09-24-t182-removing-mixlab-is-one-act-design.md).

## Consequences

- A script that ran `mix uninstall --keep-home` and then talked to the daemon now finds it gone.
- `daemon.uninstall` with `grant: false` changes nothing but the queue: the unprivileged half waits
  for a grant that did not happen.
- macOS and Linux users still run `mix uninstall` by hand before removing the package, and the
  handbook says so.
