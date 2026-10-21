# 0065. A terminal may raise the elevation prompt itself

**Status**: Accepted. It amends [0005](0005-on-demand-elevation.md)'s Linux row — *"we must detect
this and print the exact command for the user to run manually"* — and adds a second way to raise
the one prompt on Linux and macOS. Everything else in 0005 stands.
**Date**: 2026-10-21

## Context

ADR 0005 raises every elevation through a dialog the daemon spawns: UAC, `osascript`, `pkexec`. On a
Linux machine with no polkit agent it promised to *print the exact command* instead. Phase 34's
milestone runs MixEngine on an Ubuntu 24.04 server reached only over SSH, and there neither half
held. Measured on 2026-10-21 (the
[T211 design](../specs/2026-10-21-t211-elevation-from-a-terminal-design.md) has the detail):

1. `polkitd` is installed and running, and `pkexec` is not: since Ubuntu 24.04 it is a package of
   its own. The daemon said *"polkit is not installed"*, which was wrong.
2. Installing `pkexec` would not help. An SSH session has no polkit agent, and the daemon runs
   `pkexec --disable-internal-agent` so that it never prompts on a tty nobody watches.
3. The command a person was meant to be told about could never be run. The daemon refused before it
   wrote a request, and removed the request directory after any attempt.
4. The daemon has no terminal of its own: it is a systemd user unit or a launchd agent. The only
   process holding a tty is the `mix` a person just typed.

macOS over SSH has the same gap. `osascript … with administrator privileges` either fails without a
GUI session or, with somebody logged in at the console, raises its dialog on a screen nobody at
this terminal can see.

## Decision

**When `mix` holds a terminal and either the person is behind SSH or no dialog can be raised, `mix`
raises the prompt itself, with `sudo`, on the installed helper. The daemon writes the request and
waits for the report; it spawns nothing.**

1. **The client picks the route.** Only `mix` knows it has a tty and that it runs over SSH. It asks
   for `by_hand` on `elevation.grant` and on `daemon.uninstall`. The window never does. Windows
   never does: an administrator over OpenSSH already holds a full token, and UAC has no terminal
   form.
2. **The daemon waits instead of spawning.** It writes the request into the single-use directory as
   before, publishes the two paths on `elevation.status` as `waiting`, and ends the wait when the
   helper's report appears, when the job is cancelled, or after ten minutes. A cancel and a timeout
   are a decline: every row stays queued.
3. **`mix` runs only the installed helper.** It runs `sudo -- <helper> <request>`, where `<helper>`
   is the compiled-in path `mixengine-platform` names and root owns, and refuses any other path the
   daemon hands it. A compromised daemon must not be able to choose what a person runs as root. So
   the terminal route needs an installed helper: a release has one, and a development build is told
   so.
4. **No caller without a terminal is routed through `sudo -n`.** Where neither a dialog nor a
   terminal can ask, the command stops before its `continue? [y/N]` and names what to run from a
   terminal. Every elevation still has a person agreeing to it.

The helper is unchanged: it validates every request itself, writes its report with `create_new`, and
appends to its root-owned audit log.

## Consequences

**Easy**:

- `mix elevation grant` and `mix uninstall` finish over SSH on Linux and macOS, as the milestone
  asks, with one password prompt per batch, as 0005 asks.
- The command a person needs while a grant waits is on `elevation.status`, so `mix elevation status`
  and MixLab can show it, and a second session can run it.

**Hard / accepted costs**:

- **The daemon learns that a report appeared, not that the helper ran.** With a dialog it spawned
  the helper itself. Now any process of this account could write `response.json`. That process is
  already inside the daemon's trust boundary
  ([security-model.md](../architecture/security-model.md)), and a forged report only misinforms
  the queue: nothing on the machine changes without the helper.
- A grant waiting for a terminal holds the grant slot for up to ten minutes.
- `sudo` is one more failure mode to word: wrong password, not in `sudoers`, no `sudo` at all.

## Alternatives considered

- **`sudo -n` behind a `NOPASSWD` rule.** Works for scripts, and makes a standing passwordless route
  to a root program the recommended setup. Rejected: 0005 wants a person to agree to every
  elevation.
- **Ship a polkit `.policy` file, or ask for `pkexec` to be installed.** Neither gives an SSH session
  an agent. The policy file would also need elevation to install.
- **The daemon runs `sudo`.** It has no terminal to ask on, and `sudo -S` fed from a client would
  move a password over the API.
- **Print `sudo <helper> <request>` and stop.** That was 0005's promise. It needs a request to exist,
  and a person to tell the daemon when they have run it. The wait in decision 2 is that, without the
  telling.
