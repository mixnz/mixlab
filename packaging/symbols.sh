#!/usr/bin/env bash
# Keep the daemon's symbols beside the download, and ship it without them — roadmap task T91a.
#
#   bash packaging/symbols.sh --built <cargo's mixengined> --staged <the copy to strip> \
#     --target <triple> --out <directory>
#
# Writes `<out>/mixengined-<version>-<target>.sym.tar.gz` from the binary cargo wrote, which still
# has its symbol table (`strip = "debuginfo"` in the root's release profile), then strips the staged
# copy in place. On Windows the symbols were never in the executable: the archive holds the linker's
# `.pdb` and the executable it belongs to, and nothing is stripped.
#
# **The archive's name is chosen to stay out of the update feed.** `feed.sh` picks its payloads by
# `mixlab-<version>-…` and its helpers by `mixengine-elevate-<version>-…`; a rename that started
# with either would put a symbol file in front of every updater. `build`'s `mixengine-<os>`
# artifact, `release`'s gather, `sign.sh` and `release-verify.sh` all take it as they take any
# other file in `dist`.
#
# **Why here and not `strip = "symbols"` in Cargo.** Cargo strips at link time, so the unstripped
# binary a symbol file is made from would never exist. See
# docs/specs/2026-10-01-a-crash-report-names-its-frames-after-the-fact-design.md, D4.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

built=""
staged=""
target=""
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --built) built="$2"; shift 2 ;;
    --staged) staged="$2"; shift 2 ;;
    --target) target="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *)
      echo "unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

for name in built staged target out; do
  if [ -z "${!name}" ]; then
    echo "--$name is required" >&2
    exit 64
  fi
done

test -f "$built" || {
  echo "no binary at $built" >&2
  exit 1
}

archive="$out/mixengined-$(mix_version)-$target.sym.tar.gz"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

# The identifier a symbol file and a report are matched by, as each platform's own tool prints it —
# the same spelling `mixengine_platform::crash_image` writes into a report.
build_id() {
  case "$target" in
    *-linux-*)
      LC_ALL=C readelf -n "$1" | sed -n 's/^ *Build ID: *\([0-9a-f]*\)$/\1/p' | head -1
      ;;
    *-apple-darwin)
      dwarfdump --uuid "$1" | sed -n 's/^UUID: \([0-9A-F-]*\) .*/\1/p' | head -1
      ;;
  esac
}

case "$target" in
  *-windows-*)
    pdb="${built%.exe}.pdb"
    test -f "$pdb" || {
      echo "no .pdb beside $built: the Windows symbols are the linker's .pdb, and" >&2
      echo "binaries-hand-on.sh carries it from the job that built it" >&2
      exit 1
    }
    # The executable too: its sections place the `.pdb`'s symbols, and its CodeView record is the
    # build identifier `scripts/symbolize.mjs` checks a report against. It is the very file that
    # ships, since nothing is stripped on this system.
    cp "$built" "$work/mixengined.exe"
    cp "$pdb" "$work/mixengined.pdb"
    tar -czf "$archive" -C "$work" mixengined.exe mixengined.pdb
    ;;

  *-linux-* | *-apple-darwin)
    cp "$built" "$work/mixengined.sym"

    case "$target" in
      *-linux-*) mix_require objcopy readelf ;;
      *) mix_require strip dwarfdump codesign ;;
    esac

    expected="$(build_id "$built")"
    test -n "$expected" || {
      echo "$built carries no build identifier, so no report could be matched to its symbols" >&2
      exit 1
    }

    case "$target" in
      *-linux-*) objcopy --strip-all "$staged" ;;
      # No flags: everything but the symbols the dynamic linker needs.
      *) strip "$staged" ;;
    esac

    # **Open what was just made**, on the rule every script here follows. A strip that took the
    # identifier with it would ship a binary no symbol file could ever be matched to.
    stripped="$(build_id "$staged")"
    if [ "$stripped" != "$expected" ]; then
      echo "the stripped $staged has build id '$stripped', and its symbols '$expected'" >&2
      exit 1
    fi

    # Apple's strip rewrites a binary the linker ad-hoc signed; an Apple-silicon Mac will not run one
    # whose signature no longer matches it.
    if [[ "$target" == *-apple-darwin ]]; then
      codesign --verify --verbose=2 "$staged"
    fi

    # And it still runs, where this machine can run it: a slice for another architecture is checked
    # by the leg that builds natively for it. `rustc` rather than `mix_host_target`, which answers
    # `MIX_TARGET` when CI sets it — the target being built, not the machine.
    if [ "$target" = "$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')" ]; then
      "$staged" --version >/dev/null || {
        echo "the stripped $staged does not run" >&2
        exit 1
      }
    fi

    tar -czf "$archive" -C "$work" mixengined.sym
    ;;

  *)
    echo "no symbol handling for $target" >&2
    exit 1
    ;;
esac

test -s "$archive" || {
  echo "the symbol archive was not written at $archive" >&2
  exit 1
}

echo "$archive"
