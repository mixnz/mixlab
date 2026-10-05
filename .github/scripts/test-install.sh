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

check "a plain version" "0.0.14" "$(normalize_version 0.0.14)"
check "a tag" "0.0.14" "$(normalize_version v0.0.14)"
check "two numbers is not a version" "refused" "$(normalize_version 0.14 || echo refused)"
check "letters are not a version" "refused" "$(normalize_version 0.0.x || echo refused)"
check "an empty version" "refused" "$(normalize_version '' || echo refused)"

check "0.0.14 >= 0.0.8" "yes" "$(version_at_least 0.0.14 0.0.8 && echo yes || echo no)"
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
check "macos window, versioned" "mixlab-0.0.14-macos-universal.pkg" \
  "$(artifact_name macos aarch64 '' window 0.0.14)"
check "macos headless, versioned" "mixengine-0.0.14-macos-universal-headless.pkg" \
  "$(artifact_name macos aarch64 '' headless 0.0.14)"
check "deb window" "mixlab_amd64.deb" "$(artifact_name linux x86_64 apt window '')"
check "deb headless arm" "mixengine-headless_arm64.deb" \
  "$(artifact_name linux aarch64 apt headless '')"
check "deb window, versioned" "mixlab_0.0.14-1_amd64.deb" \
  "$(artifact_name linux x86_64 apt window 0.0.14)"
check "deb headless, versioned" "mixengine-headless_0.0.14-1_arm64.deb" \
  "$(artifact_name linux aarch64 apt headless 0.0.14)"
check "rpm window" "mixlab-x86_64.rpm" "$(artifact_name linux x86_64 dnf window '')"
check "rpm headless zypper" "mixengine-headless-aarch64.rpm" \
  "$(artifact_name linux aarch64 zypper headless '')"
check "rpm window, versioned" "mixlab-0.0.14-1.x86_64.rpm" \
  "$(artifact_name linux x86_64 dnf window 0.0.14)"
check "rpm headless, versioned" "mixengine-headless-0.0.14-1.aarch64.rpm" \
  "$(artifact_name linux aarch64 zypper headless 0.0.14)"
check "an unknown manager is refused" "refused" \
  "$(artifact_name linux x86_64 pacman window '' || echo refused)"

check "latest" "https://github.com/mixnz/mixlab/releases/latest/download" "$(release_base '')"
check "a version" "https://github.com/mixnz/mixlab/releases/download/v0.0.14" \
  "$(release_base 0.0.14)"

# 2 flavours x (1 macOS package + 2 architectures x 2 Linux families) = 10.
check "every name, unversioned" "10" "$(print_names '' | wc -l | tr -d ' ')"
check "every name, versioned" "10" "$(print_names 0.0.14 | wc -l | tr -d ' ')"
check "no name twice" "10" "$(print_names 0.0.14 | sort -u | wc -l | tr -d ' ')"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "install.sh: every check passed"
