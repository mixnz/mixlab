---
status: implemented
date: 2026-10-23
task: T209e
---

# The shim compiles only what it runs (design)

`mixengine-shim` runs on every `php`, `node` and `python` a user types, and it links all of
`mixengine-core`: the bundled SQLite through `sqlx`, which it uses, and an HTTP and TLS stack through
`reqwest`, which it never calls. This design takes `reqwest` out of the shim and keeps `sqlx`, for
the reasons in [ADR 0067](../decisions/0067-the-shim-reads-the-database-and-fetches-nothing.md).

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209e.

## What was read

Measured on `master` at `9e346bea`, on Windows 11 (x86_64).

**What the shim calls in core.** `Store::open_read_only`, `resolve::runtime`,
`resolve::restore_command`, `bin_commands::kind`, `services::client::{claims, executable_for,
chosen, override_env}`, `packages::records`, `runtimes::program`, `runtimes::java::home`,
`generate::Catalogue::builtin`, `generate::ca::path`, `certs::ca::certificate_path`, `shims`,
`paths`, `config::PathOverrides`, and the `Error`/`RuntimeError` variants `hint_for` matches. Its
tests add `packages::remember` and `runtimes::extensions::conf_d`. Six of these read the database,
so the shim cannot drop `sqlx` without reading the state some other way. The ADR compares the other
ways.

**Where `reqwest` is used in core.** Nine files: `index.rs` (`default_transport`, the index
`Client`, the fetch), `index/packages.rs` and `index/packages/kinds.rs`, `install.rs` (`Installer`
and its download), `prerequisites.rs`, `updates/helper.rs`, `blueprints/archive.rs` and
`extensions/registry.rs`. No error type holds a `reqwest::Error`: each is boxed into a
`Box<dyn Error>` source. `install.rs` and `index.rs` also hold what is not a fetch and is used
widely — `SmokeTest`, `archive::safe`, `present`, `check_runs`, `Artifact`, `Extensions` — so the
line runs through those modules rather than around them.

**How the release is built.** `packaging/stage.sh` builds every crate in `MIX_CRATES` with one
`cargo build --release --locked -p … -p …`, in all three of its branches (host, `--target`,
container), and CI's `build` job reaches it through `stage.sh --build-only`. Cargo unifies features
across one invocation, so today a feature turned off for the shim would be turned back on by the
daemon built beside it.

`crates/mixengine-platform/Cargo.toml` says the release pipeline gives `mixengine-elevate` an
invocation of its own for that reason. `stage.sh` does not. That is recorded as a follow-up below,
not fixed here: the helper's bytes are pinned by `packaging/helper-lock.sh`, and changing how it is
built is a change of its own.

**Baseline.**

| | before | after |
| --- | --- | --- |
| `cargo tree -p mixengine-shim -e normal`, distinct crates | 219 | 189 |
| the same for `mix` (`mixengine-cli`), for reference | 94 | 94 |
| `mixengine-shim.exe`, debug | 12,552,192 B | 10,374,656 B |
| `mixengine-shim.exe`, release, built alone | 5,274,624 B | 5,274,112 B |
| `reqwest` symbols in that build's `mixengine_shim.pdb` | 4 | 0 |
| `overhead.rs`, resolution, p50, nothing pins it / a manifest pins it | 3.04 / 1.48 ms | 3.31–5.73 / 1.42–1.61 ms over four runs |
| `overhead.rs`, shim minus program, p50, the same two cases | 39.19 / 35.96 ms | 38.31–40.79 / 41.22–45.31 ms |

The row "built as `stage.sh` builds it" was dropped from the table: `stage.sh` sets
`MIXENGINE_RELEASE` and the release `RUSTFLAGS` (a static C runtime), so its shim is not comparable
with one built alone. What it was checked for instead is that it links no `reqwest`: after the change
its `mixengine_shim.pdb` names `reqwest` 0 times, against 591 in the `mixengined.pdb` built beside
it.

**What the numbers say.** The 30 crates that left are the HTTP and TLS stack: `reqwest`, `hyper`,
`h2`, `http`, the `tower` crates, `rustls`, `rustls-webpki`, `tokio-rustls`, and `aws-lc-rs` with
`aws-lc-sys`, the C library whose build is the slowest step of a clean shim build. The rest of
`reqwest`'s subtree (`tokio`, `ring`, …) is shared with what the shim keeps. The release binary is the same size within 512 bytes, because the linker already dropped the
`reqwest` code nothing called. The debug binary is 17% smaller. The resolution and the end-to-end
time are unchanged within the run-to-run spread of this machine. The gain is the guarantee and the
build, not the binary.

The resolution is the number `overhead.rs` gates, against 15 ms. The end-to-end difference is
measured through the trampoline, the shim and the program, three process creations on Windows, and
is gated on nothing (the file's own header says why). Both were taken on a developer machine, not a
CI runner, so they compare with the "after" column and with nothing else.

The "after" column is filled by the implementation, on the same machine, and the numbers are copied
into the roadmap entry when it is ticked. Release sizes are what the linker kept: code it can prove
unreachable is dropped already, so the size may move less than the crate count does. The crate
count and the compile time are where the gain is expected.

## Decisions

**D1. Core gets a `network` feature, on by default, that carries `reqwest`.** `reqwest` becomes
`optional = true` and `network = ["dep:reqwest"]`. Every item that names `reqwest` gets
`#[cfg(feature = "network")]`, and so does every item in core that calls one of those: the index
`Client` and `default_transport`, `Installer`, the fetch in `prerequisites`, `updates::helper`,
`blueprints::archive`'s download, `extensions::registry`, and their callers such as
`extensions::install` and `updates::apply`. A module that is wholly a fetch is gated at its `mod`
line, not item by item. What is gated is found by the compiler:
`cargo check -p mixengine-core --no-default-features --all-targets` must be clean, warnings included,
so nothing is left dead behind the line.

On by default, so that `cargo test -p mixengine-core` and `cargo check --workspace` keep compiling
and testing every fetch, as they do now.

**D2. Each dependent of core says what it takes.** The workspace entry becomes
`mixengine-core = { path = "crates/mixengine-core", default-features = false }`, which is how
`mixengine-platform` is declared beside it. `mixengine-daemon` asks for `features = ["network"]`, and
`mixengine-shim` asks for nothing. Cargo ignores a member's `default-features = false` when the
workspace entry leaves defaults on, so the switch belongs in the workspace entry and nowhere else.

**D3. `stage.sh` builds the shim on its own.** In each of its three branches, the shim gets a
`cargo build --release --locked -p mixengine-shim` of its own after the build of the rest, with the
same target, flags and `MIXENGINE_RELEASE`. The list of the rest leaves the shim out. Both builds
share `target/`, so the second compiles only core without `network` and the shim. `MIX_PREBUILT`
and the stage checks are unchanged: they look for files, and the file is in the same place.

**D4. The layering test fails when `reqwest` reaches the shim.** A test in
`crates/mixengine-proto/tests/workspace_layering.rs` runs
`cargo tree -p mixengine-shim -e normal --prefix none --format {p}` (cargo from the `CARGO`
variable that `cargo test` sets, `--locked --offline`), and fails if a line names `reqwest` or
`hyper`, with a message that names this ADR. `cargo metadata` is not enough: its resolve graph is the
workspace's, with features unified, so it would show `reqwest` under the shim whatever the shim asked
for. `sqlx` is not on the list: the shim keeps it on purpose.

The test says what the shim's tree is, not whether core still compiles without `network`. Every
workspace build turns `network` on, so a new `reqwest` call left ungated would compile everywhere
until `stage.sh` built the shim on its own, at a release. So `scripts/gate.sh` and CI's `lint` job
gain two checks beside the helper's: `cargo clippy -p mixengine-core --no-default-features -- -D
warnings`, where an item only `network` reaches shows as dead, and `cargo clippy -p mixengine-shim
--all-targets -- -D warnings`, which builds the shim and its tests against that core.

**D5. No behaviour changes.** The shim, the daemon and `mix` do what they did. The shim's and
core's test counts are the same before and after, name for name, and `overhead.rs`'s resolution stays
inside its budget.

## Documents changed with the code

- `docs/roadmap/phase-47-code-that-is-safe-to-change.md`: T209e's text and **M47**'s
  `cargo tree -p mixengine-shim -e normal` sentence say `reqwest`, not `sqlx` and `reqwest`, and
  link ADR 0067. A follow-up after T209e: the release builds `mixengine-elevate` in one invocation
  with everything else, which `mixengine-platform/Cargo.toml` says it does not.
- `crates/mixengine-shim/Cargo.toml`, `crates/mixengine-core/Cargo.toml`: the comments on the
  dependency and the new `[features]`.
- `docs/architecture/overview.md`, the `mixengine-shim` line: core without its fetches.
- `docs/decisions/README.md`: the index row for 0067.

## MixLab

The window has no part in this. MixLab reaches no further into this workspace than
`mixengine-proto` and `mixengine-platform`, and starts no shim; what changes is which code one
headless binary links.

## Left to the builder

- The exact list of gated items. D1 names where to start, and the compiler names the rest.
- Whether a gated group reads better as a `network` submodule of its module than as items with
  attributes. If the `#[cfg]` would have to reach into a type or an error enum shared with code the
  shim runs, stop and come back: that is the point at which the third alternative in the ADR, a
  crate of its own, is the better line.
