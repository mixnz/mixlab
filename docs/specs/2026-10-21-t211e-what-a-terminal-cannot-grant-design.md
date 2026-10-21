---
status: approved
date: 2026-10-21
task:
  - T211e
  - T211f
---

# T211e, T211f — What a terminal cannot grant, and the window that waits for one (design)

Two gaps the T211 design left, found on a Mac over SSH on 2026-10-21 (macOS 15, helper 0.1.7, the
`t211-elevation-from-a-terminal` branch at `1c652505`). Everything else
[the T211 design](2026-10-21-t211-elevation-from-a-terminal-design.md) promised held there: a
decline, a grant run by hand from a second session, Ctrl-C at `sudo`'s password, three wrong
passwords, a grant in the same terminal, `mix doctor --repair --yes` and `mix uninstall`.

## Measured, not assumed

1. **`trust-ca-install` cannot finish from a terminal on macOS.** Under `sudo` the helper runs
   `security add-trusted-cert -d … /Library/Keychains/System.keychain` through `launchctl asuser`,
   and macOS answers with a SecurityAgent dialog **on the Mac's own screen**. This is the
   behaviour [macos/trust.rs](../../crates/mixengine-platform/src/macos/trust.rs) documents for
   `apply`: `authenticate-admin` does not exempt root. With somebody logged in at the console, `sudo`
   waits until somebody clicks there. With nobody logged in, `security` fails with *no user
   interaction was possible*, after it has already put the certificate in the keychain untrusted.
2. **MixLab's elevation dialog never shows a terminal's command on a Mac.**
   [ElevationDialog.tsx](../../apps/desktop/src/modules/mixengine/components/ElevationDialog/ElevationDialog.tsx)
   reads `waiting` only when `canPrompt` is false, and on macOS it is true. Pressing Allow while a
   terminal's grant waits gets `conflict`, so the dialog follows that grant, and meanwhile says
   *"Waiting for you to answer the system prompt"* when there is no prompt anywhere.

## T211e — an operation a terminal cannot grant stays queued

### D1. The platform says which operations need the machine's own screen

`Elevation` in `mixengine-platform` gains one method with a default:

```rust
/// Why `op` cannot be granted from a terminal on this system, or `None` when it can.
fn needs_the_screen(&self, op: &PrivilegedOp) -> Option<String> { None }
```

macOS answers `Some(…)` for `PrivilegedOp::TrustCaInstall`. Linux and Windows keep the default. The
mock can be told which operations to answer for, so the daemon's tests run on any system. A fact
about the operating system, behind the trait, with no `#[cfg]` in the daemon.

### D2. A grant raised from a terminal leaves them out

In `preflight`, for `Route::ByHand` only, the rows `needs_the_screen` answers for are taken out of
the batch **and left in the queue**: not dropped, not declined. When nothing else is waiting, the
grant is refused before any job exists, with `privileged_required` and the platform's sentence, and
a hint: *run `mix elevation grant` on this machine itself, or allow it in MixLab*.

`Route::Dialog` is unchanged. A grant raised at the Mac's own screen, from Terminal without SSH or
from MixLab's Allow, still carries the authority.

### D3. Status says so, per row

`PendingOp` gains `needs_the_screen: Option<String>` (`#[serde(default, skip_serializing_if =
"Option::is_none")]`), filled by the daemon's `elevation.status` from D1. Core's `pending` leaves it
`None`. Clients render it and decide nothing:

- `mix elevation status` prints it under the row.
- `mix elevation grant` from a terminal, before `continue? [y/N]`, says *"1 operation stays waiting:
  <sentence>"* when any row carries it.
- MixLab shows the sentence under the row only while a terminal grant is waiting (T211f). At the
  Mac's screen the dialog grants it as before.

### D4. Who pays

A person sitting at the Mac who SSHes into `localhost` could have clicked the dialog, and is now told
to run the command without SSH. This cannot be told apart from a session at another machine, so it
takes the side where nothing hangs and nothing is half-done.

### D5. The helper refuses at once where nobody can answer

At the Mac's screen the grant still goes through the dialog, and the dialog can still meet a screen
the caller is not at: Fast User Switching, or a grant started before the person walked away. Before
`add-trusted-cert` runs, `macos::trust::run_in_session` compares the owner of `/dev/console` with the
caller's uid. When they differ (root at the login window, or another account signed in), it refuses
with a sentence naming why and leaves the keychain as it was, instead of waiting on a dialog nobody
sees or leaving the certificate in the keychain untrusted. The check is a pure function in
`trust::screen`, tested on every system. The helper's version is already 0.1.8, above the
released 0.1.7, so the change ships with it.

## T211f — the window shows a terminal's grant wherever there is one

- The dialog reads `elevation.status` while it is open, whatever `canPrompt` says, and shows the
  waiting command with its copy button whenever `waiting` is present.
- While it follows a grant it did not start (the `conflict` path) and that grant is `waiting`, the
  footer says *"Waiting for this command to be run in a terminal"* rather than *"Waiting for you to
  answer the system prompt"*. New i18n key, en and vi.
- The dialog still never starts a terminal grant, and Allow behaves as before.

## Not here

- **T211g:** `mix uninstall` of one home removes resolver wiring, the helper and the audit log that
  other homes on the machine use. It needs an ADR on what belongs to a home and what to the machine,
  and a rule for homes installed before it.
- **T211h:** the display slips the Mac run found, which are older than T211.

## MixLab

T211f is MixLab's half: the elevation dialog shows a terminal's waiting command on every system, and
says what it is waiting for. T211e adds `needs_the_screen` to the rows the dialog already lists. No
new method, so `check-client-surface.mjs` is unaffected.
