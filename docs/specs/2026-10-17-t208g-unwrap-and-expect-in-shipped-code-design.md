---
status: approved
date: 2026-10-17
task: T208g
---

# `unwrap()` and `expect()` in the code MixLab ships (design)

`apps/desktop/src-tauri/src` holds 1,099 calls to `.unwrap()` or `.expect(` (measured on `master`
at `0c2e83ad`). The roadmap asks for the ones outside `#[cfg(test)]` to be counted. Each one either
becomes a typed error the window can show, or keeps an `expect` whose message says why it cannot
fail. The count is then written down.

Roadmap: [phase 46](../roadmap/phase-46-a-database-module-of-one-shape.md), task T208g.

## What the count found

**42 are in shipped code. The other 1,057 are in tests**: in `tests.rs` files, in inline
`#[cfg(test)]` modules, and in `tests/`. The count leaves those three out and reads every remaining
line. A spot check of the five files with the most calls (`sync/lend.rs`, `sync/store.rs`,
`secrets.rs`, `db/drivers/tools.rs`, `import.rs`) confirmed that each of them has its first
`.unwrap()` after its `#[cfg(test)]`.

| Kind | Sites | Where |
| --- | ---: | --- |
| `Mutex::lock()` then `.unwrap()` or `.expect("never poisoned")` | 33 | `db/commands/{mod,sql}.rs` (6), `rest/{commands,preview}.rs` (8), `terminal/{commands,local,state}.rs` (7), `updater/commands.rs` (7), `ssh/mod.rs` (2), `sync/saved.rs` (3) |
| `.unwrap()` that cannot fail | 3 | `rest/preview.rs` (2: building a response from a fixed status, two ASCII headers and a body), `db/drivers/mssql/script.rs` (1: `last_mut` on a list that starts with one element and never shrinks) |
| `.expect("…")` that already says why | 6 | `sync/crypto.rs` (4), `db/drivers/clickhouse/dump.rs` (1), `lib.rs` (1: the app failing to build at start-up) |

None of the 42 turns a failure the user could cause into a panic. No user input, file, server
answer or network reply reaches any of them.

**The window's release profile keeps `panic = "unwind"`** (`src-tauri/Cargo.toml`). So a poisoned
mutex is a real state, not a theoretical one. If one command panics while it holds, say,
`DbState::running_queries`, the lock is poisoned. From then on every `.lock().unwrap()` on it panics
too: every later script run and every cancel, in every tab, until MixLab is restarted.

**The root workspace already settled both questions**:
- `[workspace.lints.clippy] unwrap_used = "warn"` turns into an error under CI's `-D warnings`.
- The root `clippy.toml` allows `unwrap`, `expect` and `panic` in tests.
- The daemon takes a poisoned lock with `.unwrap_or_else(PoisonError::into_inner)` (`logging/mod.rs`, `extensions/config.rs`).

The window's workspace, a separate one, has none of this.

## D1 — a poisoned lock is taken, not panicked on

All 33 `lock()` sites become `.lock().unwrap_or_else(PoisonError::into_inner)`, as the daemon
writes them.

- **Why the data is still sound.** Every one of these mutexes guards a registry: running queries,
  transfers, HTTP clients, requests in flight, preview documents, terminal sessions, the SSH
  tunnel's refusal, the cached update feed, the saved sync account. Each critical section is a
  single `insert`, `remove`, `get` or `clone`. A panic cannot leave one half-done, so the value
  behind a poisoned lock is as good as any other.
- **Why not a typed error instead.** The roadmap names that option, and it would give the window a
  message where today it gets a panic. But the message would say something like "MixLab hit an
  internal error, restart it" about a lock that is perfectly usable. Recovering is the better
  answer.
- **No helper.** The daemon writes the call inline in both places it takes one, and 33 inline
  calls read the same everywhere. A `lock_or_recover` trait would be a new name for one method
  call.

`sync/saved.rs` loses its three `.expect("never poisoned")`. "Never poisoned" is the claim
`panic = "unwind"` makes false.

## D2 — the three `unwrap`s that cannot fail say why

- `rest/preview.rs`: the two `Response::builder()…body(…).unwrap()` become
  `.expect("a fixed status and ASCII headers always build")`. The CSP header is put together by
  `csp()` from constants (`default-src data:; …`), so no value of `external` or `scripts` can make
  it invalid.
- `db/drivers/mssql/script.rs`: `batches.last_mut().unwrap()` becomes
  `.expect("the fold starts with one batch and only ever adds more")`.

The six existing `expect`s keep their messages; each already says why. `lib.rs`'s is not a "cannot
fail": it is the app having no window to show. A panic there, before any state exists, is the
honest outcome, and its message says what failed.

## D3 — a lint keeps it that way

`src-tauri/Cargo.toml` gains

```toml
[lints.clippy]
unwrap_used = "warn"
```

so `cargo clippy --all-targets -- -D warnings`, already run by `scripts/gate.sh` and CI's desktop
job, refuses a new `.unwrap()` in shipped code. `expect` stays allowed: the rule
(`docs/standards/rust.md`) is that its message says why, and no lint can read a message. Tests keep
theirs through the root `clippy.toml`, which clippy finds by walking up from the crate. If it turns
out not to reach this crate, `src-tauri/clippy.toml` gets the same three lines. The plan checks
which.

## D4 — the count, written down

After D1–D3 the shipped code holds **0 `unwrap()` and 9 `expect()`**: crypto 4, ClickHouse 1,
`lib.rs` 1, preview 2, SQL Server 1. Phase 46's T208g line records the count and how it was
taken, so the next count can be compared with it.

## Checks

- `cargo clippy --locked --all-targets -- -D warnings` with the new lint: green.
- The count is the lint's count too. `cargo clippy --lib -- -W clippy::unwrap_used` on `master`
  already reports exactly 33 `unwrap()`s: the 30 lock sites that use `unwrap` and the 3 of D2. The
  other 9 of the 42 are `expect`s, which the lint does not look at.
- `cargo test --locked`: the same test count as before.
- One new test in `db/commands/mod.rs`'s tests: a `RunningQuery` dropped while its mutex is
  poisoned (poisoned on purpose by a thread that panics holding it) still removes its entry, and
  the registry can still be used afterwards. This is the case D1 is about.

## MixLab

This is MixLab's own backend, and it works the same with `mixengined` absent or present. No
command, payload or screen changes. The one change a user could notice is in a failure: after an
unrelated panic, the database, REST and terminal tabs keep working instead of failing on every
call until a restart. That is not a feature to announce, so nothing goes in `CHANGELOG.md`. `mix`
has no part in this.

## Not in scope

- `crates/`, the root workspace: it already has the lint and the poison handling.
- The `.expect` calls in tests.
- Catching panics per command, as the daemon's RPC layer does. Tauri already reports a panicking
  command as a failed `invoke` to the window. The cascade D1 removes is what turned one panic into
  many.

## Is there an ADR here? No

The rule is `docs/standards/rust.md`'s already, and the root workspace enforces it the same way.
This brings the window's workspace into line.
