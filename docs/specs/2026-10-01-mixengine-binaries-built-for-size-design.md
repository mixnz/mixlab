---
status: approved
date: 2026-10-01
---

# MixEngine's binaries are built for size

## The problem

MixLab's window went from 54.6 MB to 33.0 MB on Windows, and to roughly half on Linux and macOS,
through `opt-level = "s"` and a stripped symbol table — #242 and #243, and
[the window's symbol-table design](2026-10-01-the-window-ships-without-its-symbol-table-design.md).
The four binaries the root workspace ships beside it got neither.

Built locally on Windows x86_64 from `master` at 6aedf128, with the root's `[profile.release]` (thin
LTO, one codegen unit, `strip = "debuginfo"`), and again with only `opt-level = "s"` added to it —
no package overrides, so the second column is an upper bound on what D2 below saves. The local
build has no `crt-static`, so the absolute figures sit a little under CI's; the ratios are the point.

| Binary | Today | `opt-level = "s"` | Change |
| --- | --- | --- | --- |
| `mixengined` | 29.0 MB | 21.2 MB | −27% |
| `mix` | 8.6 MB | 6.2 MB | −28% |
| `mixengine-shim` | 5.3 MB | 4.0 MB | −25% |
| `mixengine-elevate` | 0.94 MB | 0.77 MB | −18% |
| `mixengine-trampoline` | 0.27 MB | 0.27 MB | already `"z"` (T185) |

On Linux and macOS each binary also carries its symbol table, which Windows keeps in a `.pdb`. From
the `binaries-*` artifacts of `release-exact` run 36841381812, measured as the bytes a full strip
removes (Linux) and as `LC_SYMTAB`'s entries and strings (macOS):

| Binary | Linux x86_64 | macOS arm64 |
| --- | --- | --- |
| `mixengined` | 5.5 MB of 32.5 MB (17%) | 4.6 MB of 26.1 MB (18%) |
| `mix` | 1.7 MB of 9.6 MB (18%) | 1.3 MB of 7.2 MB (18%) |
| `mixengine-shim` | 1.1 MB of 6.3 MB (17%) | 0.95 MB of 5.5 MB (17%) |
| `mixengine-elevate` | 0.29 MB of 1.3 MB (22%) | 0.27 MB of 1.2 MB (22%) |

Unlike the window's, their dyld export tries are small (0.33 MB for `mixengined`, under 0.1 MB for
the rest), so the window's `-no_exported_symbols` has nothing to remove here.

On Linux and macOS the commands in `<root>/bin` are hard links to the one shim
(`mixengine-core/src/shims.rs`, `link`), so the shim's size is paid once per home. On Windows each
command is a copy of the trampoline, which T185 already built for size.

## D1. `mix`, the shim and `mixengine-elevate` drop their symbols

Three package overrides in the root `Cargo.toml`, the same shape as the trampoline's:

```toml
[profile.release.package.mixengine-cli]
strip = "symbols"
[profile.release.package.mixengine-shim]
strip = "symbols"
[profile.release.package.mixengine-elevate]
strip = "symbols"
```

`strip` acts when a binary is linked, so an override on the package that owns the binary is
enough. That is not true of `opt-level`, which D2 has to set for the whole profile. The trampoline's
override shows it works: its Linux binary carries about 1 KB of symbols beside the others' megabytes.

Nothing in these three reads its symbols. None installs a panic hook or captures a backtrace; the
only `set_hook` and `Backtrace` in `crates/` are the daemon's. **`mixengined` keeps its symbols.**
`crates/mixengine-daemon/src/crash.rs` renders a backtrace into the crash report "copy diagnostics"
(T66) hands a person, and the frames are worthless without names.

What else D1 touches:

- **Windows.** The executables do not change: MSVC's linker already keeps symbols out of them, in a
  `.pdb` that nothing in `packaging/` or `.github/workflows/` keeps.
- **Their release test binaries.** A package override applies to every target of the package, so
  the `bench` job's `cargo test --release -p mixengine-cli` and `-p mixengine-shim` run stripped. A
  failing budget still prints its assertion and its figures; a panic's backtrace loses its names.
- **The helper's version.** `packaging/helper-lock.mjs` fingerprints the helper's sources and its
  dependency list, not the build profile, so `--check` stays green and `HELPER_VERSION` does not
  move. That is the intent of T182b's D1: the version tracks what the helper does, and a smaller
  build of the same code is not a reason to raise an elevation prompt on every machine. A helper
  already installed keeps its larger bytes until the next change that does bump it.

Expected on Linux and macOS: −17% to −22% for each of the three. Windows is unchanged.

## D2. The root profile is `opt-level = "s"`, and the CPU-bound crates stay at 3

`[profile.release]` gains `opt-level = "s"` for every binary, the daemon included. The other route,
`"s"` for the three small binaries and `3` for the daemon, needs a second profile and therefore a
second build of every shared dependency, roughly doubling the `binaries` job.

What these binaries do that is bound by CPU rather than by I/O or by the processes they wait on is
unpacking and hashing the distributions the daemon installs, which run to several hundred megabytes
(the reason `[profile.dev.package]` already lifts several of these crates in debug builds, T170c),
and the shim's version resolution, which reads the home's SQLite database. Each crate below is in
the normal-dependency closure of `mixengined` and of the shim (`cargo tree -e normal -i`), and stays
at full speed:

| Crate | Why |
| --- | --- |
| `miniz_oxide`, `adler2`, `crc32fast`, `flate2`, `zip` | inflating `.tar.gz` and `.zip` distributions |
| `ruzstd` | decoding `.tar.zst` distributions |
| `sha2` | checking every download against the hash pinned in the signed index |
| `libsqlite3-sys` | the shim's 15 ms budget (T29) gates a resolution that reads SQLite; `cc` builds it at the profile's level, which `"s"` would make `-Os` |

Left out on purpose:

- **`zstd`, `zstd-safe`, `zstd-sys`**, which `[profile.dev.package]` lifts: they reach the workspace
  only through `mixengine-testkit`, a dev-dependency, and no shipped binary links them.
- **`ring` and `aws-lc-sys`**, the TLS under the daemon's downloads: their bulk ciphers and hashes
  are hand-written assembly that the opt level does not reach. What is left is handshake code, run
  once per connection. Keeping both large libraries at `3` would give back part of what `"s"` saves
  on the daemon, for a speed-up nothing here would see.

The trampoline's own `opt-level = "z"` override is unchanged.

**Branch builds.** `.github/actions/release-profile` sets `CARGO_PROFILE_RELEASE_OPT_LEVEL=0` for
every non-tag ref, and a package override outranks it, so on a branch these eight crates compile at
3, as the window's overrides already do. All are small; `libsqlite3-sys` is the one with a C build
that takes noticeably longer.

## Verification, and when D2 is dropped

The `bench` job reads `[profile.release]` from `Cargo.toml`; `release-profile` is used only by
`_build.yml`. So a branch run measures D2 against the four budgets in
[docs/standards/testing.md](../standards/testing.md): shim overhead, idle footprint, cold path and
M3's warm start. `ci.yml` runs on `master` only when asked, so there is no recent `master` figure
to compare with: the `bench` group is dispatched **on `master` and on the branch together**, so both
sets of runners are measured in the same hour.

- **A red budget drops D2**, as does a figure more than 10% worse than `master`'s from that pair of
  runs. The exception is M3's warm start on ubuntu, which is bimodal: a figure there in the slow mode
  is run again on both refs before anything is concluded. Without D2, D1 ships alone.
- Sizes are read from the `binaries-*` artifacts of a `release-exact` run, `build` group, on all five
  targets, against the figures above.
- `bash scripts/gate.sh`, which includes `helper-lock.sh --check`, before the branch is pushed.
- A full `release-exact` run (`all`) before the branch merges.

## Changelog

One line under `### Changed` in `## Unreleased`, beside the window's: MixEngine's own programs are
smaller, with the figure the `release-exact` run measures.

## MixLab

No screen. This changes how four of MixEngine's executables are compiled and how large the download
that carries them is. Nothing they do changes, so nothing in the window that drives them does.
MixLab's Settings → Updates downloads a smaller payload, and that is the only place a person could
notice.
