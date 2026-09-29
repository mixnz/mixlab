# Packaging

What turns a release build into the files a person downloads. One script per operating system, each
run on that system — there is no cross-packaging here, and CI's `build` job is three legs for that
reason.

Design: [`docs/specs/2026-09-04-t85-installers-design.md`](../docs/specs/2026-09-04-t85-installers-design.md).
Release process: [`docs/operations/build-and-release.md`](../docs/operations/build-and-release.md).

## Running it

```bash
bash packaging/desktop.sh            # anywhere:   MixLab, the window — see below
bash packaging/windows/build.sh      # on Windows: a per-user installer, a headless one, and the update zip
bash packaging/macos/build.sh        # on macOS:   one universal .pkg with the window and one without
bash packaging/linux/build-deb.sh    # on Linux:   .deb, with the window and headless
bash packaging/linux/build-rpm.sh    #             .rpm, with the window and headless
bash scripts/build-installer.sh      # anywhere:   picks this OS's line above, tools checked first
```

**The first line is optional and is there for speed.** `stage.sh` runs `desktop.sh` itself when
nothing has staged the window, or when the staged one was built from other sources than the checkout
holds now (its `fingerprint` file differs), so any one of the lines below it works on its own — but the four
Linux scripts each call `stage.sh`, and running it once up front means one of them is not paying for
a ten-minute webview build inside a packaging run. It is a script of its own because the window's
crate is a workspace this one `exclude`s (ADR 0027, rule 5): `cargo build -p mixlab` at the root is
an error rather than a build, so `stage.sh` copies what `desktop.sh` produced instead of compiling
it. Roadmap task **T105**,
[design](../docs/specs/2026-09-09-t105-the-window-in-every-installer-design.md).

Every leg additionally publishes its **`mixengine-elevate` on its own** —
`mixengine-elevate-<version>-<os>-<arch>` — which is the one artifact here that exists for a program
rather than for a person. `mix self-update` never replaces the privileged helper, so a release
cannot deliver it inside a payload; what a machine fetches instead is that file and the `.minisig`
`sign.sh` puts beside it, and the signature's trusted comment is what the elevated process reads to
learn which version and which machine the bytes are for. Roadmap task **T88a**, and
[ADR 0018](../docs/decisions/0018-a-signed-candidate-is-what-lets-a-path-cross-the-boundary.md).

Everything lands in `target/packaging/dist/`, with a `.sha256` beside each artifact. Each script
opens what it just made and checks the six binaries are in it before it exits — five for a headless
archive, which is additionally checked for *not* holding the window. An empty archive is a perfectly
valid archive, and nothing else in the pipeline would notice.

**`mixengine-shim` goes beside `mixengined` in every one of them**, because that is the only place
`core::shims::source` looks. An artifact without it installs cleanly, starts, reports itself healthy,
and leaves `<root>/bin` empty — which is every runtime command the product exists to provide
(roadmap task **T85c**). `mixengine-trampoline` goes beside it for the same reason: on Windows it is
what `<root>/bin` is filled with, and it ships on the other two so there is one list (T185).
`packaging/common.sh` names the six binaries and the six crates that
produce them, in one place, and `crates/mixengine-core/tests/packaging.rs` fails the build when that
list and the names the code looks for drift apart. `MIX_WINDOW` there names the sixth — the one
entry several scripts have to treat differently, and the one a headless archive leaves out.

Two pieces here have checks that need no packaging tools and run on any of the three systems, because
what they get wrong is invisible until a release is in somebody's hands:

```bash
bash packaging/feed-check.sh          # feed.sh over a fixture distribution — see below
bash packaging/bindings.sh --check    # the committed API contract is what the crate generates
```

| OS | Artifacts |
| --- | --- |
| Windows | `mixlab-<version>-windows-x86_64-setup.exe`, `mixengine-<version>-windows-x86_64-headless-setup.exe`, and `mixlab-<version>-windows-x86_64.zip` (the update payload, not linked for installing) |
| macOS | `mixlab-<version>-macos-universal.pkg`, `mixengine-<version>-macos-universal-headless.pkg` |
| Linux | `mixlab_<version>-1_amd64.deb`, `mixlab-<version>-1.x86_64.rpm`, `mixengine-headless_<version>-1_amd64.deb`, `mixengine-headless-<version>-1.x86_64.rpm` |

**What carries the window is named `mixlab`, and what does not keeps MixEngine's name** —
[ADR 0049](../docs/decisions/0049-a-download-is-named-after-what-it-installs.md). The two prefixes
are `MIX_ARTIFACT` and `MIX_HEADLESS_ARTIFACT` in `common.sh`, and every script builds its names
from them. The `.deb` and the `.rpm` are the package `mixlab`, which replaces an installed
`mixengine`. Only the file names moved: the `mixengine/` directory inside each archive, the install
paths and the helper's own asset keep their names, because an installed copy reads them.

In the `.deb` and the `.rpm` alone, `<version>` is `mix_native_version` rather than the version as
written: neither format can hold the `-` of a pre-release, so `0.0.1-beta.1` is named
`0.0.1~beta.1` there and as written everywhere else. `common.sh` says why.

**On a CI branch other than `master`, the macOS names say `macos-arm64`** instead of
`macos-universal`: `MIX_MACOS_SLICES=aarch64` builds the one slice and checks x86_64 without
building it (T171c). Those files do not run on an Intel Mac, and `macos/build.sh` refuses to make
them for a tag. Unset, as on a developer's machine, both slices are built.

**Every installer in the table above, and every headless archive, is published a second time under a
name with no version in it** — `mixlab-windows-x86_64-setup.exe` beside
`mixlab-<version>-windows-x86_64-setup.exe`, and so on for each of the others — through
`mix_publish_alias` in `common.sh`. That is what lets the handbook's install page link
`.../releases/latest/download/<name>` and never need editing again: GitHub resolves that URL to
whichever release is newest and not a pre-release, the same mechanism `latest.json` already relies
on (T88). The update payloads and `latest.json` itself are not aliased — nothing downloads those by
hand.

## The update payload, and the feed

One artifact is not an installer at all: the Windows zip, a plain archive of the release's binaries,
which is what `mix self-update` applies to a per-user Windows install — roadmap task **T88**. The
`.deb`, the `.rpm` and the `.pkg` need root, so a copy one of them placed is updated by the next
package of its own kind, listed in the feed's `installers` (T88f, and T182b for Linux). macOS and
Linux have no payload since T182b.

It holds **one top-level `mixengine/` directory**, which is what lets one `provides` shape in
the feed describe every artifact this project ships — and what stops a zip extracted into `Downloads`
scattering six binaries there.

**A headless archive is never a payload.** None is published since T182b, and `feed.sh` still skips
`*-headless.*` by name: one that got in would produce a second row for an (os, arch) pair that
already has one, and a client takes the first row it matches.

```bash
bash packaging/feed.sh --tag v0.2.0 --repo mixnz/mixlab
```

`latest.json` lists, per operating system and architecture, the payload's URL, its SHA-256 and its
size, and where each binary sits inside it. **It is written into the distribution directory before
`sign.sh` runs**, so it is signed with everything else and `latest.json.minisig` lands beside it under
the name `mixengine_core::index::Client` appends. That signature is the whole chain of trust: an
installed MixEngine verifies the document before parsing it, and then checks the payload against the
SHA-256 the verified document carries.

`provides` maps each executable's **name** to its path inside the payload — `mixengined`, never
`mixengined.exe`, on every operating system. That is `index::format::Artifact`'s own shape, and it is
what `updates::apply` reads: it appends this platform's executable suffix itself. `feed-check.sh`
runs the script over a fixture distribution and asserts exactly that, because the only sign of
getting it wrong is a `mix self-update` that refuses the release it was offered.

**On macOS one `provides` value names a directory** — roadmap task **T106**. A windowed application
there is a bundle, `MixLab.app`, so the macOS payload carries `mixengine/MixLab.app/…` and its rows
read `"mixlab": "mixengine/MixLab.app"`. `feed.sh` emits that row once however many entries the bundle
holds inside it, and `updates::apply::swap` renames the installed bundle aside and copies the new one
in as a tree. The headless archive is built from a root of its own for exactly this reason: one
shared staging directory is how a machine with no display would end up downloading a webview.

macOS is universal, so its one archive is listed under **both** architectures. The notes are the
tag's own commit subjects, read from `git` — the feed is signed before the draft release exists, so
GitHub's generated notes cannot reach it — with `notes_url` pointing at the page somebody may have
edited afterwards.

The version comes from `[workspace.package]` in the root `Cargo.toml`, so cutting a release is a
version bump and nothing else. Host architecture only: the second architecture on Windows and Linux
is roadmap task **T85a**, and macOS is universal here because Apple's toolchain builds the other
slice with no extra sysroot.

## The API contract

```bash
bash packaging/bindings.sh            # regenerate bindings/ in place
bash packaging/bindings.sh --check    # regenerate into a temp dir and diff; writes nothing
bash packaging/bindings.sh --pack     # archive the committed tree into dist; runs no cargo
```

`bindings/` at the repository root is the MixEngine API as TypeScript: every request, response,
event and error, generated from `mixengine-proto` with `ts-rs` and committed — roadmap task **T56**,
[design](../docs/specs/2026-09-05-t56-the-published-api-contract-design.md). The
desktop application under `apps/desktop/` is typed against that directory directly
([ADR 0027](../docs/decisions/0027-the-desktop-client-lives-in-this-repository.md)), and it is
published as an archive on every release for any other client.

**Every file in it is generated**, the barrel and its README included, which is what lets `--check`
be a plain `diff -r` with nothing to exclude and what makes a deleted type take its file with it.
Where the files go and how a `u64` is spelled live in `.cargo/config.toml` rather than in the script,
so the obvious command — `cargo test -p mixengine-proto --features ts` — produces exactly the
committed answer.

`--pack` writes `mixengine-api-<version>-typescript.tar.gz`, an installable npm tarball with a single
top-level `package/` and no runtime code in it at all. The version is stamped **there** and is not in
the committed tree, so cutting a release stays a version bump and nothing else. `sign.sh` signs it
beside the binaries, and `feed.sh` leaves it alone: a payload is matched by the
`mixlab-<version>-<os>-…` shape and this is not one.

What the contract states is what the daemon **writes** — a few requests accept more than that, and
[ADR 0020](../docs/decisions/0020-the-published-contract-is-the-shape-the-daemon-writes.md) is
why those alternatives are not described.

## The user handbook

```bash
bash packaging/docs.sh              # build the site into target/site/
bash packaging/docs.sh --reference  # regenerate docs/guide/en/cli.md from `mix` itself
bash packaging/docs.sh --restamp    # rewrite every Vietnamese page's source_sha256
bash packaging/docs.sh --check      # build into a temp dir, validate it, diff the reference
```

`docs/guide/{en,vi}/` is the handbook: sixteen Markdown pages per language, published at
`https://mixnz.github.io/mixlab/` as HTML **and** as plain Markdown at a predictable address, and
compiled into `mix` so that `mix docs <topic>` answers the same bytes with no network and no running
daemon — roadmap task **T90**,
[design](../docs/specs/2026-09-05-t90-the-documentation-site-design.md).

**Unlike `bindings/`, the generated site is not committed**, and the difference is what each is for:
`bindings/` is source code another repository compiles, and this is what a browser receives at a URL.
So `--check` builds into a temporary directory and asserts the shape of what came out; the one thing
it diffs is `docs/guide/en/cli.md`, which is generated by `mix docs --reference` and *is* committed,
because it is a page of the corpus like any other.

**`--restamp` is run after translating a page, never instead of it.** Every Vietnamese page carries
the SHA-256 of the English page it was made from, so editing the English one without revisiting the
Vietnamese one is a failing test rather than a discovery six months later. All the stamp records is
that somebody looked; no machine here can check that a translation is right.

Publishing is `.github/workflows/pages.yml` and not the `release` job: the site follows `master`
rather than a tag, because a handbook that only updated when a version was cut would describe the
previous release for as long as the next one took.

## Signing

```bash
bash packaging/sign.sh          # signs everything in target/packaging/dist
```

Every artifact gets a detached `.minisig` beside it, made with the updater key and **verified back
against the key compiled into `mixengine-core`** before the script returns — so a signature this
product would not accept fails the run rather than reaching a release. `.sha256` files are not signed:
a checksum is for a person who downloaded twice, and a signature over it would be a weaker way of
saying what the signature over the artifact already says. The script also counts, because a release
with one unsigned artifact in it is the failure it exists to prevent.

The private half is not in this repository and never will be. In CI it arrives as
`MIX_SIGN_SECRET_KEY` / `MIX_SIGN_PASSWORD` and is used by one job on one runner; by hand it is read
from `~/.config/mixengine/updates.key` and the password is typed. Roadmap task **T86**,
[design](../docs/specs/2026-09-04-t86-updater-signing-design.md).

## Probing

```bash
bash packaging/windows/probe.sh      # on Windows, after windows/build.sh
bash packaging/macos/probe.sh        # on macOS,   after macos/build.sh
```

What an unsigned release looks like to the machines that judge it — roadmap task **T86a**,
[design](../docs/specs/2026-09-04-t86a-unsigned-distribution-design.md), findings in
[`docs/features/updates.md`](../docs/features/updates.md). Each takes a fixed list of readings
against the artifacts beside it, prints a report, and writes it to `target/packaging/probe/` — which
is **not** `dist/`, because the release job signs and publishes everything it finds in there.

What they measure is the **mark**, not the verdict: SmartScreen is reached through
Mark-of-the-Web and Gatekeeper through `com.apple.quarantine`, so which files ever carry one is a
property of our own artifacts, while the dialog itself needs a browser and a person. That half is
release-checklist item 4 in
[build-and-release.md](../docs/operations/build-and-release.md).

A reading that came back wrong about a MixEngine artifact **fails**; anything the machine could not
answer is printed as a **void reading** under its own heading, so a green run that measured nothing
cannot be read as a green run that measured and found nothing.

Some readings install for real — the NSIS installer into a temporary directory and this account's
`PATH`, the `.pkg` into `/usr/local/bin` and `/Library/PrivilegedHelperTools` as root. Those are
behind `MIX_PROBE_INSTALL=1`, set by CI's `build` job and nowhere else, and are skipped without it.
Both probes put the machine back as they found it, and the macOS one **refuses to run at all** when
there is already a MixEngine installed: it writes the real paths, and removing them afterwards would
take a real installation with it.

Neither probe ever turns a protection off to obtain a reading. A number measured on a machine we
disarmed is about the tampering rather than about the product.

## What is not here

**No OS code signing.** Authenticode and an Apple Developer ID are not purchased
([ADR 0005](../docs/decisions/0005-on-demand-elevation.md)). The minisign signature above is the
other column of that table and is not a substitute for it: it says the file is ours, not that the
operating system will run it without a warning.

**No installer places `mixengine-elevate`.** MixEngine installs it itself, inside the elevation
prompt first-run setup already costs — [ADR 0015](../docs/decisions/0015-the-helper-installs-itself.md).
The `.deb`, the `.rpm` and the `.pkg` ship it at that same path anyway, because they run as root and
can; the operation then finds its work already done. The per-user Windows installer and a source
build cannot, which is why the mechanism is not a packager's.

**No autostart entry.** `ServiceInstaller` is roadmap task **T85b**.
