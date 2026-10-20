---
status: implemented
date: 2026-10-21
task:
  - T211
  - T211a
  - T211b
  - T211c
  - T211d
---

# T211 — Elevation from a terminal (design)

Roadmap task **T211**, phase 34: on a machine where no dialog can be raised, `mix` in a terminal
raises the prompt itself, through `sudo` on the helper — amending
[ADR 0005](../decisions/0005-on-demand-elevation.md). Milestone **M34** ends with *"over the same SSH
session, `mix elevation grant` applies its queue and `mix uninstall` finishes"*, and today neither
does.

## Measured, not assumed

On 2026-10-21, on this repository's development machine: Ubuntu 24.04.5 LTS, reached over SSH, no
desktop.

1. **`polkitd` is installed and running; `pkexec` is not.** `dpkg -l polkitd` → `ii 124-2ubuntu1.24.04.4`,
   `dpkg -l pkexec` → `un`, `/usr/bin/pkexec` absent. Since Ubuntu 24.04 `pkexec` is its own package,
   so "no `pkexec`" does not mean "no polkit" — which is what
   [prompt.rs](../../crates/mixengine-platform/src/prompt.rs) says today.
2. **Installing `pkexec` would not help.** The SSH session has no seat and no polkit agent, and
   [linux/prompt.rs](../../crates/mixengine-platform/src/linux/prompt.rs) runs
   `pkexec --disable-internal-agent` on purpose, so a daemon never prompts on a tty nobody watches.
3. **`sudo` is there, and the person at the terminal can answer it.** `/usr/bin/sudo` is present;
   the account is in `sudo` (this machine's cloud image also has `NOPASSWD`, which this design does
   not rely on).
4. **The manual command a person is told about cannot be run.** `Elevation::preflight`
   ([require.rs](../../crates/mixengine-daemon/src/elevation/require.rs)) refuses on `probe()` before
   any request is written, so the `pkexec <helper> <request>` that `linux::missing` composes in
   `run()` is never reached; and when it is, `flush` removes the request directory before anybody
   could type the command.
5. **The daemon has no terminal.** It runs as a systemd user unit (T195) or a launchd agent; only
   the `mix` process the person started holds a tty `sudo` can ask on.

macOS over SSH has the same gap from the other side: `osascript … with administrator privileges`
either fails without a GUI security session or, with somebody logged in at the console, raises the
dialog on the Mac's own screen — the trap `mix self-update` already documents for Installer.app
([render/self_update.rs](../../crates/mixengine-cli/src/render/self_update.rs)).

## Goal

Over SSH, on Linux or macOS, `mix elevation grant` shows the batch, asks `continue? [y/N]`, and on
*yes* `sudo` asks for the person's password in that same terminal; the queue is applied and settled
exactly as after a dialog. `mix uninstall` does the same for its privileged half and finishes. Where
nothing can ask — no dialog and no terminal — the error says so *before* the question, and names
the command that would work.

## Non-goals

- **Windows.** Over OpenSSH an administrator's session already holds a full token, and UAC has no
  terminal form; nothing changes there.
- **`sudo -n` / `NOPASSWD`.** A caller with no terminal is refused with the command to run, never
  routed through a non-interactive `sudo` (decided in conversation, 2026-10-21): ADR 0005 wants a
  person to agree to every elevation.
- **A polkit `.policy` file**, or any change to how `pkexec` is run where it works.
- **MixLab running `sudo`.** The window has no terminal; see *MixLab*.

## Design

### D1. The client decides the route; the daemon only waits

Which of the two ways a prompt can be raised is a fact about the *caller*, not the daemon: only
`mix` knows it holds a tty, and that the person is behind SSH. So:

- **Dialog** (unchanged): the daemon runs `Elevation::run` — UAC, `osascript`, `pkexec`.
- **Terminal** (new): the daemon writes the request as it does now, publishes where it is, and
  waits for the helper's report to appear beside it. It spawns nothing.

`mix` picks *terminal* when it has a tty on stdin **and** either `$SSH_CONNECTION`/`$SSH_TTY` is
set or `elevation.status` says the dialog cannot be raised. Otherwise it picks *dialog*. Never on
Windows. *My call:* SSH wins even when the daemon could raise a dialog, because on macOS that
dialog lands on a screen nobody at this terminal can see.

### D2. `elevation.grant` and `daemon.uninstall` take `by_hand`

`ElevationGrant { by_hand: bool }` (new params; absent = today's behaviour) and
`UninstallQuery::by_hand: bool` (`#[serde(default)]`). With `by_hand: true`, `preflight` does not
refuse on `probe()`; `flush` replaces `machine.elevation().run(…)` with a wait for the report. No
new method: the wait is a stage of the job the caller is already following, and the uninstall's
grant (`grant_only`) goes through the same `flush`.

The job's progress says `waiting for the command to be run in a terminal`, and
`ElevationStatus` gains `waiting: Option<ByHand>`, with `ByHand { job, helper, request }`: the two
absolute paths, present only while a `by_hand` grant is waiting. That is how `mix` learns what to
run, and how anybody else — `mix elevation status`, MixLab — can show it.

### D3. What `mix` runs, and what it refuses to run

`mix` runs `sudo -- <helper> <request>` with stdin, stdout and stderr inherited, so `sudo`'s
password prompt and the helper's own output reach the person. Then it goes back to following the
job.

**The helper is never a path from the wire.** `mix` runs only `mixengine_platform::install::helper_path()`
— a compiled-in constant on both Unixes — and only when `elevated::owner_of` says root owns it;
`ByHand.helper` must equal it or `mix` refuses. A compromised daemon must not be able to hand a
person a program to `sudo`. The request must lie under the home's `elevate/` directory. **Consequence:**
the terminal route needs an installed helper. A development build with only the copy beside the
daemon, or a first `helper-install`, is refused with a sentence that says so; packages place the
helper (T88e), so a release does not hit this.

### D4. How the wait ends

- **A report appears** (`response.json` in the request directory): judged exactly as `Completed`
  is now — `read_report`, `settle`, the firewall memory, the helper re-handshake.
- **`sudo` exits non-zero and no report is there**: `mix` cancels the job (`job.cancel`) and says
  why; the daemon treats a cancelled wait as `Declined` — every row kept, ADR 0005's "a declined
  prompt is a normal outcome".
- **Nobody runs it**: after **10 minutes** *(my call)* the daemon ends the wait as `Declined`, frees
  the grant slot and removes the directory. A `--no-wait` caller, or someone who prints the command
  and runs it in another session, has that long.
- **Ctrl-C in `mix`**: cancels the job, as above.

The single-use directory, its removal on every branch, and the anti-replay check stay as they are.

**Trust:** with `pkexec` the daemon knew the helper ran; now it knows a report appeared. Any process
of this account could write one — but such a process already *is* the daemon's trust boundary
([security-model.md](../architecture/security-model.md)), and a forged report only misinforms the
queue; it changes nothing on the machine. The helper still validates every request itself.

### D5. Asked before the question (T211c)

`mix elevation grant` and `mix uninstall` decide the route **before** printing `continue? [y/N]`.
If neither route exists — no tty (script, `--json`, `--yes` under cron) and no dialog — they stop
there with nothing started:

```
this machine cannot raise an elevation prompt, and this command has no terminal to ask in.
run it again from a terminal:  mix elevation grant
```

*My call:* the sentence names `mix elevation grant` and not `sudo <helper> <request>`, because no
request exists yet; the raw command is what `mix elevation status` shows while a grant waits (D2).

### D6. The `pkexec` sentence

`linux::missing` says *"pkexec is not installed (on Ubuntu 24.04 and later it is a package of its
own)"*, and every Linux `Unavailable` reason ends by pointing at `mix elevation grant` in a terminal
instead of a `pkexec` command that will not run. `ElevationStatus::reason`'s doc stops promising
a `pkexec` command.

### D7. Documentation (T211d)

`docs/guide/en/uninstalling.md` names `mixengine-headless` beside `mixlab` for `apt remove` and
`dnf remove`; `mix uninstall`'s last line names the package command; `--keep-home`'s help stops
saying the daemon keeps running ([ADR 0051](../decisions/0051-an-uninstall-ends-what-it-undoes.md)). `security-model.md` and
`platform-abstraction.md` gain the terminal route. The ADR (**0065**, amending 0005) records D1, D3
and D4's trust note.

## MixLab

The window never runs `sudo` and never sends `by_hand: true`. Where `elevation.status` says the
dialog cannot be raised, the elevation dialog's existing *cannot prompt* notice
([ElevationDialog.tsx](../../apps/desktop/src/modules/mixengine/components/ElevationDialog/ElevationDialog.tsx))
reads *"Run `mix elevation grant` in a terminal on this machine."*, and while `waiting` is present
it shows the waiting command with a copy button. No new method, so `check-client-surface.mjs` is
unaffected; the new field reaches the window through `bindings/`.

## Tasks

- **T211** — ADR 0065 and this design. **T211a (P)** — D2, D4 in the daemon, Linux and macOS.
  **T211b (P)** — D1, D3 in `mix`. **T211c** — D5, D6. **T211d** — D7 and the MixLab notice.
  macOS joins T211a/b rather than a task of its own; the (P) verification includes a Mac over SSH.

## Left to the builder

How `flush` waits (poll or a file watcher), the exact progress percentages, the i18n wording (en
and vi), and whether `ByHand` carries the composed command as well as the two paths.
