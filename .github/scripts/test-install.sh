#!/bin/sh
# T197: the install script's choices, checked without a network or an installer.
#
# Sourced with MIXLAB_INSTALL_SOURCED=1, so `main` does not run and each function is called on its
# own. Runs under whatever `sh` is: dash on the Linux runner, which is the strict one.

set -eu

root="$(cd "$(dirname "$0")/../.." && pwd)"
MIXLAB_INSTALL_SOURCED=1
. "$root/packaging/install/install.sh"

failures=0
check() {
  if [ "$2" = "$3" ]; then
    return 0
  fi
  printf 'FAIL %s\n  expected: %s\n  actual:   %s\n' "$1" "$2" "$3" >&2
  failures=$((failures + 1))
}

# The key in the script is the product's key.
check "the embedded key is packaging/updates.pub's" \
  "$(sed -n '2p' "$root/packaging/updates.pub" | tr -d '\r')" "$MIXLAB_PUBKEY"

# The current release is never written in these files: scripts/set-version.mjs refuses a bump while
# packaging/ or .github/ types it out, so every example names a fixed older release instead.
current="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
for typed in packaging/install/install.sh packaging/install/install.ps1 \
  .github/scripts/test-install.sh .github/scripts/test-install.ps1; do
  check "$typed does not name the current release" "no" \
    "$(grep -E "(^|[^0-9.])$(printf '%s' "$current" | sed 's/\./\\./g')([^0-9]|$)" "$root/$typed" >/dev/null && echo yes || echo no)"
done

check "a plain version" "0.0.13" "$(normalize_version 0.0.13)"
check "a tag" "0.0.13" "$(normalize_version v0.0.13)"
check "two numbers is not a version" "refused" "$(normalize_version 0.14 || echo refused)"
check "letters are not a version" "refused" "$(normalize_version 0.0.x || echo refused)"
check "an empty version" "refused" "$(normalize_version '' || echo refused)"

check "0.0.13 >= 0.0.8" "yes" "$(version_at_least 0.0.13 0.0.8 && echo yes || echo no)"
check "0.0.7 < 0.0.8" "no" "$(version_at_least 0.0.7 0.0.8 && echo yes || echo no)"
check "glibc 2.39 >= 2.35" "yes" "$(version_at_least 2.39 2.35 && echo yes || echo no)"
check "glibc 2.31 < 2.35" "no" "$(version_at_least 2.31 2.35 && echo yes || echo no)"
check "equal is enough" "yes" "$(version_at_least 2.28 2.28 && echo yes || echo no)"

check "amd64" "x86_64" "$(machine_arch amd64)"
check "x86_64" "x86_64" "$(machine_arch x86_64)"
check "arm64 (macOS)" "aarch64" "$(machine_arch arm64)"
check "aarch64" "aarch64" "$(machine_arch aarch64)"
check "i686 is refused" "refused" "$(machine_arch i686 || echo refused)"

check "macos window" "mixlab-macos-universal.pkg" "$(artifact_name macos aarch64 '' window '')"
check "macos headless" "mixengine-macos-universal-headless.pkg" \
  "$(artifact_name macos x86_64 '' headless '')"
check "macos window, versioned" "mixlab-0.0.13-macos-universal.pkg" \
  "$(artifact_name macos aarch64 '' window 0.0.13)"
check "macos headless, versioned" "mixengine-0.0.13-macos-universal-headless.pkg" \
  "$(artifact_name macos aarch64 '' headless 0.0.13)"
check "deb window" "mixlab_amd64.deb" "$(artifact_name linux x86_64 apt window '')"
check "deb headless arm" "mixengine-headless_arm64.deb" \
  "$(artifact_name linux aarch64 apt headless '')"
check "deb window, versioned" "mixlab_0.0.13-1_amd64.deb" \
  "$(artifact_name linux x86_64 apt window 0.0.13)"
check "deb headless, versioned" "mixengine-headless_0.0.13-1_arm64.deb" \
  "$(artifact_name linux aarch64 apt headless 0.0.13)"
check "rpm window" "mixlab-x86_64.rpm" "$(artifact_name linux x86_64 dnf window '')"
check "rpm headless zypper" "mixengine-headless-aarch64.rpm" \
  "$(artifact_name linux aarch64 zypper headless '')"
check "rpm window, versioned" "mixlab-0.0.13-1.x86_64.rpm" \
  "$(artifact_name linux x86_64 dnf window 0.0.13)"
check "rpm headless, versioned" "mixengine-headless-0.0.13-1.aarch64.rpm" \
  "$(artifact_name linux aarch64 zypper headless 0.0.13)"
check "an unknown manager is refused" "refused" \
  "$(artifact_name linux x86_64 pacman window '' || echo refused)"

check "latest" "https://github.com/mixnz/mixlab/releases/latest/download" "$(release_base '')"
check "a version" "https://github.com/mixnz/mixlab/releases/download/v0.0.13" \
  "$(release_base 0.0.13)"

# 2 flavours x (1 macOS package + 2 architectures x 2 Linux families) = 10.
check "every name, unversioned" "10" "$(print_names '' | wc -l | tr -d ' ')"
check "every name, versioned" "10" "$(print_names 0.0.13 | wc -l | tr -d ' ')"
check "no name twice" "10" "$(print_names 0.0.13 | sort -u | wc -l | tr -d ' ')"

# A signed release served from this machine: a throwaway key, a fake artifact signed the way
# packaging/sign.sh signs (trusted comment `mixengine <version> <file>`), and a .sha256 beside it.
work="$(mktemp -d)"
server_pid=""
cleanup() {
  if [ -n "$server_pid" ]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

password="not-the-release-key"
printf '%s\n%s\n' "$password" "$password" |
  minisign -G -p "$work/test.pub" -s "$work/test.key" >/dev/null 2>&1
key="$(sed -n '2p' "$work/test.pub" | tr -d '\r')"

release="$work/release/download/v9.9.9"
mkdir -p "$release"
name="mixengine-headless_9.9.9-1_amd64.deb"
echo "a package" >"$release/$name"
(cd "$release" && file_sha256 "$name" | awk -v n="$name" '{ print $1 "  " n }' >"$name.sha256")
printf '%s\n' "$password" |
  minisign -S -H -s "$work/test.key" -t "mixengine 9.9.9 $name" -m "$release/$name" >/dev/null 2>&1
# The same bytes signed for another file, which must be refused under this name.
cp "$release/$name" "$release/other.deb"
printf '%s\n' "$password" |
  minisign -S -H -s "$work/test.key" -t "mixengine 9.9.9 mixlab_9.9.9-1_amd64.deb" \
    -m "$release/other.deb" >/dev/null 2>&1

port=8765
(cd "$work/release" && exec python3 -m http.server "$port" >/dev/null 2>&1) &
server_pid=$!
base="http://127.0.0.1:$port/download/v9.9.9"
tries=0
until url_answers "$base/$name"; do
  tries=$((tries + 1))
  [ "$tries" -lt 50 ] || { echo "the test server did not start" >&2; exit 1; }
  sleep 0.1
done

got="$work/got"
mkdir -p "$got"
fetch "$base/$name" "$got/$name"
fetch "$base/$name.sha256" "$got/$name.sha256"
fetch "$base/$name.minisig" "$got/$name.minisig"
check "a file answers" "yes" "$(url_answers "$base/$name" && echo yes || echo no)"
check "a missing file does not" "no" "$(url_answers "$base/nothing.deb" && echo yes || echo no)"
check "the checksum matches" "yes" "$(check_sha256 "$got/$name" "$got/$name.sha256" && echo yes || echo no)"

echo "tampered" >>"$got/$name"
check "a changed file fails its checksum" "no" \
  "$(check_sha256 "$got/$name" "$got/$name.sha256" && echo yes || echo no)"
fetch "$base/$name" "$got/$name"

ms="$(command -v minisign)"
check "the signature and its name" "9.9.9" \
  "$(check_signature "$ms" "$key" "$got/$name" "$name" "" 2>/dev/null)"
check "and the version asked for" "9.9.9" \
  "$(check_signature "$ms" "$key" "$got/$name" "$name" 9.9.9 2>/dev/null)"
check "another version is refused" "refused" \
  "$(check_signature "$ms" "$key" "$got/$name" "$name" 9.9.8 2>/dev/null || echo refused)"
check "another key is refused" "refused" \
  "$(check_signature "$ms" "$MIXLAB_PUBKEY" "$got/$name" "$name" "" 2>/dev/null || echo refused)"

cp "$release/other.deb" "$got/$name"
cp "$release/other.deb.minisig" "$got/$name.minisig"
check "a file signed under another name is refused" "refused" \
  "$(check_signature "$ms" "$key" "$got/$name" "$name" "" 2>/dev/null || echo refused)"

# An Intel Mac with no minisign stops, and says what to install.
intel="$( (
  # An empty search path is the point: this is a machine with no minisign.
  # shellcheck disable=SC2123
  PATH="/nonexistent"
  minisign_for macos x86_64 "$work" 2>&1
) || echo refused)"
case "$intel" in
  *"brew install minisign"*refused) check "an intel mac is refused" "ok" "ok" ;;
  *) check "an intel mac is refused" "brew install minisign … refused" "$intel" ;;
esac

check "macos command" "installer -pkg /t/x.pkg -target /" "$(install_command macos '' /t/x.pkg)"
check "apt command" "env DEBIAN_FRONTEND=noninteractive apt-get install -y /t/x.deb" \
  "$(install_command linux apt /t/x.deb)"
check "dnf command" "dnf install -y /t/x.rpm" "$(install_command linux dnf /t/x.rpm)"
check "zypper command" "zypper --non-interactive install --allow-unsigned-rpm /t/x.rpm" \
  "$(install_command linux zypper /t/x.rpm)"

# main, run as the user runs it, through the script file.
script="$root/packaging/install/install.sh"
check "an unknown option" "refused" "$(sh "$script" --nope 2>/dev/null || echo refused)"
check "a bad version" "refused" "$(sh "$script" --version 1.2 2>/dev/null || echo refused)"
check "a version before 0.0.8" "refused" "$(sh "$script" --version 0.0.7 2>/dev/null || echo refused)"
check "--help says how to pass options" "yes" \
  "$(sh "$script" --help 2>&1 | grep -q -- 'sh -s -- --headless' && echo yes || echo no)"
check "--print-names" "10" "$(sh "$script" --print-names 0.0.13 | wc -l | tr -d ' ')"
check "--print-names with a bad version" "refused" "$(sh "$script" --print-names 1.2 2>/dev/null || echo refused)"

# A dry run against the published releases; skipped with no network.
if url_answers "https://github.com/mixnz/mixlab/releases/latest" 2>/dev/null; then
  dry="$(sh "$script" --dry-run --headless 2>&1)" || dry="FAILED: $dry"
  case "$dry" in
    *FAILED*) check "a dry run" "passes" "$dry" ;;
    *"/releases/latest/download/"*".minisig"*) check "a dry run" "ok" "ok" ;;
    *) check "a dry run lists the signature" "a .minisig URL" "$dry" ;;
  esac
fi

# The installer gets no stdin: under `curl | sh` it would be reading the rest of the script.
stdin_probe="$work/probe.sh"
# Single quotes on purpose: $line belongs to the probe, not to this script.
# shellcheck disable=SC2016
printf '#!/bin/sh\nif read -r line; then echo "read: $line"; else echo closed; fi\n' >"$stdin_probe"
chmod +x "$stdin_probe"
check "an install command runs with stdin closed" "closed" \
  "$(echo "leftover script" | run_install "" "$stdin_probe")"

# A path with a space stays one argument: TMPDIR may contain one. run_install is replaced for this
# check by one that prints each argument on its own line.
spaced="$work/with space/x.pkg"
args_of() {
  (
    run_install() {
      shift
      printf '%s\n' "$@"
    }
    install_package "" "$@"
  )
}
check "macos keeps a spaced path whole" "$spaced" "$(args_of macos "" "$spaced" | sed -n '3p')"
check "apt keeps a spaced path whole" "$spaced" "$(args_of linux apt "$spaced" | sed -n '6p')"
check "dnf keeps a spaced path whole" "$spaced" "$(args_of linux dnf "$spaced" | sed -n '4p')"
check "zypper keeps a spaced path whole" "$spaced" "$(args_of linux zypper "$spaced" | sed -n '5p')"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "install.sh: every check passed"
