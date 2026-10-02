---
status: approved
date: 2026-10-02
---

# MixLab.app is signed ad-hoc as a bundle

## The problem

`MixLab.app` has never been signed as a bundle. On 2026-10-01 an Apple-silicon Mac checked three
copies: the `MixLab.app` installed there from a release `.pkg`, the universal window of CI run
36829166545, and an arm64-only window built on that Mac.

- `codesign --verify --strict` fails on all three: `code object is not signed at all` for the
  universal builds, and `code has no resources but signature indicates they must be present` for
  the arm64-only one.
- `codesign -dv` shows only the linker's signature on the executable:
  `flags=0x20002(adhoc,linker-signed)`, `Info.plist=not bound`, `Sealed Resources=none`, and an
  identifier made up by the linker (`mixlab-6362b50039344494`), not the bundle's
  `io.github.mixnz.mixlab`.
- In the universal binary the x86_64 slice carries no signature at all, because the linker signs
  arm64 slices alone.

The app runs: an Apple-silicon Mac requires a valid signature on the executable, and the linker's
is one. What is missing is everything a bundle signature adds on top: `Info.plist` bound to the
code, the resources sealed, both slices signed, and the bundle's own identifier.

## What this does not change

This is **ad-hoc** signing, `codesign --sign -`, which needs no certificate and no Apple Developer
account. Both of the things a person actually meets stay as they are:

- **Gatekeeper still warns about the `.pkg`.** Only a Developer ID signature with notarization
  ends that, and the release notes say the product is unsigned on purpose.
- **The Keychain still asks after every update.** An ad-hoc designated requirement is the code
  directory hash of that exact build, so every build is a new identity to the Keychain, as now.
  The rule in `apps/desktop/CLAUDE.md`, one Keychain item and so one prompt per launch, is
  unaffected.

The value is a bundle that macOS and its tools can verify. It also puts the signing step where a
Developer ID would one day go: the same line, with a certificate instead of `-`, and a
notarization step after it.

## D1. Sign in `packaging/macos/build.sh`, after the bundle is final

`build.sh` changes the bundle after Tauri has built it: it copies `mixengine-elevate` into
`Contents/Resources` (T88d) and sets modes. A signature made earlier, for instance through
Tauri's `bundle.macOS.signingIdentity = "-"`, would seal resources that this step then changes,
and the seal would be broken on every install. So the signature is the **last** change to the
bundle in `build.sh`, after the helper copy and the `chmod`, before `pkgbuild`:

```bash
codesign --force --deep --sign - "$root/Applications/$MIX_WINDOW_APP"
codesign --verify --strict --deep --verbose=2 "$root/Applications/$MIX_WINDOW_APP"
```

`--deep` signs the code nested in the bundle the way `codesign` finds it: the executable's two
slices, and any helper in `Contents/MacOS`. The helper copy in `Contents/Resources` is expected to
be a resource to `codesign`, sealed by its hash and not re-signed. Whether its bytes stay exactly
as `build.sh` copied them is checked, not assumed (Verification).

**The one question only a Mac answers.** The helper copy is itself a universal Mach-O whose x86_64
slice is unsigned, for the reason the window's is. If `codesign --verify --strict --deep` refuses an
unsigned binary inside `Resources`, the bundle cannot pass D1's check as written. The answer then
is to sign the helper copy ad-hoc first (`codesign --force --sign -` on that one file), which
changes its bytes. That costs nothing in the product, because helper currency is decided by
`HELPER_VERSION` (T182b), not by bytes. The copy would no longer be byte-for-byte the
`mixengine-elevate-<version>-macos-universal` asset the release signs with minisign. Nothing asks it
to be: [the T88d design](2026-09-11-t88d-a-helper-a-machine-can-reinstall-design.md) copies it
into place and checks no bytes or signature, and this spec's Outcome would record which way it
went. The CI run of the branch decides which of the two is built,
and the plan carries both.

The headless `.pkg` carries no window and is unaffected. `packaging/desktop.sh`'s staged window,
the `window-<os>` artifact, stays as Tauri left it: it is for trying a build, and nothing installs
it.

## D2. The probe checks the installed bundle

`packaging/macos/probe.sh` already records M6, the installed `mix` and `mixengined`. It gains the
installed `/Applications/MixLab.app`: `codesign --verify --strict --deep` must pass, and
`codesign -dv` is recorded. A failure fails the probe, as an unrunnable `mix` does.

## Risks

- **macOS may ask once more for a permission it had already granted**, on the first launch after
  the update that brings this, because the app's identity changes from the linker's identifier to
  the bundle's. Local network access is the one MixLab is likely to hold. This happens once, and
  it is the same prompt a person met on first install.
- **`--deep` is discouraged by Apple for Developer ID signing**, which prefers signing nested code
  inside out. For an ad-hoc signature with no nested bundles it is the simple, correct call. A
  Developer ID change would revisit it.

## Verification

- CI's `build` job on macOS: `build.sh`'s own `codesign --verify --strict --deep` (D1), and the
  probe's M6 line on the installed bundle (D2).
- On an Apple-silicon Mac, from the full `.pkg` of a branch run, expanded and not installed:
  - `codesign -dv` shows `Identifier=io.github.mixnz.mixlab`, `Info.plist` bound and resources
    sealed;
  - `codesign --verify --strict --deep` passes;
  - both slices are signed (`codesign -dv --arch x86_64`);
  - `Contents/Resources/mixengine-elevate` is byte-for-byte the staged helper (`cmp`), or, if D1's
    fallback was needed, differs from it only by its signature. In that case `codesign -dv` is
    recorded on it, and run with no arguments it still exits with its own `mixengine-elevate:`
    complaint rather than being killed (`Killed: 9` is what a refused signature looks like);
  - installed over the current release, MixLab launches, opens a saved connection, and the
    Keychain asks no more than once.
- A full `release-exact` run before the branch merges.

## MixLab

No screen. This changes how the macOS installer's copy of MixLab is signed, and nothing in the window
reads its own signature. A person may see one permission prompt again on the first launch after
updating (Risks), and nothing else.
