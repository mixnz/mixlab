# Rust standards

## Toolchain

- Edition 2024. The compiler is pinned in `rust-toolchain.toml` and bumped deliberately. When it
  moves, `rust-version` in the workspace manifest moves with it. Read the version off those two
  files, not off this page.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` gate every commit.
- So does rustdoc: `cargo doc --workspace --no-deps --document-private-items`. Intra-doc links are
  resolved by nothing else, and `rustdoc::all` is denied in `[workspace.lints]`, so this fails
  locally exactly as it does in CI. A link inside an OS-specific module is only resolved when
  documenting *that* OS, and `--target` no longer substitutes for being on it: cargo runs the build
  scripts of the dependencies, and `libsqlite3-sys` compiles SQLite for the target it is given. CI
  therefore documents on each of the three runners, and a change to `mixengine-platform`'s Windows
  or macOS half is checked there rather than from your machine.
- Workspace-level `[workspace.dependencies]`; member crates use `dep.workspace = true`. One version
  of `tokio`, `serde`, `sqlx`, `tracing` across the tree.

## Core dependencies (decided, do not swap casually)

| Concern | Crate |
| --- | --- |
| async runtime | `tokio` (multi-thread) |
| HTTP server/client | `hyper` + `hyper-util`, `reqwest` (rustls, OS verifier) for downloads — [ADR 0063](../decisions/0063-outbound-tls-trusts-the-operating-system.md) |
| index signatures | `minisign-verify` in the product, `minisign` in the testkit and nowhere else |
| artifact hashing | `sha2` — pinned to the 0.10 `sqlx` already brings, not the newer 0.11 |
| archives | `zip` (deflate only), `tar`, `flate2`; zstd is `ruzstd` in the product and `zstd` in the testkit, for `minisign`'s reason |
| serialisation | `serde`, `serde_json`, `toml` |
| DB | `sqlx` (SQLite, compile-time checked) |
| CLI | `clap` (derive) |
| logging | `tracing` + `tracing-subscriber` |
| errors | `thiserror` in libraries, `anyhow` in binary crates only — see *Error handling* |
| templates | `minijinja` |
| certs | `rcgen`, `rustls`, `x509-parser` |
| DNS | `hickory-server` + `hickory-proto`. **Not `hickory-resolver`**: the daemon answers for the TLDs it manages and refuses everything else rather than forwarding, so there is no stub resolver and no cache in this workspace |
| mDNS | `mdns-sd` |
| process/system info | `sysinfo` |
| keyring | `keyring`, plus a direct edge onto its Linux backend `dbus-secret-service` — the one exception to this table, [ADR 0013](../decisions/0013-reading-the-d-bus-error-name-to-tell-an-absent-store.md) |
| paths | `directories` |
| Windows APIs | `windows` (official crate), never `winapi` |
| TS bindings | `ts-rs` |
| Markdown rendering | `pulldown-cmark`, a **dev**-dependency of `mixengine-docs` alone, so no renderer is linked into `mix` — [ADR 0021](../decisions/0021-the-handbook-is-one-corpus-published-three-ways.md) |

### A second copy of a crate is a decision, and the build asks for it in writing

`multiple-versions` in [../../deny.toml](../../deny.toml) is `deny`, not `warn`. A new dependency
that drags a second copy of something already in the tree fails the `lint` job, and the only way
past is a `skip` (or `skip-tree`, when the duplicate root brings a subtree with it) entry naming the
edge that causes it. The list there is the whole of what this workspace accepts today; every entry
on it is a transitive edge whose owner has not moved, and none can be removed by choosing
differently in our own manifests.

Do not reach for the skip list first. A duplicate is usually telling you that a version was picked
without looking — the `sha2` row above is what looking does, pinning our own edge to the 0.10 line
`sqlx` brings rather than taking 0.11 and doubling it. Add the entry only when the second copy comes
from somebody else's manifest, and write the reason as the edge, not as "upstream".

## Error handling

- Every library crate defines its own `Error` enum with `thiserror`. **No `anyhow::Result` in
  library signatures.** A binary crate (`mixengined`) may use `anyhow` inside itself. What reaches
  a client is converted to the typed wire error at the boundary, never sent as an `anyhow` chain.
- That enum may wrap one enum per subject, each in the module of its subject, through a
  `#[error(transparent)] Subject(#[from] SubjectError)` variant. `mixengine-core` does this: its
  `Error` holds what every subject shares, and `GraphError`, `JobError`, `ExtensionError` and
  the rest hold the others. The daemon classifies each subject's errors in a `match` with no
  wildcard, so a variant added to a subject does not compile until it has a wire code. The outer
  `match` on `mixengine_core::Error` does need a `_ => internal` arm, because that enum is
  `#[non_exhaustive]`. So a **new subject** compiles without a classification and reaches clients
  as `internal`: adding one means adding its arm in `error.rs` in the same change.
- `mixengine-proto::Error` is the wire error; conversions live at the daemon boundary and always set
  a stable `code` and a useful `hint`.
- `unwrap()`/`expect()` are allowed only where a panic is genuinely impossible and the message says
  why (`expect("template compiled at build time")`). A panic in the daemon kills every managed
  service, so treat it accordingly: the RPC layer catches panics per request and returns `internal`.
- Never swallow an error to keep going. If a degraded path is intended, log at `warn!` with context
  and reflect it in the returned state (`Degraded`, not `Running`).

## Async

- Blocking work (file extraction, hashing, `sqlx` on a large query, any shell-out that can hang) goes
  through `spawn_blocking` or a dedicated task; nothing blocks the runtime.
- **No detached tasks that outlive shutdown.** A task that runs past the call that started it is
  held by something shutdown waits for (a `TaskTracker`, a `JoinSet`, a kept `JoinHandle`), and
  stops on a `CancellationToken` derived from the daemon's root token. A task that writes state is
  never dropped unjoined. `tokio::task::Builder` is not how a task is named here: it needs
  `--cfg tokio_unstable`, which this workspace does not set. Existing `tokio::spawn` calls whose
  handle nobody keeps are listed and fixed by T209h.
- Timeouts on everything that touches the network, a child process, or a socket. A missing timeout is
  a review blocker.

## Structure & style

### Files and modules

- Modules by capability, not by layer: `sites/`, `runtimes/`, `certs/` — not `models/`, `utils/`.
  `utils` and `helpers` are banned module names.
- A file stays under the size limit in [source-files.md](source-files.md); one that would cross it
  splits along these lines.

### Types and state

- Newtypes for identifiers (`SiteId`, `ServiceId`, `RuntimeKind`); no bare `String` IDs crossing a
  function boundary.
- Constructors take injected dependencies (`Arc<dyn Host>`, `Arc<Store>`); no global singletons, no
  reading env vars deep inside a function — configuration enters at `main`.
- **A `static` that holds state is the exception, and says why.** It is allowed only for something
  there is one of per process by nature: the log sink, a lock that serialises writes to one file,
  state the OS keeps per process. Its doc comment says why it cannot be passed in. Anything a test
  would want two of is injected instead.

### Control flow

- **A function that decides what state comes next is a list of named steps.** Starting,
  supervising, stopping, repairing and upgrading are examples. The states are an explicit `enum`.
  Each step is a function that takes one state and returns the next. What a step decides (retry or
  give up, which reason to record) is a pure function, tested without spawning a process. The walk
  that is left reads top to bottom. A flat table with one arm per method or per error is not such
  a function. `api/rpc/table.rs` and `error.rs`'s `to_wire` are long, but nothing in them depends
  on an earlier arm, and they stay tables. The seven walks that predate this rule are T209.
- **A list of steps that always all run, where no step reads another's result, needs no state
  `enum`.** A daemon start's repairs are the example. Each step is a named function that logs its
  own failure, and the list is the calls in order. See
  [the T209 design](../specs/2026-10-20-t209-the-daemons-walks-as-named-steps-design.md), D2.

### Comments

- Public items in `core`, `proto`, `platform` carry doc comments explaining *why*, with `# Errors`
  and `# Panics` sections where applicable. Review checks this; no lint does, because
  `missing_docs` is not turned on.
- **A comment earns its place by carrying what the code cannot**: an alternative that was tried and
  rejected, a constraint found by experiment, a hazard in somebody else's crate. One that restates
  the line below it is a line to delete — and one that restates a note in `docs/` is worse, since
  two tellings of a decision are two places for it to drift (see
  [../roadmap/todo.md](../roadmap/todo.md), "Working on this file", for which telling wins).
- **A code comment does not cite a roadmap task number** (`T182f`, "roadmap task T32"). It says
  the reason itself. Which task brought the line in is history, and history belongs in the commit
  message, where `git log -S` and `git blame` find it. A number in a comment sends the reader to
  another file, and it goes wrong when the task is split, lettered or moved. Where a reader really
  needs the longer argument, link the ADR or the spec under `docs/specs/` by file name. Never link
  a plan under `docs/plans/`. The rule holds for the window's TypeScript too, and for the build's
  configuration: every TOML manifest and everything under `.github/`.
  `node scripts/check-task-citations.mjs`, in `scripts/gate.sh` and CI's `lint` job, fails on any
  citation.
- Where the claim is about **another crate's behaviour** — that `sqlx` reports this as that, that
  `tracing-subscriber` appends recorded fields — prefer a test to a sentence. A sentence is checked
  by nobody and goes stale in silence on the day the dependency is upgraded.

## Logging

- `tracing` spans around every RPC (`method`, `request_id`) and every job (`job_id`). Structured
  fields, not formatted strings: `info!(service = %id, port, "starting")`.
- Levels: `error!` = user-visible failure; `warn!` = degraded but continuing; `info!` = lifecycle
  events a user might care about; `debug!` = developer detail; `trace!` = firehose.
- **Never log secrets**: DB passwords, API tokens, private keys. Where a struct might contain one,
  implement `Debug` manually and redact.

## Platform code

All of it in `mixengine-platform` behind traits — see
[../architecture/platform-abstraction.md](../architecture/platform-abstraction.md). `#[cfg(windows)]`
appearing anywhere else fails review. Shell-outs use argument vectors, never interpolated strings.

It fails the build too, and not only review:
`no_crate_but_platform_compiles_a_line_away_by_operating_system` in
[workspace_layering.rs](../../crates/mixengine-proto/tests/workspace_layering.rs) scans the `src/` of
every crate but `platform`, `elevate` and `testkit`. The two files that legitimately hold one are
listed there with their reason, and a listed file has to keep the attribute inside its test module,
or be a `tests.rs` its parent declares under `#[cfg(test)]`, or a topic such a `tests.rs` declares —
so a permission cannot quietly widen into the product.

**`cfg!` is a different thing and is deliberately allowed.** A `cfg!(windows)` is a *value*: both
arms compile on every OS, type-check on every OS, and are reachable from a test on every OS, which
is what "cross-platform or not merged" asks for. A `#[cfg(windows)]` deletes code from the build, so
the other two platforms compile a different program and no test on them can say anything about it.
Where even the value can be avoided, avoid it — `recipes/mysql.rs` takes `windows` as a function
argument so that one table is exercised on every runner rather than half of it on each.
