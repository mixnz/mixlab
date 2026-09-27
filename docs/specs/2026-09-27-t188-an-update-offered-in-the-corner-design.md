---
status: draft
date: 2026-09-27
task: T188
---

# T188 — An update offered in the corner

Roadmap task T188, in [phase 31](../roadmap/phase-31-mixlab-updates-itself.md), after the
[T187 design](2026-09-26-t187-mixlab-updates-itself-design.md). 2026-09-27.

T187 built the whole updater and put the offer in two places: a strip across the top of the window,
shown once per version, and Settings → Updates, where *Install* downloads, swaps and relaunches in
one click. MixDB had something people liked better: a panel in the bottom-right corner that said a
release was out, downloaded it when asked, and then asked again before installing. It was removed
by T106 along with MixDB's own updater, and nothing replaced the corner.

This task brings the corner back on top of T187's machinery. **It changes D9 of the T187 design**
and nothing else in it: the feed, the placement, the lock, the swap and the recovery stay as built.

## D1 — Two steps, each asked for

The corner panel walks through these states, and every step forward is a click:

| State | The panel says | Buttons |
| --- | --- | --- |
| **Offer** | *MixLab 0.0.10 is available*, the size, up to three lines from the release notes | **Download**, **Later**, and a small *Skip this version* link |
| **Downloading** | a progress bar with bytes received out of the total | **Cancel** |
| **Ready** (Windows) | *MixLab 0.0.10 is ready to install*, and when a daemon runs, *MixEngine and N services will restart* | **Install and restart**, **Later** |
| **Ready** (macOS, Linux) | *The installer for 0.0.10 is downloaded* | **Open installer**, **Later** |
| **Failed** | what went wrong, in the backend's words | **Try again**, **Close** |

- **Later** hides the panel until the next window start, as T187's *Remind me later* does. A
  download that finished stays on disk, so the next start opens straight into **Ready** without
  downloading again.
- **Skip this version** is T187's *Skip*: the panel stays away until a newer release appears.
- **Close** on a failure is *Later*.
- **The panel shows once per window start while an offer is pending**, not once per version: a
  person who pressed *Later* has asked to be reminded. T187's once-per-version strip is removed,
  because two notices for one release is one too many. The dot on the Settings button stays.
- The panel never covers a dialog: it sits under modals and above tab content, 16 px from the right
  and bottom edges, and it is at most 360 px wide.

Settings → Updates keeps working as it does, and reads the same state. A download started in the
corner shows its progress in the pane, and the other way round.

## D2 — The backend splits `update_install` in two

T187's `install_swap` runs D4 steps 1 to 8 as one function. It is split at the point where nothing
installed has been touched yet:

- **`update_download`** — T187 D4 steps 2 to 4 on Windows: download into
  `updates/<version>/payload.zip`, resuming a partial file, check the SHA-256, unpack, smoke test.
  On macOS and Linux, D5 step 1: download the installer and check its SHA-256. It reports progress
  through a `Channel` exactly as `update_install` does today, and it can be cancelled. On success it
  writes `updates/<version>/ready` with the SHA-256 it proved.
- **`update_install`** — T187 D4 steps 1 and 5 to 8 on Windows, from the staged directory. It refuses
  with `error.updateNotDownloaded` when `ready` is missing or names another version, which is what a
  cleared cache looks like. The lock is still taken first, before anything is stopped.
- **`update_open_installer`** — D5 steps 2 to 4 on macOS and Linux, from the downloaded file. The
  existing handover and *Finish* are reused as they are.
- **`update_status`** gains `downloaded: string | null`, the version a `ready` file names, so a
  window starting with a finished download opens in **Ready**.
- **`update_cancel_download`** stops the transfer. The partial file stays for the next attempt.

`updates/<version>/` for any version other than the offered one is removed at start, as the swap
already removes its own staging directory.

## D3 — What does not change

- **Nothing downloads on its own.** The check still only reads the feed (T187 D6). The panel's
  *Download* is the click ADR 0056 asks for, and *Install and restart* is a second one.
- The automatic-check switch, the 30-second first check and the 24-hour interval stay.
- A development build and an install placed *elsewhere* (T187 D3) never show the panel, as they
  never showed the strip.
- MixEngine's own updater, `mix self-update`, is untouched.

## D4 — Where it lives

- `src/shell/components/UpdatePanel/` — the component, with its CSS module, drawn by `Workspace`
  in place of the update `TabNotice`. It uses `Button` and `ProgressBar`-like markup from
  `src/components/`, and adds a shared component only if two places need it.
- `src/shell/update/view.ts` gains the panel's states, derived purely from `UpdateStatus` plus the
  hook's local state (downloading, failed), so they are unit-tested the way `updateView` is.
- The release-note lines reuse MixDB's `highlights()` rule from the removed `UpdateToast`: bullets
  under the version's headings, Markdown stripped, three at most, each cut at 120 characters.
- Every string through `t()`, in `en.ts` and `vi.ts`, written with the `writing-user-facing-text`
  skill.

## MixLab

All of it: this task is MixLab's own updater, in the shell, drawn whatever modules are visible
(ADR 0056). D1 is the corner panel, D4 the files it lives in; Settings → Updates keeps reading the
same state.

## Tests

- `view.test.ts`: every state in D1 from the inputs that produce it, including a start with a
  finished download (**Ready** without a download), *Later* and *Skip*.
- `highlights()` over the three shapes the changelog takes: bullets, bullets that wrap, prose.
- Rust: `update_install` refuses without a `ready` file and with one naming another version; a
  `ready` file is written only after the smoke test passes; a cancelled download leaves the partial
  file and no `ready`.
- By hand, once, on a Windows release build against a local feed: offer, download, *Later*, restart
  the window, **Ready** without downloading again, install, relaunch.
