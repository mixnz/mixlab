#!/usr/bin/env bash
# Exercise `packaging/feed.sh` on every CI run, against a fixture distribution directory.
#
# The script it tests writes the one document an installed MixEngine reads to find out that a newer
# one exists, and the only other thing that would ever run it is a release. So it is run here
# instead, and the properties that matter are asserted: the hash and the size describe the file
# beside them, `provides` is read out of the archive rather than assumed, and a universal macOS file
# produces two rows — one per architecture — pointing at the same URL.
#
# **The one payload is the Windows zip**; macOS and Linux are updated by their
# installers, which the feed lists under `installers`, two flavours each.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The helper's version is the repository's own — `feed.sh` looks the helper assets up by it.
helper_version="$(sed -n 's/^pub const HELPER_VERSION: &str = "\(.*\)";$/\1/p' \
  "$root/crates/mixengine-proto/src/privileged.rs" | head -1)"
test -n "$helper_version" || {
  echo "no HELPER_VERSION in mixengine-proto" >&2
  exit 1
}

version="9.9.9"
dist="$work/dist"
mkdir -p "$dist"

windows="mixlab-$version-windows-x86_64.zip"
python3 - "$dist/$windows" <<'PY'
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1], "w") as archive:
    for name in ["mix", "mixengined", "mixengine-elevate"]:
        archive.writestr(f"mixengine/{name}.exe", f"not really {name}\n")
PY

# One with a `.sha256` beside it and one without, because both happen: every packaging script writes
# one, and a hand-assembled directory may not.
(cd "$dist" && sha256sum "$windows" >"$windows.sha256")

# The installers. Both flavours on both systems, and the unversioned Linux aliases the handbook
# links, which must not become rows of their own.
pkg="mixlab-$version-macos-universal.pkg"
pkg_headless="mixengine-$version-macos-universal-headless.pkg"
deb="mixlab_$version-1_amd64.deb"
deb_headless="mixengine-headless_$version-1_amd64.deb"
rpm="mixlab-$version-1.x86_64.rpm"
rpm_headless="mixengine-headless-$version-1.x86_64.rpm"
for name in "$pkg" "$pkg_headless" "$deb" "$deb_headless" "$rpm" "$rpm_headless" \
  "mixlab_amd64.deb" "mixlab-x86_64.rpm"; do
  echo "not really an installer: $name" >"$dist/$name"
done

# The privileged helper of each leg, published as its own asset and named by the helper's own
# version. `feed.sh` refuses a distribution with none.
helper_linux="mixengine-elevate-$helper_version-linux-x86_64"
helper_macos="mixengine-elevate-$helper_version-macos-universal"
helper_windows="mixengine-elevate-$helper_version-windows-x86_64.exe"
echo "not really a privileged helper" >"$dist/$helper_linux"
echo "not really a privileged helper either" >"$dist/$helper_macos"
echo "nor this one" >"$dist/$helper_windows"

bash "$root/packaging/feed.sh" --dist "$dist" --version "$version" --tag "v$version" \
  --repo "example/mixengine"

test -f "$dist/latest.json" || {
  echo "feed.sh wrote no latest.json" >&2
  exit 1
}

python3 - "$dist/latest.json" "$dist" "$version" "$helper_version" <<'PY'
import hashlib
import json
import os
import re
import sys

feed_path, dist, version, helper_version = sys.argv[1:5]

with open(feed_path, encoding="utf-8") as handle:
    feed = json.load(handle)


def digest(name):
    with open(os.path.join(dist, name), "rb") as handle:
        return hashlib.sha256(handle.read()).hexdigest()


def size(name):
    return os.path.getsize(os.path.join(dist, name))


assert feed["schema"] == 1, feed["schema"]
assert feed["version"] == version, feed["version"]
assert re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", feed["generated_at"]), feed[
    "generated_at"
]
assert re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", feed["published_at"]), feed[
    "published_at"
]
assert feed["notes"].strip(), "the feed carries no notes at all"
assert feed["notes_url"].endswith(f"/v{version}"), feed["notes_url"]

# One payload, the Windows zip, and `provides` read out of it with the `.exe` off the key.
assert len(feed["artifacts"]) == 1, feed["artifacts"]
row = feed["artifacts"][0]
windows = f"mixlab-{version}-windows-x86_64.zip"
assert (row["os"], row["arch"]) == ("windows", "x86_64"), row
assert row["sha256"] == digest(windows), row
assert row["size"] == size(windows), row
assert row["url"] == f"https://github.com/example/mixengine/releases/download/v{version}/{windows}"
assert row["provides"] == {
    "mix": "mixengine/mix.exe",
    "mixengined": "mixengine/mixengined.exe",
    "mixengine-elevate": "mixengine/mixengine-elevate.exe",
}, row["provides"]

# Four helper rows out of three files: macOS publishes one universal helper and is listed under
# both architectures. Each row carries the helper's own version, and no `sha256` — the helper is
# checked inside the elevated process against a detached signature.
helpers = {(row["os"], row["arch"]): row for row in feed["helpers"]}
assert set(helpers) == {
    ("linux", "x86_64"),
    ("windows", "x86_64"),
    ("macos", "x86_64"),
    ("macos", "aarch64"),
}, helpers.keys()
assert helpers[("macos", "x86_64")]["url"] == helpers[("macos", "aarch64")]["url"]
for row in feed["helpers"]:
    assert row["version"] == helper_version, row
    assert "sha256" not in row, row
    name = row["url"].rsplit("/", 1)[1]
    assert row["size"] == size(name), row

# One row per (os, arch, kind, flavour), bound by its SHA-256, and never an
# unversioned alias.
installers = {
    (row["os"], row["arch"], row["kind"], row["flavour"]): row for row in feed["installers"]
}
expected = {
    ("macos", "x86_64", "pkg", "window"): f"mixlab-{version}-macos-universal.pkg",
    ("macos", "aarch64", "pkg", "window"): f"mixlab-{version}-macos-universal.pkg",
    ("macos", "x86_64", "pkg", "headless"): f"mixengine-{version}-macos-universal-headless.pkg",
    ("macos", "aarch64", "pkg", "headless"): f"mixengine-{version}-macos-universal-headless.pkg",
    ("linux", "x86_64", "deb", "window"): f"mixlab_{version}-1_amd64.deb",
    ("linux", "x86_64", "deb", "headless"): f"mixengine-headless_{version}-1_amd64.deb",
    ("linux", "x86_64", "rpm", "window"): f"mixlab-{version}-1.x86_64.rpm",
    ("linux", "x86_64", "rpm", "headless"): f"mixengine-headless-{version}-1.x86_64.rpm",
}
assert len(feed["installers"]) == len(expected), feed["installers"]
assert set(installers) == set(expected), sorted(installers)
for key, name in expected.items():
    row = installers[key]
    assert row["url"].endswith("/" + name), (key, row["url"])
    assert row["sha256"] == digest(name), (key, row)
    assert row["size"] == size(name), (key, row)

print(
    f"latest.json describes {len(feed['artifacts'])} payload, {len(feed['helpers'])} helper rows "
    f"and {len(feed['installers'])} installer rows"
)
PY

# **An empty directory is a failure and not an empty feed.** A release whose feed lists nothing is
# one every installed copy would read and act on by doing nothing, for ever.
empty="$work/empty"
mkdir -p "$empty"
if bash "$root/packaging/feed.sh" --dist "$empty" --version "$version" --tag "v$version" \
  --repo "example/mixengine" 2>/dev/null; then
  echo "feed.sh wrote a feed for a directory with no payloads in it" >&2
  exit 1
fi

# **And a distribution with payloads but no privileged helper is a failure too.** That is a release
# whose helper no machine could ever be offered, which is the state the helper's update path exists
# to make impossible — and it is the shape a leg that forgot `mix_publish_helper` would produce.
helperless="$work/helperless"
mkdir -p "$helperless"
cp "$dist/$windows" "$helperless/"
if bash "$root/packaging/feed.sh" --dist "$helperless" --version "$version" --tag "v$version" \
  --repo "example/mixengine" 2>/dev/null; then
  echo "feed.sh wrote a feed for a directory with no privileged helper in it" >&2
  exit 1
fi

# **And a helper for macOS or Linux with no installer beside it is a failure.**
# Every such machine would be offered nothing by that release, and nothing else would notice.
for system in macos linux; do
  installerless="$work/installerless-$system"
  mkdir -p "$installerless"
  cp "$dist/$windows" "$dist/$helper_windows" "$installerless/"
  case "$system" in
    macos) cp "$dist/$helper_macos" "$installerless/" ;;
    linux) cp "$dist/$helper_linux" "$installerless/" ;;
  esac
  if bash "$root/packaging/feed.sh" --dist "$installerless" --version "$version" \
    --tag "v$version" --repo "example/mixengine" 2>/dev/null; then
    echo "feed.sh wrote a feed for a $system helper with no installer beside it" >&2
    exit 1
  fi
done

echo "packaging/feed.sh writes what a release needs, and refuses what it cannot describe"
