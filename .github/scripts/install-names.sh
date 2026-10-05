#!/usr/bin/env bash
# T197: every file the install scripts can choose for this version is in the release, with its
# .sha256 and .minisig. Run by _release.yml after signing, before a draft exists, which is the last
# point where a renamed artifact can be caught without breaking a published command.
#
#   bash .github/scripts/install-names.sh <version> <dist>
set -euo pipefail
version="$1"
dist="$2"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

names="$(
  sh "$root/packaging/install/install.sh" --print-names "$version"
  sh "$root/packaging/install/install.sh" --print-names
  pwsh -NoProfile -Command "& '$root/packaging/install/install.ps1' -PrintNames -Version '$version'"
  pwsh -NoProfile -Command "& '$root/packaging/install/install.ps1' -PrintNames"
)"
missing=0
while IFS= read -r name; do
  for file in "$name" "$name.sha256" "$name.minisig"; do
    if [ ! -f "$dist/$file" ]; then
      echo "the install scripts can choose $file, and this release has no such file" >&2
      missing=1
    fi
  done
done <<<"$names"
[ "$missing" = 0 ] && echo "every name the install scripts can choose is in $dist ($(wc -l <<<"$names" | tr -d " ") names)"
exit "$missing"
