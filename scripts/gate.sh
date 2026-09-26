#!/usr/bin/env bash
# What CI would refuse, asked here first — the checks this machine can answer for itself.
#
#   bash scripts/gate.sh
#
# **Run by `scripts/ask-ci.sh` before every push**, which is the reason it exists: a CI run that goes
# red on a formatting slip or a broken doc link costs twenty minutes and a second run, and every one
# of these answers in seconds here. Measured on a Windows machine after an edit to core, the CLI and
# the daemon: about a minute and a half, most of it clippy.
#
# **Not `cargo test`.** Several suites answer for the machine rather than for the code — a port
# somebody holds, a login entry a real install left — so a red test here is not the same claim as a
# red test on a runner. CI runs them; this does not pretend to.
#
# **This system only.** Clippy and rustdoc compile this operating system's code; what is behind
# another system's `cfg` is CI's to answer, or WSL's for Linux.
#
# Exit status: 0 when every check passed, 1 when one did not, naming it.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

failed=()

# One check: a name, then the command. Its output is shown only when it fails, so a green gate is a
# short list and a red one is the error itself.
check() {
  local name="$1"
  shift

  local started=$SECONDS
  local output
  if output="$("$@" 2>&1)"; then
    echo "ok      $name ($((SECONDS - started))s)"
  else
    echo "FAILED  $name ($((SECONDS - started))s)"
    printf '%s\n' "$output" | tail -40
    failed+=("$name")
  fi
}

check "rustfmt" cargo fmt --all --check
check "clippy" cargo clippy --workspace --all-targets --all-features -- -D warnings
check "rustdoc" env RUSTDOCFLAGS="-D warnings" \
  cargo doc --workspace --no-deps --document-private-items --all-features
check "the helper's version" bash packaging/helper-lock.sh --check
check "documentation links" node scripts/check-docs.mjs
# CI's `docs` job: the corpus builds a site, and the committed command reference is what `mix`
# generates. A new or changed flag on `mix` without `bash packaging/docs.sh --reference` is red there
# and green on the check above, which reads links and spec headers and never runs `mix`.
check "the command reference" bash packaging/docs.sh --check

if [ ${#failed[@]} -ne 0 ]; then
  echo "the gate is red: ${failed[*]}" >&2
  exit 1
fi

echo "the gate is green"
