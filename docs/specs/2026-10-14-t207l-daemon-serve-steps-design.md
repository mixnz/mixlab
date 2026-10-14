---
status: draft
date: 2026-10-14
task: T207l
---

# The daemon's `main.rs`: `Args` and `detach` moved out, `serve` made of its steps (design)

`crates/mixengine-daemon/src/main.rs` is 2,683 lines, and 1,163 of them are one function:
`serve` (lines 1214–2376), which builds every part of a running daemon, binds the endpoint, starts
the background work, accepts clients and shuts down. Around 40 steps share about 25 locals. The
file also holds the command line (`Args` and the types beside it, lines 229–512), the detached start
(`detach`, `take_over` and their four constants, 1002–1213), `run`, which opens the home and calls
`serve`, and the uninstall's last act (`remove_what_the_uninstall_armed`).

Moving `serve` whole into a file of its own would satisfy the limit, but it would leave a
1,163-line function in it, which is the thing the limit is there to prevent. So this task moves the
command line and the detached start, the way T207a–T207i moved code, and then **makes `serve` a
short function calling its steps in order**. Each step becomes a function, its block comment
becomes the function's doc comment, and the state it uses is passed to it explicitly.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207l. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 (a commit only moves) holds
for D1 and D2. D3 is the one part of phase 45 that changes code, and D4 says what it must not change.

## D1 — `args.rs`: the command line (a move)

`Args` and its `impl`, `Launch`, `Sources`, `LogLevel` and `LogFormat` with their `From` impls,
and `as_arg`: about 285 lines. `main.rs` names them through `use args::{Args, Launch, Sources};`.
Fields that `run`, `detach` and `serve` read become `pub(crate)`. Their docs do not change.

## D2 — `detach.rs`: starting detached, and taking over from a dying daemon (a move)

`detach`, `take_over`, and the four constants only they read (`DETACH_TIMEOUT`, `DETACH_POLL`,
`HANDOFF`, `HANDOFF_POLL`): about 230 lines. The two functions become `pub(crate)`.

## D3 — `serve/`: one function per step

`serve` moves to `serve.rs` and becomes the sequence of its steps. The steps go into children of
`serve`, grouped by what a start is doing at that point:

| File | Steps, in the order `serve` calls them | Returns |
| --- | --- | --- |
| `serve/projections.rs` | `refresh_bin` (the `bin/` rescan, T26/T185b), `seed_gallery` (T79) | — |
| `serve.rs`, in place | the endpoint, signals, the root token, `Background`, `Events`, `Jobs`, the host, the `Registry`, DNS, mDNS, `Elevation`, the administrator warning | the shared handles |
| `serve/recovery.rs` | `report_recovery` (what `services.recover()` found, T18) | — |
| `serve/machine.rs` | `ask_for_grants` (helper, port access, resolver: T85, T42, T45), `certificates` (discard a staged CA, ensure it, trust store, browsers, trust bundle, JDK trust stores, site certificates: T54, T48, T49a/b, T132, T27e, T50) | — |
| `serve/clocks.rs` | `start_clocks` (renewal T52, idle T69, sampler T71 and watchdog T71a) | the metrics handle |
| `serve/repairs.rs` | `repair_home` (offline adoption T182f, pools T32, activation T70/T70a/T123, conf.d T28, abandoned jobs T22) | what adoption left |
| `serve/sources.rs` | `signed_sources` (transport T72b, fetcher T23, registry T81, runtimes, packages, adopting what was left T182f, the updater T88, `complete_install` T185a, the two "not the published one" lines) | the four clients the API takes |
| `serve.rs`, in place | `api::Api::new` | the API |
| `serve/started.rs` | `after_the_api` (shared sites T75, extension pools T82/T82a, share revocation T76, restore after update T88, helper handshake T182b) | — |
| `serve/accept.rs` | `accept_until_stopped` (the accept loop, the stop with its escalation, the clients' grace: T9a, T22, T170), and with it `signalled_budget`, `shut_down`, the shutdown constants and their four tests | — |

**What stays inline in `serve`**: building the shared handles, because each of them is one
expression and every later step reads it; holding `_watching` (the `bin/` watch is stopped when
the guard is dropped, so it has to live in `serve`'s frame); `api::Api::new`; the "listening for
clients" line, then `services::autostart::start`. Estimated size: about 300 lines, most of them the
existing comments on those handles.

**State passed explicitly.** Most steps read the same few handles: `paths`, `store`, `config`, the
host, `events`, `jobs`, the `Registry`, `shims`, `Elevation`, the root token. They are gathered in
one `serve::Shared<'a>` of borrows, built once `Elevation` exists and passed as `&Shared`.
`Background` is passed separately as `&mut Background`, because it is the one handle a step
changes. A step that needs only one or two handles takes them directly. One struct of borrows is
used, not a parameter per handle, because seven parameters is the most clippy allows, and the
sources step alone reads ten.

**Comments.** Each step's `//` block becomes the function's `///` doc, with the same words. Where a
block points at a position ("a few hundred lines up", "the block above", "far below"), the
pointer is rewritten to name the step it meant (`refresh_bin`, `certificates`, …). A pointer
that counts lines is wrong once the lines move. These rewrites go in a commit of their own, after
the moves.

**The shutdown constants.** `CONFIRMATION_REPRIEVE` is also read as `crate::CONFIRMATION_REPRIEVE`
by `services/shutdown.rs` and a test in `services/runner/stop.rs`. `main.rs` re-exports it with
`use serve::accept::CONFIRMATION_REPRIEVE;`, so neither of those files changes.

## D4 — what the change must not alter

- **The order of everything a start does.** Every `.await`, every `background.spawn` and every
  `tracing` line happens in the order it happens today. Steps are called in source order, and no
  step is spawned in place of being awaited (or the other way round). The spawned names (`"trust
  bundle"`, `"jdk trust stores"`, `"activation ports"`, …) are unchanged.
- **What fails the start.** The `?`s stay where they are: the endpoint, the signal handlers, the
  transport, the fetcher, the registry client and the updater. A step that contains one returns
  `anyhow::Result`, and `serve` applies the `?`. Every "nothing here fails the start" block keeps
  logging and continuing.
- **Lifetimes.** The `bin/` watch guard, the listener, the signal handlers and `connections` live
  exactly as long as they do now. Clones into spawned tasks stay inside the step that spawns them.
- **Log messages and fields** are the same strings. They are checked as T207f checked moved
  literals: the set of string literals in `main.rs` before equals the set across `main.rs`,
  `args.rs`, `detach.rs` and `serve/` after.
- **Tests.** The four shutdown-arithmetic tests move to `serve/accept.rs`, and the two uninstall
  tests stay in `main.rs`. Unit-test count stays at 705 on Windows and 707 on Linux. The daemon's
  integration suites (`tests/api.rs` and the rest) start real daemons through `serve`, and they run
  in full on Windows and on Linux (WSL) before the push.

## D5 — commits

1. `args.rs` (move). 2. `detach.rs` (move). 3. `serve` and its shutdown helpers to `serve.rs` and
`serve/accept.rs` (move). 4–9. One step module per commit, in the table's order: `projections`,
`recovery`, `machine`, `clocks`, `repairs`, `sources`, `started`, then `accept_until_stopped`
taking the loop. 10. The comment pointers. 11. Roadmap, and `main.rs` out of the baseline.
Each commit gets: clippy and rustdoc with `-D warnings`, `cargo fmt --check`, the unit-test count,
the literal-set check, and `check-file-sizes --shrink`. The full integration suites run after
commit 10.

Expected sizes: `main.rs` about 700, `args.rs` 285, `detach.rs` 230, `serve.rs` about 300, the
largest step module (`machine.rs`) about 250. `main.rs` leaves the baseline, which then holds two
files.

## MixLab

No part in it. Nothing a client can ask for changes, and the daemon a client reaches answers the
same methods with the same startup behaviour.

## Not in scope

- `run` (lines 575–898), which is already a sequence of short steps with one call to `serve`.
- Changing what any step does. A step that looks wrong when it is moved is recorded as a follow-up
  in the phase file and is not fixed in this task.

## Is there an ADR here? No

No decision changes. The startup order, which is the part with consequences, is fixed by D4.

## Documentation, when it lands

`docs/architecture/` names `serve` only as the function that runs the daemon (checked when the
task lands). The phase file ticks T207l, and this spec flips to `implemented`.
