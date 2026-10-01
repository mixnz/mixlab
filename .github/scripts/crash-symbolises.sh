#!/usr/bin/env bash
# A stripped release daemon's crash report turns back into names — roadmap task T91a.
#
# `bench`'s last step on every system. It builds `mixengined` the way a release does — the release
# profile and `mix_release_rustflags` — plus the `crash-probe` feature, strips it with
# `packaging/symbols.sh`, has it panic, and runs `scripts/symbolize.mjs` over the report with the
# symbol archive from this run. The binary under test is the shipped one plus one feature, which is
# named in docs/specs/2026-10-01-a-crash-report-names-its-frames-after-the-fact-design.md.
set -euo pipefail

source packaging/common.sh

target="$(rustc -vV | sed -n 's/^host: //p')"
suffix="$(mix_exe_suffix)"

# **The probe is not in what ships.** The daemon this job built for its measurements is built the
# way `stage.sh` builds one, with no features; the probe's variable must not be in it.
if grep -a -q MIXENGINE_CRASH_PROBE "target/release/mixengined$suffix"; then
  echo "::error title=Crash probe shipped::target/release/mixengined$suffix contains the crash probe, and a build without the crash-probe feature must not"
  exit 1
fi

# With release flags (Windows), `--target`: they stay off build scripts, as `stage.sh` keeps them
# off, and the build lands in `target/<triple>` beside the measured daemon. Without any (Linux,
# macOS), the same directory as the measurements, so only the daemon's own crate compiles again.
flags="$(mix_release_rustflags "$target")"
if [ -n "$flags" ]; then
  RUSTFLAGS="$flags" cargo build --release -p mixengine-daemon --features crash-probe \
    --target "$target" --locked --offline
  built="target/$target/release/mixengined$suffix"
else
  cargo build --release -p mixengine-daemon --features crash-probe --locked --offline
  built="target/release/mixengined$suffix"
fi

work="$RUNNER_TEMP/crash-probe"
rm -rf "$work"
mkdir -p "$work/symbols"
cp "$built" "$work/mixengined$suffix"

bash packaging/symbols.sh --built "$built" --staged "$work/mixengined$suffix" \
  --target "$target" --out "$work/symbols"

set +e
MIXENGINE_CRASH_PROBE=1 "$work/mixengined$suffix" --home "$work/home"
status=$?
set -e
if [ "$status" -eq 0 ]; then
  echo "::error title=Crash probe did not panic::the probe daemon exited 0 with MIXENGINE_CRASH_PROBE set"
  exit 1
fi

shopt -s nullglob
reports=("$work"/home/logs/crashes/crash-*.json)
if [ ${#reports[@]} -ne 1 ]; then
  echo "::error title=No crash report::expected one report in $work/home/logs/crashes, found ${#reports[@]}"
  ls -la "$work/home/logs" "$work/home/logs/crashes" 2>/dev/null || true
  exit 1
fi
cat "${reports[0]}"

archives=("$work"/symbols/*.sym.tar.gz)
named="$(node scripts/symbolize.mjs "${reports[0]}" --symbols "${archives[0]}")"
echo "$named"

if ! grep -q 'mixengined::crash::probe::raise' <<<"$named"; then
  echo "::error title=Crash report did not symbolise::no frame named mixengined::crash::probe::raise"
  exit 1
fi
echo "the stripped daemon's report names its frames, the probe's among them"
