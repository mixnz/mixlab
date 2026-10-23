# 0067. The shim reads the database itself, and links nothing that fetches

**Status**: Accepted.
**Date**: 2026-10-23

## Context

`mixengine-shim` is the one client that links `mixengine-core`. It resolves a version in its own
process, with no daemon to ask, so that `php -v` answers in two directories differently while
`mixengined` is stopped (`docs/features/runtime-versions.md`, and the bench in
`crates/mixengine-shim/tests/overhead.rs`). Linking core brings everything core links: `sqlx` with a
bundled SQLite, and `reqwest` with an HTTP and TLS stack. Phase 47's T209e asked for both to leave
the shim.

Measured on 2026-10-23 (the
[T209e design](../specs/2026-10-23-t209e-the-shim-compiles-only-what-it-runs-design.md) has the
numbers): the shim's normal dependency tree is 219 crates on Windows against `mix`'s 94. `reqwest`'s
own subtree is about 104 crates and `sqlx`'s about 109, and the two overlap.

The shim never touches the network. It does read the database: `Store::open_read_only`, then
`resolve::runtime`, `bin_commands::kind`, `services::client::claims` and `chosen`,
`packages::records` and `runtimes::program`. Moving a dozen functions into a smaller crate removes
`reqwest` and leaves `sqlx` exactly where it was.

## Decision

**The shim keeps reading the SQLite database through `sqlx`, and links no code that fetches.**

1. `mixengine-core` puts every fetch behind a feature, `network`, which carries `reqwest`. It is on
   by default, so `cargo test -p mixengine-core` tests everything, and the workspace declares core
   with `default-features = false` so that each dependent says what it takes. The daemon takes
   `network`, and the shim does not.
2. The release build compiles the shim in a cargo invocation of its own. Cargo unifies features
   across the packages one invocation builds, so a shim built beside `mixengined` would compile core
   with `network` and link against it.
3. `crates/mixengine-proto/tests/workspace_layering.rs` fails when `reqwest` reaches the shim's
   normal dependency tree.
4. Nothing new is written for the shim to read instead of the database. The state it resolves from
   is the state the daemon keeps, read where the daemon keeps it.

## Consequences

**Easy**:

- The shim is smaller and compiles faster, and a change to a fetch can no longer reach a binary that
  runs on every `php`, `node` and `python` a user types.
- The shim still answers from the database while the daemon is stopped, after a migration, and after
  a home is restored from a backup, with nothing to keep in step.

**Hard**:

- A function in core that fetches carries `#[cfg(feature = "network")]`, and so does anything in
  core that calls it. `cargo clippy -p mixengine-core --no-default-features`, in the gate and in
  CI's `lint` job, is the check that the line is drawn right.
- The shim's binary barely shrinks: the linker already drops the `reqwest` code it never calls. What
  goes is the hundred crates a build of the shim alone compiles, and the chance of a fetch reaching
  it later.
- The release build has one more cargo invocation, and compiles core twice: once with `network` and
  once without.

**Accepted**:

- `sqlx` and a bundled SQLite stay in the shim. The phase 47 milestone says so instead of asking for
  them to go.

## Alternatives considered

- **A file the daemon writes for the shim to read.** The shim would link neither `sqlx` nor
  `reqwest`. It lost because it is a second copy of the state: every change to runtimes, defaults,
  bin commands and service claims has to rewrite it, and one forgotten path makes `php` run the wrong
  version with no error anywhere. A migration on the daemon's start, a home restored from a backup,
  or a daemon killed between the database write and the file write each leave the two apart, and the
  shim has to work in exactly those moments.
- **A second, small crate that reads the database with `rusqlite`.** Fewer crates than `sqlx`, still
  a bundled SQLite. It lost because each query the shim runs would be written twice, and the copy
  would no longer be checked against the schema by `sqlx::query!`.
- **Moving the shim's functions into a new crate that core re-exports, as T209e first proposed.**
  It removes `reqwest` the same way the feature does, and moves a dozen functions and their tests to
  do it. It lost because the feature draws the same line with less code moved; it stays the fallback
  if the feature's `#[cfg]` spreads into shared types.
