#!/bin/sh
# One command installs MixLab on macOS and Linux: it picks the installer for this machine, checks
# its checksum, its signature and the name the signature vouches for, and runs it.
#
#   curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh
#   curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh -s -- --headless --version 0.0.13
#
# Roadmap task T197.
# Design: docs/specs/2026-10-05-t197-one-command-installs-mixlab-design.md
#
# Served from `master` through the handbook's site, not from a release, so it has to work with every
# release from MIXLAB_OLDEST on. Everything is a function and `main` is called on the last line: a
# download cut off halfway defines functions and runs nothing. POSIX sh, because `curl | sh` runs
# whatever `sh` is; there is no `local`, so each function prefixes its variables.

# Examples name 0.0.13, a fixed older release, never the current one: scripts/set-version.mjs refuses
# a bump while packaging/ types out the version it is moving from.
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

fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    die "neither curl nor wget is installed. install one of them, then run this again"
  fi
}

url_answers() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsIL -o /dev/null "$1"
  else
    wget -q --spider "$1"
  fi
}

file_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    die "neither sha256sum nor shasum is installed. install coreutils, then run this again"
  fi
}

# A broken or truncated download. Not a signature: anybody who could replace the file could
# replace this beside it.
check_sha256() {
  cs_expected="$(awk 'NR == 1 { print $1 }' "$2")"
  [ -n "$cs_expected" ] && [ "$(file_sha256 "$1")" = "$cs_expected" ]
}

# A minisign on PATH, or the pinned one for this run. Prints its path.
minisign_for() {
  if command -v minisign >/dev/null 2>&1; then
    command -v minisign
    return 0
  fi
  mf_work="$3"
  case "$1/$2" in
    linux/x86_64 | linux/aarch64)
      mf_archive="minisign-0.12-linux.tar.gz"
      mf_sum="$MINISIGN_LINUX_SHA256"
      mf_bin="minisign-linux/$2/minisign"
      ;;
    macos/aarch64)
      mf_archive="minisign-0.12-macos.zip"
      mf_sum="$MINISIGN_MACOS_SHA256"
      mf_bin="minisign"
      ;;
    macos/x86_64)
      die "this intel mac has no minisign, and none is published for it. run brew install minisign, then run this again"
      ;;
    *) die "no minisign for $1 $2. install minisign, then run this again" ;;
  esac
  say "minisign is not installed, so this run fetches minisign 0.12 and removes it afterwards"
  fetch "$MINISIGN_URL/$mf_archive" "$mf_work/$mf_archive"
  [ "$(file_sha256 "$mf_work/$mf_archive")" = "$mf_sum" ] ||
    die "the minisign download does not match its pinned checksum. run this again later"
  mkdir -p "$mf_work/minisign"
  case "$mf_archive" in
    *.tar.gz) tar -xzf "$mf_work/$mf_archive" -C "$mf_work/minisign" ;;
    *.zip) unzip -q -o "$mf_work/$mf_archive" -d "$mf_work/minisign" ;;
  esac || die "cannot unpack minisign. install minisign, then run this again"
  chmod +x "$mf_work/minisign/$mf_bin"
  echo "$mf_work/minisign/$mf_bin"
}

# The signature, then what it vouches for: packaging/sign.sh signs every artifact with the trusted
# comment `mixengine <version> <file>`. Prints the signed version.
check_signature() {
  sg_out="$("$1" -V -H -P "$2" -m "$3" 2>&1)" || {
    say "the signature does not match: $sg_out"
    return 1
  }
  sg_comment="$(printf '%s\n' "$sg_out" | sed -n 's/^Trusted comment: //p')"
  sg_product="$(printf '%s\n' "$sg_comment" | awk '{ print $1 }')"
  sg_version="$(printf '%s\n' "$sg_comment" | awk '{ print $2 }')"
  sg_file="$(printf '%s\n' "$sg_comment" | awk '{ print $3 }')"
  if [ "$sg_product" != mixengine ] || [ "$sg_file" != "$4" ]; then
    say "the signature vouches for '$sg_comment', not for $4"
    return 1
  fi
  if [ -n "$5" ] && [ "$sg_version" != "$5" ]; then
    say "the signature is for version $sg_version, not $5"
    return 1
  fi
  printf '%s\n' "$sg_version"
}

usage() {
  cat <<'USAGE'
Installs MixLab, or with --headless the command-line programs alone.

  curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh
  curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh -s -- --headless

Options:
  --headless         the four command-line programs, without the MixLab window
  --version X.Y.Z    that release instead of the newest
  --dry-run          say what would be downloaded and run, check the URLs, install nothing
  -h, --help         this text
USAGE
}

root_prefix() {
  if [ "$(id -u)" = 0 ]; then
    echo ""
  elif command -v sudo >/dev/null 2>&1; then
    echo sudo
  else
    return 1
  fi
}

install_command() {
  case "$1/$2" in
    macos/*) echo "installer -pkg $3 -target /" ;;
    linux/apt) echo "env DEBIAN_FRONTEND=noninteractive apt-get install -y $3" ;;
    linux/dnf) echo "dnf install -y $3" ;;
    linux/zypper) echo "zypper --non-interactive install --allow-unsigned-rpm $3" ;;
    *) return 1 ;;
  esac
}

# PREFIX OS MANAGER FILE: the install itself, with FILE kept one argument whatever TMPDIR holds.
# install_command above is the same command as text, for --dry-run to print.
install_package() {
  ip_prefix="$1"
  case "$2/$3" in
    macos/*) run_install "$ip_prefix" installer -pkg "$4" -target / ;;
    linux/apt) run_install "$ip_prefix" env DEBIAN_FRONTEND=noninteractive apt-get install -y "$4" ;;
    linux/dnf) run_install "$ip_prefix" dnf install -y "$4" ;;
    linux/zypper) run_install "$ip_prefix" zypper --non-interactive install --allow-unsigned-rpm "$4" ;;
    *) return 1 ;;
  esac
}

# PREFIX COMMAND… with stdin closed: under `curl | sh` stdin is the rest of this script, and sudo
# asks for its password on the terminal, not on stdin.
run_install() {
  ri_prefix="$1"
  shift
  say "running: ${ri_prefix:+$ri_prefix }$*"
  if [ -n "$ri_prefix" ]; then
    "$ri_prefix" "$@" </dev/null
  else
    "$@" </dev/null
  fi
}

main() {
  m_flavour="window"
  m_version=""
  m_dry=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --headless)
        m_flavour="headless"
        shift
        ;;
      --version)
        [ $# -ge 2 ] || die "--version needs a version, such as --version 0.0.13"
        m_version="$(normalize_version "$2")" || die "$2 is not a version. use one such as 0.0.13"
        shift 2
        ;;
      --dry-run)
        m_dry=1
        shift
        ;;
      --print-names)
        m_names_version=""
        if [ -n "${2:-}" ]; then
          m_names_version="$(normalize_version "$2")" || die "$2 is not a version. use one such as 0.0.13"
        fi
        print_names "$m_names_version"
        return 0
        ;;
      -h | --help)
        usage
        return 0
        ;;
      *) die "unknown option $1. run with --help to see the options" ;;
    esac
  done

  if [ -n "$m_version" ] && ! version_at_least "$m_version" "$MIXLAB_OLDEST"; then
    die "this script installs $MIXLAB_OLDEST or newer. download $m_version from $MIXLAB_RELEASES/tag/v$m_version"
  fi

  case "$(uname -s)" in
    Darwin) m_os="macos" ;;
    Linux) m_os="linux" ;;
    *) die "this script is for macOS and Linux. on windows run: irm https://mixnz.github.io/mixlab/install.ps1 | iex" ;;
  esac
  m_arch="$(machine_arch "$(uname -m)")" ||
    die "there is no build for $(uname -m). see From source at ${MIXLAB_HANDBOOK}"

  m_manager=""
  if [ "$m_os" = linux ]; then
    m_manager="$(linux_manager)" ||
      die "this linux has no apt-get, dnf or zypper, so no package fits it. see From source at ${MIXLAB_HANDBOOK}"
    m_glibc="$(glibc_version)"
    [ -n "$m_glibc" ] ||
      die "this linux has no glibc (musl?), which every build needs. see From source at ${MIXLAB_HANDBOOK}"
    version_at_least "$m_glibc" "$MIXLAB_GLIBC_HEADLESS" ||
      die "this linux has glibc $m_glibc; the programs need $MIXLAB_GLIBC_HEADLESS or newer"
    if [ "$m_flavour" = window ] && ! version_at_least "$m_glibc" "$MIXLAB_GLIBC_WINDOW"; then
      die "this linux has glibc $m_glibc; the window needs $MIXLAB_GLIBC_WINDOW. run again with --headless for the command-line programs"
    fi
    if [ "$m_flavour" = window ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
      say "no desktop here; MixLab's window needs one. --headless is the package for a server"
    fi
  fi

  m_name="$(artifact_name "$m_os" "$m_arch" "$m_manager" "$m_flavour" "$m_version")" ||
    die "no package for this machine. see From source at ${MIXLAB_HANDBOOK}"
  m_base="$(release_base "$m_version")"

  if [ "$m_dry" = 1 ]; then
    say "found $m_os $m_arch${m_manager:+ with $m_manager}"
    m_failed=0
    for m_url in "$m_base/$m_name" "$m_base/$m_name.sha256" "$m_base/$m_name.minisig"; do
      echo "$m_url"
      url_answers "$m_url" || {
        say "no answer from $m_url"
        m_failed=1
      }
    done
    if ! command -v minisign >/dev/null 2>&1; then
      case "$m_os/$m_arch" in
        linux/*) m_pinned="$MINISIGN_URL/minisign-0.12-linux.tar.gz" ;;
        macos/aarch64) m_pinned="$MINISIGN_URL/minisign-0.12-macos.zip" ;;
        *) m_pinned="" ;;
      esac
      if [ -n "$m_pinned" ]; then
        echo "$m_pinned"
        url_answers "$m_pinned" || {
          say "no answer from $m_pinned"
          m_failed=1
        }
      fi
    fi
    install_command "$m_os" "$m_manager" "<downloaded $m_name>"
    return "$m_failed"
  fi

  m_prefix="$(root_prefix)" ||
    die "installing needs root, and this account has no sudo. run this as root"

  m_work="$(mktemp -d)" || die "cannot make a temporary directory"
  trap 'rm -rf "$m_work"' EXIT
  trap 'exit 1' INT TERM

  say "downloading $m_name"
  for m_part in "$m_name" "$m_name.sha256" "$m_name.minisig"; do
    fetch "$m_base/$m_part" "$m_work/$m_part" || die "cannot download $m_base/$m_part. check the connection, then run this again"
  done
  check_sha256 "$m_work/$m_name" "$m_work/$m_name.sha256" ||
    die "$m_name does not match its checksum. run this again; if it keeps failing, the download is being changed on the way"
  m_minisign="$(minisign_for "$m_os" "$m_arch" "$m_work")" || exit 1
  m_signed="$(check_signature "$m_minisign" "$MIXLAB_PUBKEY" "$m_work/$m_name" "$m_name" "$m_version")" ||
    die "$m_name is not signed by MixLab. nothing was installed"
  say "$m_name is version $m_signed, signed by MixLab"

  if [ "$m_manager" = apt ]; then
    run_install "$m_prefix" env DEBIAN_FRONTEND=noninteractive apt-get update ||
      die "apt-get update failed. fix the package lists, then run this again"
  fi
  install_package "$m_prefix" "$m_os" "$m_manager" "$m_work/$m_name" ||
    die "the installer failed. its own message is above"

  if [ "$m_flavour" = headless ]; then
    say "installed. open a new terminal and run: mix status"
  else
    say "installed. open MixLab, or in a new terminal run: mix status"
  fi
}

[ "${MIXLAB_INSTALL_SOURCED:-}" = 1 ] || main "$@"
