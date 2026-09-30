# 0050. A copy the `.pkg` installed is updated by the `.pkg`, through Installer.app

**Status**: Accepted. It narrows the rule in [features/updates.md](../features/updates.md) that
*"the updater applies the archive and never runs an installer"*, for a macOS `.pkg` install and
nothing else. Extended to the Linux `.deb` and `.rpm` by
[0053](0053-the-helper-has-its-own-version-and-follows-the-product.md).
**Date**: 2026-09-23

## Context

T88's updater replaces the binaries in the directory that holds `mixengined`. It first checks that
this account can write there, and refuses a copy it cannot write (`Placement::Managed`) with a
sentence and a link to the releases page. It never elevates, and
[features/updates.md](../features/updates.md) calls that the most important rule on the page.

**On macOS that refusal is the common case, not the edge.** The `.pkg` is the macOS download a
person is pointed at, and it writes three places as root:

- `/usr/local/bin`: `mix`, `mixengined` and `mixengine-shim`;
- `/Applications/MixLab.app`: the window, with a copy of the helper inside it (T88d);
- `/Library/PrivilegedHelperTools/dev.mixengine.elevate`: the installed helper.

The updater can reach none of the three. The helper has no other way forward either:
`elevation.upgrade` refuses `Placement::Managed` (T88d). So every macOS user who installed MixLab
the usual way sees *"something else installed this copy"* and has to find, download and run the
next `.pkg` by hand, every release.

**One machine gets a worse answer than a refusal.** Intel Homebrew takes ownership of
`/usr/local`, so on such a Mac the write probe passes, and the updater swaps the four binaries in
`/usr/local/bin` while leaving `MixLab.app` and the helper at the old version. This follows from
reading the probe; it has not been measured on a machine.

**What other products do.** An application that is only a `.app` bundle the user can write updates
by replacing the bundle; Sparkle does this for most apps outside the App Store. An application that
installs root-owned parts from a `.pkg` updates by installing a newer `.pkg`: Sparkle has a
"package" update mode for exactly this, and Microsoft AutoUpdate and Zoom work the same way. On
Windows, re-running the installer is the common pattern, but MixEngine's Windows install is
per-user and T88's swap already needs no UAC there. On Linux, a `.deb` or an `.rpm` is left to the
package manager.

## Decision

**On macOS, a copy of MixEngine that the `.pkg` installed is updated by handing the next release's
`.pkg` to Installer.app. MixEngine downloads and verifies it, the person installs it, and macOS asks
for the password.**

1. **Which copy.** A copy whose `mixengined` belongs to the package receipt `dev.mixengine.cli`, as
   `pkgutil --file-info` reports it. This is asked **before** the write probe, so the Homebrew Mac
   above is also routed to the installer rather than half-updated. Any other macOS copy (the
   `.tar.gz` payload unpacked somewhere writable) keeps T88's in-place swap.

2. **Bound the way every other artifact is bound.** The feed gains an `installers` list, one entry
   per machine, carrying the `.pkg`'s URL, size and SHA-256 inside the minisign-signed
   `latest.json`. That is T88's D3 rule, and no second key-handling path is added. The field is
   optional, so a feed with it still reads in a copy from before it.

3. **Nothing in MixEngine elevates.** The daemon hands the verified file to Installer.app through
   `mixengine-platform`. Installer.app shows its own screens and asks for the password itself. No
   new `PrivilegedOp` is added, `mixengine-elevate` runs nothing new, and the `.pkg` keeps having
   no `preinstall` or `postinstall` scripts.

4. **Nothing stops before the install.** Services keep running while Installer.app is open. A
   person who cancels has lost nothing, and nothing needs to be put back. Stopping, recording what
   was running and exiting come afterwards, as a separate step taken once the new `mixengined` is on
   disk. The new daemon restores the services exactly as it does after T88's swap, and MixLab
   relaunches itself from the new bundle.

5. **Every other format is unchanged.** Windows and the archives keep the in-place swap. The
   `.deb`, the `.rpm` and the AppImage keep the refusal and the link.

6. **It starts with the release after the one that ships it.** The code that updates is the code
   already installed, so the first release carrying this cannot be reached this way; its `.pkg` has
   to be installed by hand once. v0.0.7, the first release of MixLab (the standalone client and MixEngine as one
   product), is published without it. A `.pkg` user therefore installs v0.0.8 by hand, and
   v0.0.8 → v0.0.9 is the first update that goes through Installer.app.

## Consequences

- A `.pkg` install gets MixEngine, MixLab and the helper from one step, which T88 and T88a together
  could not give it. It is also the only way a packaged helper moves forward, because
  `elevation.upgrade` still refuses a managed placement.
- The person clicks through Installer.app's screens: more clicks than a password prompt alone.
  That is the price of adding no root code. The alternative below is where to go if it proves too
  much.
- The daemon calls something that opens a window. On a Mac where `mixengined` runs without a
  graphical session (started over SSH), the call fails. The failure names the verified file and
  the `installer` command to run by hand.
- Three things are assumed and have to be measured on a Mac before the design is final: whether a
  `.pkg` the daemon downloaded carries the quarantine attribute, and so meets Gatekeeper; whether
  Installer.app replaces a running `mixengined` and a running `MixLab.app` cleanly; and whether an
  `open` from the daemon brings Installer.app to the front. The design spec lists them.

## Alternatives considered

- **A `PrivilegedOp` that runs `installer -pkg` on a package the helper verifies itself**, on
  [ADR 0018](0018-a-signed-candidate-is-what-lets-a-path-cross-the-boundary.md)'s pattern. One
  password prompt and a silent install, as Zoom and Microsoft do. Rejected for now: it widens what
  the root-owned helper does, from a closed list of edits to "run Apple's installer on a file",
  and it would need its own ADR and review. Revisit if clicking through Installer.app is reported
  as a real cost.
- **`preinstall` and `postinstall` scripts that stop and restart the services.** Rejected: they
  run as root, and restarting a user's daemon from root means `launchctl asuser` and a second copy
  of the stop order T88 already owns in the daemon.
- **Reshape the macOS install so the updater can write all of it.** For example `MixLab.app`
  alone, with the command-line tools linked from inside it and the helper registered through
  `SMAppService`. Rejected here as a far larger change to installation, uninstall, T88d's helper
  sources and ADR 0048. It is not ruled out as a later direction.
- **Keep the refusal.** Rejected: it leaves the most common macOS install without updates.
