#!/bin/sh
# One command installs MixLab on macOS and Linux: it picks the installer for this machine, checks
# its checksum, its signature and the name the signature vouches for, and runs it.
#
#   curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh
#   curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh -s -- --headless --version 0.0.14
#
# Roadmap task T197.
# Design: docs/specs/2026-10-05-t197-one-command-installs-mixlab-design.md
#
# Served from `master` through the handbook's site, not from a release, so it has to work with every
# release from MIXLAB_OLDEST on. Everything is a function and `main` is called on the last line: a
# download cut off halfway defines functions and runs nothing. POSIX sh, because `curl | sh` runs
# whatever `sh` is; there is no `local`, so each function prefixes its variables.

MIXLAB_RELEASES="https://github.com/mixnz/mixlab/releases"
# packaging/updates.pub, line 2. .github/scripts/test-install.sh fails when the two differ.
MIXLAB_PUBKEY="RWSELKuybM79fmLhYywhXv8mdDvB3LPCC3TyHTdm2svTti6clLkck051"
# The first release that published every name artifact_name can return.
MIXLAB_OLDEST="0.0.8"
MIXLAB_GLIBC_WINDOW="2.35"
MIXLAB_GLIBC_HEADLESS="2.28"
MIXLAB_HANDBOOK="https://mixnz.github.io/mixlab/en/install/"

# One pinned minisign for a machine that has none, checked against these hashes before it runs.
# Moved by hand, URL and hashes together; the install workflow fetches it on every run.
MINISIGN_URL="https://github.com/jedisct1/minisign/releases/download/0.12"
MINISIGN_LINUX_SHA256="9a599b48ba6eb7b1e80f12f36b94ceca7c00b7a5173c95c3efc88d9822957e73"
MINISIGN_MACOS_SHA256="89000b19535765f9cffc65a65d64a820f433ef6db8020667f7570e06bf6aac63"

say() {
  printf 'mixlab-install: %s\n' "$*" >&2
}

die() {
  say "$*"
  exit 1
}

normalize_version() {
  nv_version="${1#v}"
  case "$nv_version" in
    "" | *[!0-9.]* | .* | *. | *..*) return 1 ;;
  esac
  [ "$(printf '%s' "$nv_version" | awk -F. '{ print NF }')" = 3 ] || return 1
  printf '%s\n' "$nv_version"
}

version_at_least() {
  awk -v have="$1" -v floor="$2" 'BEGIN {
    n = split(have, h, "."); m = split(floor, f, ".")
    top = n > m ? n : m
    for (i = 1; i <= top; i++) {
      a = (i <= n) ? h[i] + 0 : 0
      b = (i <= m) ? f[i] + 0 : 0
      if (a > b) exit 0
      if (a < b) exit 1
    }
    exit 0
  }'
}

machine_arch() {
  case "$1" in
    x86_64 | amd64) echo x86_64 ;;
    arm64 | aarch64) echo aarch64 ;;
    *) return 1 ;;
  esac
}

# The manager that will install the file decides the package family, not /etc/os-release.
linux_manager() {
  if command -v apt-get >/dev/null 2>&1; then
    echo apt
  elif command -v dnf >/dev/null 2>&1; then
    echo dnf
  elif command -v zypper >/dev/null 2>&1; then
    echo zypper
  else
    return 1
  fi
}

# Empty on musl, or on anything that does not say it is glibc.
glibc_version() {
  gv_version="$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{ print $2 }')"
  if [ -z "$gv_version" ]; then
    gv_version="$(ldd --version 2>&1 | head -n 1 | grep -i -E 'glibc|gnu libc' |
      grep -o -E '[0-9]+\.[0-9]+' | tail -n 1)"
  fi
  printf '%s\n' "$gv_version"
}

# The names ADR 0049 gives a download. VERSION empty means the unversioned alias `latest` resolves.
artifact_name() {
  an_os="$1"
  an_arch="$2"
  an_manager="$3"
  an_flavour="$4"
  an_version="$5"
  case "$an_os" in
    macos)
      if [ "$an_flavour" = headless ]; then
        an_stem="mixengine"
        an_tail="macos-universal-headless.pkg"
      else
        an_stem="mixlab"
        an_tail="macos-universal.pkg"
      fi
      if [ -n "$an_version" ]; then
        echo "$an_stem-$an_version-$an_tail"
      else
        echo "$an_stem-$an_tail"
      fi
      ;;
    linux)
      if [ "$an_flavour" = headless ]; then
        an_package="mixengine-headless"
      else
        an_package="mixlab"
      fi
      case "$an_manager" in
        apt)
          case "$an_arch" in
            x86_64) an_deb_arch=amd64 ;;
            aarch64) an_deb_arch=arm64 ;;
            *) return 1 ;;
          esac
          if [ -n "$an_version" ]; then
            echo "${an_package}_${an_version}-1_${an_deb_arch}.deb"
          else
            echo "${an_package}_${an_deb_arch}.deb"
          fi
          ;;
        dnf | zypper)
          if [ -n "$an_version" ]; then
            echo "${an_package}-${an_version}-1.${an_arch}.rpm"
          else
            echo "${an_package}-${an_arch}.rpm"
          fi
          ;;
        *) return 1 ;;
      esac
      ;;
    *) return 1 ;;
  esac
}

release_base() {
  if [ -n "$1" ]; then
    echo "$MIXLAB_RELEASES/download/v$1"
  else
    echo "$MIXLAB_RELEASES/latest/download"
  fi
}

# Every name this script can choose for VERSION, for the release's own check (_release.yml).
print_names() {
  for pn_flavour in window headless; do
    artifact_name macos x86_64 "" "$pn_flavour" "$1"
    for pn_arch in x86_64 aarch64; do
      artifact_name linux "$pn_arch" apt "$pn_flavour" "$1"
      artifact_name linux "$pn_arch" dnf "$pn_flavour" "$1"
    done
  done
}

main() {
  :
}

[ "${MIXLAB_INSTALL_SOURCED:-}" = 1 ] || main "$@"
