#!/usr/bin/env bash
# `preflight`: the tag, the version, the key and the secrets agree, checked in seconds and
# before anything is built, so a release that cannot work fails at once. Reads `GITHUB_REF_NAME`
# and the two signing secrets, which the step passes in as `UPDATE_SECRET_KEY` and
# `UPDATE_PASSWORD`; nothing here prints either.
set -euo pipefail

source packaging/common.sh
version="$(mix_version)"
test "$GITHUB_REF_NAME" = "v$version" || {
  echo "the tag is $GITHUB_REF_NAME and the workspace version is $version" >&2
  exit 1
}

pinned="$(sed -n 's/^pub const PUBLIC_KEY: &str = "\(.*\)";$/\1/p' \
  crates/mixengine-core/src/updates.rs)"
committed="$(sed -n '2p' packaging/updates.pub | tr -d '\r')"
test -n "$pinned" || {
  echo "no PUBLIC_KEY in crates/mixengine-core/src/updates.rs" >&2
  exit 1
}
test "$pinned" = "$committed" || {
  echo "packaging/updates.pub is not the key this build pins" >&2
  exit 1
}

test -n "${UPDATE_SECRET_KEY:-}" || { echo "the UPDATE_SECRET_KEY secret is not set" >&2; exit 1; }
test -n "${UPDATE_PASSWORD:-}" || { echo "the UPDATE_PASSWORD secret is not set" >&2; exit 1; }

echo "v$version, to be signed by $pinned"
