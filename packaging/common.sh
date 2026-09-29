#!/usr/bin/env bash
# Shared by the per-OS packaging scripts. Sourced, never run.
#
# Bash on all three systems, because CI already runs `shell: bash` on the Windows runner and one
# language for six artifacts is one language to get right. See the T85 design, D9 and D10.

set -euo pipefail

# The repository root, however this was invoked.
MIX_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export MIX_ROOT

# Everything is built and staged under `target/`, which every machine already ignores.
MIX_OUT="$MIX_ROOT/target/packaging"
export MIX_OUT

# The six binaries a release is made of, in the order a reader wants them: the four that install
# into one directory, then the one that does not, then the window.
#
# **`mixengine-shim` is in this list because `core::shims::source` looks for it beside the running
# `mixengined` and nowhere else** — T85c. A release without it starts, reports itself healthy, and
# has an empty `<root>/bin`, which is every runtime command the product exists to provide.
# `crates/mixengine-core/tests/packaging.rs` reads this line and refuses a build where the two have
# drifted apart. **`mixengine-trampoline` is here for the same reason** (T185): on Windows it is what
# `<root>/bin` is filled with. It ships on all three systems so there is one list to keep.
#
# **`mixlab` is MixLab, the window** — T105. One name on every operating system, lower case, because
# `updates::apply::swap` looks a payload's name up as `directory.join(binary_name(name))` and
# `binary_name` appends this platform's executable suffix and nothing else: an install file spelled
# any other way is one every future update would skip without a word. On macOS the *bundle* around
# it is `MIX_WINDOW_APP`; the executable inside it still has this name.
MIX_BINARIES=(mix mixengined mixengine-shim mixengine-trampoline mixengine-elevate mixlab)
export MIX_BINARIES

# The crates that produce them, in the same order. Beside the names rather than inside `stage.sh`,
# because "what is built" and "what is shipped" disagreeing is exactly what T85c was.
#
# **The last one is not built by `stage.sh`** and cannot be: the desktop application's crate is a
# workspace of its own that this one excludes (ADR 0027, rule 5), so `cargo build -p mixlab` at the
# root is an error rather than a build. `packaging/desktop.sh` builds it; this array is what
# `crates/mixengine-core/tests/packaging.rs` holds against the manifests.
MIX_CRATES=(mixengine-cli mixengine-daemon mixengine-shim mixengine-trampoline mixengine-elevate mixlab)
export MIX_CRATES

# The one of the six that is not an ordinary binary, named once so that the several scripts which
# have to treat it differently do not each spell it — T105.
MIX_WINDOW=mixlab
export MIX_WINDOW

# What macOS wraps it in. A webview application there is a directory rather than a file, so this is
# what `packaging/macos/build.sh` places in `/Applications` and what `mix_window_in` returns.
MIX_WINDOW_APP=MixLab.app
export MIX_WINDOW_APP

# **What a release calls the files it publishes** — T176f, ADR 0049. An artifact that installs or
# carries the window is named after the product; one without it is the headless distribution, which
# is MixEngine, and always says `-headless`. Named here once so no script copies a prefix from its
# neighbour, and held to the handbook's download links by `crates/mixengine-core/tests/packaging.rs`.
#
# **Only the file names.** The payload's `mixengine/` directory, `Programs\MixEngine`, the `.pkg`
# identifier and the helper's own asset keep MixEngine's name: those are read by an installed copy.
MIX_ARTIFACT=mixlab
export MIX_ARTIFACT
MIX_HEADLESS_ARTIFACT=mixengine
export MIX_HEADLESS_ARTIFACT

# Where each operating system's installer puts MixEngine — T107.
#
# **Read by the packaging scripts and by `mixengine-platform`, and held together by
# `crates/mixengine-core/tests/packaging.rs`.** `install::program_dirs` is what the daemon, the CLI
# and the window all ask *where is MixEngine*; these three lines are what actually put it there. An
# installer that moved without the lookup following is a machine that has MixEngine and is told it
# does not — no error anywhere, just an install page offered to somebody already past it.
#
# Windows is the sub-path and not the whole one, because the base is a folder each side spells in
# its own language: `$LOCALAPPDATA` to NSIS, `%LOCALAPPDATA%` to a person, `SHGetKnownFolderPath` to
# the platform crate. Only what comes after it is a decision. Single-quoted so bash leaves both
# backslashes alone; `packaging/windows/build.sh` hands it to `makensis` as `INSTALL_SUBDIR`.
MIX_INSTALL_WINDOWS='Programs\MixEngine'
export MIX_INSTALL_WINDOWS

# `/usr/local/bin` and not `/usr/bin`: the `.pkg` is not a system package manager's, and
# `/usr/local` is where a Mac expects one that is not. The window is not here — `MIX_WINDOW_APP`
# goes to `/Applications`, which is the split `install::window_dirs` exists for.
MIX_INSTALL_MACOS=/usr/local/bin
export MIX_INSTALL_MACOS

# What the `.deb` and the `.rpm` write, the window included. They are the only Linux downloads
# since T182b, D5.
MIX_INSTALL_LINUX=/usr/bin
export MIX_INSTALL_LINUX

# The oldest glibc the window runs on, and the WebKitGTK soname it links — T105a, ADR 0028. Both
# are a promise made to a person rather than an implementation detail:
# `packaging/linux/window-floor.sh` holds the binary to them on every Linux build leg, and
# `crates/mixengine-core/tests/packaging.rs` holds both install pages to them.
#
# **2.35 is the glibc of `ubuntu-22.04`, the runner both Linux legs build the window on.** It cannot
# be built in the `manylinux_2_28` container the other five binaries come from, whose WebKitGTK is
# the 4.0 API on libsoup 2 (T103, D12). The binary itself usually needs less than the machine that
# built it, and `window-floor.sh` prints what it really needs; the number *promised* is the build
# machine's, because no distribution sits between the two that would gain from a lower one —
# enterprise Linux 9 is at 2.34 and has no WebKitGTK 4.1 at all.
#
# The command line is unaffected by either. The five binaries keep the container's glibc 2.28 floor
# in every artifact, which is why the headless packages install below the window's floor.
MIX_WINDOW_GLIBC=2.35
export MIX_WINDOW_GLIBC

MIX_WINDOW_WEBKIT=libwebkit2gtk-4.1.so.0
export MIX_WINDOW_WEBKIT

# macOS ships `shasum -a 256` and no `sha256sum`. Defined once here, so the three scripts do not
# each discover it.
if ! command -v sha256sum >/dev/null 2>&1; then
  sha256sum() { shasum -a 256 "$@"; }
fi

# The workspace version, read from the one place it is written.
#
# `sed` over the `[workspace.package]` block rather than `cargo metadata` piped through `jq`: jq is
# not on a Git Bash install, and a release has to be buildable by hand on the machine that cut it.
mix_version() {
  sed -n '/^\[workspace\.package\]/,/^\[/p' "$MIX_ROOT/Cargo.toml" \
    | sed -n 's/^version = "\(.*\)"$/\1/p' \
    | head -1
}

# The privileged helper's own version, which is not the release's — roadmap task T182b, D1. Read out
# of the one constant both the helper and the daemon compile in, so the asset's name and its signed
# stamp say what the helper itself will answer a probe with.
mix_helper_version() {
  sed -n 's/^pub const HELPER_VERSION: &str = "\(.*\)";$/\1/p' \
    "$MIX_ROOT/crates/mixengine-proto/src/privileged.rs" | head -1
}

# The same version, spelled the way a native Linux package manager can order it.
#
# **`.deb` and `.rpm` both read `-` as structure rather than as text**, so a pre-release cannot be
# handed to either as it is written. rpm refuses it outright — `Illegal char '-' (0x2d) in: Version:
# 0.0.1-beta.1`, measured on the first tag that had one — because the hyphen is what separates
# version from release. dpkg is worse than that: it accepts the same string, reads the last hyphen as
# the revision separator, and then orders `0.0.1-beta.1` *above* the `0.0.1` it comes before, so the
# beta would be offered as an upgrade over the final release.
#
# Both formats spell a pre-release with `~`, which sorts before everything including the empty
# string — `0.0.1~beta.1 < 0.0.1` on either system, which is the semver ordering. Build metadata goes
# the same way: `+` is a character `set-version.mjs` allows and neither format has a meaning for.
#
# Only the two native packages use this. Every other artifact is named for the version as written,
# because `packaging/feed.sh` matches payloads by name and `mixengine_core::index` parses semver.
# `'+-'` and not `'-+'`: a set beginning with a hyphen is an option to `tr`, and both spellings of
# the tool say so by failing rather than by translating.
mix_native_version() {
  mix_version | tr '+-' '~~'
}

# The single source of truth for "which architecture is this leg" — T85a design, D4. An explicit
# override takes priority, because the two Linux legs that build inside a container have no `rustc`
# on the runner itself to ask; everywhere else this asks the toolchain that is about to build rather
# than `uname -m`, which an emulated shell can misreport.
mix_host_target() {
  if [ -n "${MIX_TARGET:-}" ]; then
    echo "$MIX_TARGET"
  else
    rustc -vV | sed -n 's/^host: //p'
  fi
}

# The per-package-format spelling of a target triple's architecture. `.deb`'s `Architecture:` field
# wants `amd64`/`arm64` and is translated in `build-deb.sh` alone; every other artifact name in this
# product says `x86_64`/`aarch64`, which is what this returns.
mix_arch_label() {
  case "$1" in
    x86_64-*) echo x86_64 ;;
    aarch64-*) echo aarch64 ;;
    *)
      echo "unrecognised target: $1" >&2
      return 1
      ;;
  esac
}

# `.exe` on the one shell that needs it. Written here rather than in each script that appends it,
# because `stage.sh` and `desktop.sh` have to agree about the name of a file one hands the other.
mix_exe_suffix() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) echo ".exe" ;;
    *) echo "" ;;
  esac
}

# The macOS slices this build makes — T171, E2. Both unless told otherwise, so a tag, `master` and a
# developer's machine build what has always shipped; CI sets `aarch64` alone on any other branch,
# where a second release build of everything bought nothing a `cargo check` does not.
#
# Spelled as a closed list rather than taken apart word by word: a typo here would otherwise be a
# package with one slice missing and a name claiming both.
mix_macos_slices() {
  case "${MIX_MACOS_SLICES:-x86_64 aarch64}" in
    "x86_64 aarch64") echo "x86_64 aarch64" ;;
    "aarch64") echo "aarch64" ;;
    *)
      echo "MIX_MACOS_SLICES is '$MIX_MACOS_SLICES'; it takes 'x86_64 aarch64' or 'aarch64'" >&2
      exit 64
      ;;
  esac
}

# The word a macOS file name carries: what the slices add up to, so that a one-slice build can never
# pass for universal in `dist`.
mix_macos_label() {
  local slices
  slices="$(mix_macos_slices)" || exit $?
  case "$slices" in
    "aarch64") echo "arm64" ;;
    *) echo "universal" ;;
  esac
}

# Which staged window a target uses — T105, D2.
#
# **On macOS the key is the slice set, whatever slice was asked for.** `packaging/macos/build.sh`
# calls `stage.sh` once per architecture and the window is built once for all of them, so keying its
# staging directory by the slice would build it twice and place whichever finished last.
mix_window_key() {
  local slices
  case "$(uname -s)" in
    Darwin)
      slices="$(mix_macos_slices)" || exit $?
      case "$slices" in
        "aarch64") echo "aarch64-apple-darwin" ;;
        *) echo "universal-apple-darwin" ;;
      esac
      ;;
    *) echo "${1:-$(mix_host_target)}" ;;
  esac
}

# The window inside a directory — a bundle on macOS, a file everywhere else. $1 the directory.
mix_window_in() {
  case "$(uname -s)" in
    Darwin) echo "$1/$MIX_WINDOW_APP" ;;
    *) echo "$1/$MIX_WINDOW$(mix_exe_suffix)" ;;
  esac
}

# What a staged window was built from: the release's version, then a hash of the sources on disk.
# `desktop.sh` writes it beside the window and `stage.sh` rebuilds a window whose line differs.
#
# **The sources as they are, not as committed.** A temporary index takes every file under the paths
# the window compiles from — ignored ones left out by `.gitignore`, uncommitted edits and new files
# in — and `write-tree` hashes it. A version alone missed a window staged for 0.0.9 inside a 0.0.10
# setup, and a commit hash would miss a fix still being tried out, which is when a local package is
# built. Outside a git checkout the version is all there is to go on.
mix_window_fingerprint() {
  local index tree
  index="$(mktemp)"
  rm -f "$index"
  if tree="$(cd "$MIX_ROOT" \
    && GIT_INDEX_FILE="$index" git add -A -- apps/desktop bindings crates/mixengine-proto \
      crates/mixengine-platform Cargo.toml 2>/dev/null \
    && GIT_INDEX_FILE="$index" git write-tree 2>/dev/null)"; then
    echo "$(mix_version) $tree"
  else
    echo "$(mix_version)"
  fi
  rm -f "$index"
}

# `MIX_BINARIES` without the window, one per line — what every headless artifact holds, and what
# `stage.sh` builds from the root workspace. Derived rather than declared as a sixth list: a second
# hand-kept array of the same names is what T85c was.
mix_headless_binaries() {
  local binary
  for binary in "${MIX_BINARIES[@]}"; do
    [ "$binary" = "$MIX_WINDOW" ] || printf '%s\n' "$binary"
  done
}

# Runs `cmd` inside `container`, with this repository bind-mounted at `/work` and a rustup toolchain
# matching `rust-toolchain.toml` installed first — T85a, D2/D3. Used by both Linux legs of the `build`
# job, which link against an older glibc than the runner ships by compiling inside a manylinux_2_28
# image rather than on the runner directly. The container always matches the runner's own
# architecture, so this is never cross-compilation, only an older sysroot.
mix_in_container() {
  local container="$1"
  local cmd="$2"
  local channel uid gid
  channel="$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$MIX_ROOT/rust-toolchain.toml")"
  # The container runs as root — it needs to for `dnf install` — and `chown`s `target/` back to the
  # invoking user as its last act, so nothing this leaves behind is root-owned on the host. The
  # packages below are the ones `mixengine-packages`' own AlmaLinux 8 recipe already installs for its
  # PHP/Ruby builds, named here rather than reached for blind.
  uid="$(id -u)"
  gid="$(id -g)"
  # `-e NAME` with no value passes the caller's through, and passes nothing when it is unset.
  # `MIXENGINE_RELEASE` is T95's: the two Linux legs build in here, and a variable that stopped at
  # the container boundary would make exactly those two the artifacts that rename a user's home. The
  # three profile variables are T170h's and T173a's: a branch's `build` leg builds without LTO and
  # at opt-level 1, and a container that did not hear them would keep the slowest settings cargo has
  # on exactly the legs it runs on.
  docker run --rm \
    -v "$MIX_ROOT:/work" -w /work \
    -e MIXENGINE_RELEASE \
    -e CARGO_PROFILE_RELEASE_LTO \
    -e CARGO_PROFILE_RELEASE_CODEGEN_UNITS \
    -e CARGO_PROFILE_RELEASE_OPT_LEVEL \
    "$container" \
    bash -c "
      set -euo pipefail
      dnf install -y dbus-devel openssl-devel perl-core make gcc gcc-c++
      curl --proto '=https' --tlsv1.2 -sSf --retry 6 --retry-delay 10 --retry-max-time 240 --connect-timeout 20 https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain '$channel'
      source \"\$HOME/.cargo/env\"
      $cmd
      chown -R $uid:$gid /work/target
    "
}

# Refuse early and by name, rather than half-building an artifact and failing on the tool that wraps
# it. A missing packaging tool is a machine that was not set up, and the message should say which.
mix_require() {
  local missing=()
  local tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
  done

  if [ ${#missing[@]} -ne 0 ]; then
    echo "missing tools: ${missing[*]}" >&2
    return 1
  fi
}

# Publish one leg's `mixengine-elevate` as a release asset of its own — roadmap task T88a.
#
# **Its own asset and not a file inside the payload archive**, because the signing key exists only in
# the `release` job, after every build leg has uploaded: nothing signed can be inside an artifact a
# build leg produced, and a detached signature is exactly what the elevated process needs in order to
# check a replacement for itself. `sign.sh` signs everything in `dist/`, so this needs no new signing
# machinery at all — and `feed.sh` lists what it finds here.
#
# $1 the file to publish, $2 the os label, $3 the arch label. The `.exe` is carried across from the
# source so a Windows asset is one Windows will run.
mix_publish_helper() {
  local source="$1" os="$2" arch="$3"
  local suffix=""
  case "$source" in
    *.exe) suffix=".exe" ;;
  esac

  local name="mixengine-elevate-$(mix_helper_version)-$os-$arch$suffix"
  cp "$source" "$MIX_OUT/dist/$name"
  mix_checksum "$MIX_OUT/dist/$name"
  echo "$MIX_OUT/dist/$name"
}

# A checksum beside the artifact.
#
# **Not a signature, and never presented as one** — the minisign half is `sign.sh`, which signs the
# artifact itself and deliberately skips these files. What this is for is a person who downloaded
# twice and wants to know whether they got the same file.
mix_checksum() {
  local file="$1"
  (cd "$(dirname "$file")" && sha256sum "$(basename "$file")" >"$(basename "$file").sha256")
}

# A second copy of an installer, published under a name with no version in it — so the handbook's
# install page can link `.../releases/latest/download/<name>` and never need editing again, the same
# trick T88's `latest.json` already relies on (`crates/mixengine-core/src/updates/feed.rs`). GitHub
# resolves that URL to the newest **published, non-draft, non-prerelease** release, which is why this
# is worth nothing until the first such release exists — the handbook says so until then.
#
# Checksummed under its own name rather than copied alongside the versioned one's `.sha256`: that
# file names the file it is beside, and `sha256sum -c` fails on a name mismatch.
#
# Not read by `feed.sh`, which matches payload archives by their versioned shape — an installer, of a
# different extension, with no version in its name, cannot collide with what that script looks for.
#
# $1 the file to alias, $2 the unversioned name to give the copy.
mix_publish_alias() {
  local source="$1" name="$2"
  local dest
  dest="$(dirname "$source")/$name"
  cp "$source" "$dest"
  mix_checksum "$dest"
  echo "$dest"
}
