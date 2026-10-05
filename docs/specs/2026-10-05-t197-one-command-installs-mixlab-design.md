---
status: implemented
date: 2026-10-05
task: T197
---

# One command installs MixLab

## The problem

Every release publishes an installer for each system, and the handbook's
[install page](../guide/en/install.md) explains each one. Installing from a terminal already works,
but only after the person has found and downloaded the right file by hand:

- macOS: `sudo installer -pkg mixlab-*-macos-universal.pkg -target /`
- Debian and Ubuntu: `sudo apt install ./mixlab_*_amd64.deb`; Fedora: `sudo dnf install ./mixlab-*.x86_64.rpm`
- Windows: the per-user NSIS installer, which runs silently with `/S` but is documented only as a
  double-click.

Choosing the file means knowing the system, the architecture, the package family, and whether the
window is wanted. A person setting up a new machine, and an agent following the handbook, would
rather run one line. That line also has to check what it downloaded, because a release is not
signed by Apple or Microsoft, and its `.minisig` is the only real answer about a file.

## D1. Two scripts, kept on `master`, served by the handbook's site

`packaging/install/install.sh` (macOS and Linux) and `packaging/install/install.ps1` (Windows) live
on `master`. `packaging/docs.sh` copies both into the root of the built site, so `pages.yml`
publishes them on every push to `master`, beside the handbook:

```bash
curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh
```

```powershell
irm https://mixnz.github.io/mixlab/install.ps1 | iex
```

These URLs never change, and a fix to a script needs no release.

**The scripts are not release assets.** A release does not carry them, and `_release.yml` is
unchanged. The cost is that one script on `master` must work with **every published release** a
person can ask for, because it downloads the newest one or the one `--version` names. It depends
only on what is already stable: the unversioned names `releases/latest/download/<name>` resolves,
and the versioned names under `releases/download/v<version>/`, both named by
[ADR 0049](../decisions/0049-a-download-is-named-after-what-it-installs.md). Every release from
v0.0.8 on carries all of them with a `.sha256` and a `.minisig`; `--version` older than that is
refused, because those releases shipped other files. A change to how artifacts are named changes
the scripts in the same commit, and D6's checks fail on a name a release does not have.

A commit to `master` reaches the site before any release, which is harmless: the scripts only point
at releases that are already published.

## D2. What the script chooses

| System | Architecture | With the window (default) | `--headless` |
| --- | --- | --- | --- |
| macOS | either | `mixlab-macos-universal.pkg` | `mixengine-macos-universal-headless.pkg` |
| Linux with `apt` | x86_64, aarch64 | `mixlab_{amd64,arm64}.deb` | `mixengine-headless_{amd64,arm64}.deb` |
| Linux with `dnf` or `zypper` | x86_64, aarch64 | `mixlab-{x86_64,aarch64}.rpm` | `mixengine-headless-{x86_64,aarch64}.rpm` |
| Windows | x86_64, aarch64 | `mixlab-windows-{x86_64,aarch64}-setup.exe` | `mixengine-windows-{x86_64,aarch64}-headless-setup.exe` |

`uname -m` answers `arm64` on macOS and some Linux, and `aarch64` elsewhere; both mean aarch64.
With `--version 0.0.14` (or `v0.0.14`) the versioned name of the same file is used instead, from
that tag: `mixlab_0.0.14-1_amd64.deb`, `mixlab-0.0.14-1.x86_64.rpm`,
`mixlab-0.0.14-macos-universal.pkg`, `mixlab-0.0.14-windows-x86_64-setup.exe`, and the headless
names alike.

The package family on Linux is read from the package manager that is present (`apt-get`, then
`dnf`, then `zypper`), not from `/etc/os-release`, because the manager is what installs the file.
Any other Linux, and any other system or architecture, is refused with one line that points at the
install page's *From source* section. Nothing is guessed.

**On Linux the C library is read before anything is downloaded.** The window needs glibc 2.35 and
WebKitGTK 4.1, the headless programs glibc 2.28, as the install page says. Below the window's floor
the script refuses the window and names `--headless`; below the headless floor, or on musl, it
refuses both. A package manager would refuse the same package later, with a dependency error a
person has to decode.

On Linux with no display (`DISPLAY` and `WAYLAND_DISPLAY` both unset) and no `--headless`, the
script says once that the window needs a desktop and that `--headless` is the package for a server,
then installs what was asked. It does not switch on its own.

## D3. What it checks

1. **The `.sha256` beside the file, always.** It catches a broken or truncated download. It is not
   a signature, and the script does not say it is.
2. **The `.minisig`, always.** The public key from `packaging/updates.pub` is written into both
   scripts, and the script runs `minisign -V -H -P <key>`, the same prehashed form
   `packaging/sign.sh` signs with and `mix self-update` checks. A failed check stops the install and
   deletes the download. There is no way to skip it.
3. **The trusted comment names the file.** `sign.sh` signs every artifact with the trusted comment
   `mixengine <version> <file>`, which the signature covers. The script checks that `<file>` is the
   name it asked for, so a validly signed file served under another name (the headless package for
   the window, one architecture for another) is refused. With `--version` it also checks
   `<version>`; without it, the version read there is the one the script reports installing.
4. **A machine with no `minisign` gets one for the length of the run.** The script downloads one
   pinned release of minisign from its own project (`jedisct1/minisign` on GitHub, 0.12 today):
   `minisign-0.12-linux.tar.gz` holds static x86_64 and aarch64 builds, `minisign-0.12-win64.zip`
   holds x86_64 and aarch64 builds, and `minisign-0.12-macos.zip` holds **an arm64 build only**.
   Each archive is checked against a SHA-256 written into the script beside its URL, unpacked into
   the temporary directory, used once and removed with it. A `minisign` already on `PATH` is used
   instead.
5. **An Intel Mac with no `minisign` is refused**, with one line: `brew install minisign`, then run
   the command again. Upstream publishes nothing it can run, and building one here would make this
   project the publisher of a verifier. An Intel Mac with `minisign` installed is checked like any
   other machine.

Verifying in the script itself does not work everywhere: Ed25519 over a BLAKE2b-512 prehash needs
OpenSSL 3, and macOS ships LibreSSL while Windows PowerShell 5.1 has neither primitive. A pinned
minisign does, and it adds nothing to trust: its hash sits in the script, which is exactly as
trusted as the script.

Two values live in a second place. The key in the scripts is compared with `packaging/updates.pub`
by a check, so a key rotation cannot leave the scripts behind. The pinned minisign is moved by hand,
URL and hash together, and D6 downloads and checks it on every run, so a release of minisign that
is taken down fails CI rather than a person's install.

## D4. How it installs

| System | Command |
| --- | --- |
| macOS | `sudo installer -pkg <file> -target /` |
| Linux, `apt` | `sudo apt-get update`, then `sudo apt-get install -y ./<file>` |
| Linux, `dnf` | `sudo dnf install -y ./<file>` |
| Linux, `zypper` | `sudo zypper --non-interactive install --allow-unsigned-rpm ./<file>` |
| Windows | `<file> /S`, waited for; per-user, no UAC |

`dnf` and `zypper` refresh their metadata on their own. `apt` does not, and a fresh Debian or
Ubuntu has empty lists that cannot resolve the window's dependencies.

`sudo` is dropped when the script already runs as root, and it reads its password from the terminal,
so `curl … | sh` still works. A machine with neither root nor `sudo` is refused before anything is
downloaded. Each command is printed before it runs.

`install.ps1` runs on Windows PowerShell 5.1, which every supported Windows has, as well as on
PowerShell 7. On 5.1 it turns on TLS 1.2 for the session, which GitHub requires and 5.1 does not
always offer by default, and hides the progress bar, which makes `Invoke-WebRequest` many times
slower there.

Installing over an existing install is what each installer already does: it upgrades in place, and
the two Linux packages replace each other. The script adds nothing to that. It does not start the
daemon, change `PATH` beyond what the installer does, or open MixLab. It ends by printing what to
run next: open a new terminal and run `mix status`, or open MixLab.

## D5. How it is written

- `install.sh` is POSIX `sh`, not bash, because `curl | sh` runs whatever `sh` is. It needs only
  `curl` or `wget`, `uname`, `tar`, and `shasum` or `sha256sum`.
- **Everything is inside one function, called on the last line.** A download cut off halfway then
  defines a function and runs nothing, rather than running half a script.
- It works in a temporary directory and removes it on exit, whatever the outcome.
- Options: `--headless`, `--version <x.y.z>`, `--dry-run`, `--help`. `--dry-run` prints the system
  it found, then one URL per line for each file it would fetch (the installer, its `.sha256` and
  `.minisig`, and minisign when it would need one), then the install command. It sends a `HEAD` to
  each URL and fails on any that does not answer `200`, and downloads and installs nothing. Through a pipe they
  go after `sh -s --`. In PowerShell the same options are parameters, passed through
  `& ([scriptblock]::Create((irm https://mixnz.github.io/mixlab/install.ps1))) -Headless`.
- Its messages follow [writing-user-facing-text](../../.claude/skills/writing-user-facing-text/SKILL.md):
  one line each, lowercase, ending with what to do.

## D6. How it is tested

- **shellcheck**, **PSScriptAnalyzer** and the scripts' unit tests, in the same reusable workflow as
  the dry runs, so `pages.yml` is gated on them too.
- **The names a release is about to publish.** On a tag's run, after packaging, the scripts are
  asked for every file name they would choose for that version (each system, architecture and
  flavour) and each must be in `target/packaging/dist/`. This fails before a draft exists, which is
  the only point where a renamed artifact can still be caught without breaking a published
  command.
- **`--dry-run` against the newest release** on Linux, macOS and Windows (D5). This is what fails
  when the scripts and the published names drift apart (D1), and when the pinned minisign is gone.
- **A real install of the headless package** on each OS runner, then `mix --version` prints the
  release's version. Headless, because the runner needs no window and the Linux runner no WebKitGTK.
  On Linux it runs three times: on the Ubuntu runner itself (`apt`), and in a Fedora container
  (`dnf`) and an openSUSE Leap container (`zypper`) on the same runner, as root and so without
  `sudo`. At least one run has no `minisign`, so the pinned download in D3 is exercised every time.
  This tests the script against what is published, not against the branch, which is the point of
  keeping it on `master`.

Every check runs the script from the checkout, not from the site, so a change is tested before
it is published. The dry runs and the real installs need the network and a published release, so
they are a job of their own that `ci.yml` and `pages.yml` both call, not part of `test`. **`pages.yml`
deploys only when the lint and the dry runs pass**, so a broken script never reaches the URL; the
real installs are too slow for every push to `master` and run when CI is asked, as the rest of it
is.

## What this does not do

- **No Homebrew, winget or Scoop.** Each one is a repository or a pull request somewhere else, to be
  updated on every release. That is worth doing once there is a stable release, as a task of its
  own.
- **No uninstall script.** Each system's own removal already works and is documented.
- **No updates.** `mix self-update` and MixLab's Settings → Updates stay the only ways a new version
  is offered. The script runs only when a person types it, which keeps *nothing updates unasked*
  true.
- **No other Linux.** Arch, NixOS, Gentoo, Void and the musl distributions have no package in a
  release, and a script cannot invent one. A per-user `.tar.gz` of the headless programs, and
  perhaps an AppImage for the window, would reach them. Both shipped until
  [ADR 0053](../decisions/0053-the-helper-has-its-own-version-and-follows-the-product.md) removed
  them, so bringing either back is a new decision and its own task (T198). Until then the script refuses them in one line
  that points at *From source*.
- **No MixLab without MixEngine.** Every installer that carries the window also carries the four
  command-line programs today, and this does not change that.

## MixLab

The window has no part in this. The script installs MixLab with the installers that already exist,
and nothing in MixLab changes. The handbook's install page, in English and Vietnamese, gets the one-line
command at the top of each system's section, with the download-and-read alternative beside it, and
`for-agents.md` gets it too, since an agent is one of the people this is for. `CHANGELOG.md` gets an
`### Added` line.

## Roadmap

A new phase after phase 35, with one task: **T197**, the two scripts, `docs.sh` publishing them, the
checks in D6, and the handbook pages in English and Vietnamese.

## Decisions taken while writing this

1. **The signature is always checked**, with a pinned minisign for a machine that has none (D3),
   rather than the checksum alone with an opt-in to be strict. Agreed 2026-10-05.
2. **openSUSE stays**, through `zypper`, and CI installs on it in a container (D6). Agreed
   2026-10-05.

## Open questions

1. An Intel Mac with no `minisign` is refused (D3, 5). The alternative is to build minisign as a
   universal binary in this repository's CI and publish it once, pinned like the others, which makes
   this project a publisher of a verifier. Refusing is proposed: Intel Macs are a shrinking share,
   and a person on one who installs from a terminal very likely has Homebrew.
