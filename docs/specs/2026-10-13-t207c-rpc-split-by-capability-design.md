---
status: approved
date: 2026-10-13
task: T207c
---

# The daemon's `rpc.rs` split by capability, its tests beside what they test (design)

`crates/mixengine-daemon/src/api/rpc.rs` is 5,683 lines, the longest file left in
`scripts/file-size-baseline.json`: 2,542 lines of code and a test module of 3,140 holding 87 of the
daemon's 705 unit tests. The [parent design](2026-10-13-files-that-fit-in-one-read-design.md) left
it out because a move there touches code that owns state. This design says what in it is the
routing table and stays, which handlers and helpers go and where, where each test and each fixture
goes, and why moving them changes nothing the daemon does.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207c. The parent
design's D7 and D8 hold here as they held for T207b's
[render split](2026-10-13-render-split-by-command-design.md): a helper goes where its callers are,
and a commit only moves.

## How the mapping was measured

On a `git archive` of `master` at `d789d1e4`, items were renamed with a suffix (`summary` →
`summary__zz`) and `cargo check -p mixengine-daemon --all-targets --message-format=json` was run.
Every place that names a renamed item then fails, and the compiler's primary span, mapped to the
item enclosing it, is a call site. Four passes, because one rename can hide another's uses:

| Pass | Renamed | Distinct uses | Unexplained errors |
| --- | --- | --- | --- |
| A | the 20 top-level items: 18 functions, `Failure`, `JOBS_LOOKED_AT`; and `api/mod.rs`'s re-export of `on_a_blocking_thread` with them | 389 | 0 |
| B | the 37 methods of `impl Api` | 73 | 12, all `E0277` cascading from `running_services` |
| C | the 24 helpers of the test module that are not tests | 193 | 0 |
| D | the 6 methods of the test fixture `Daemon` | 138 | 0 |

C and D are separate passes for the reason T207b missed `Lines::shows`: renaming the struct
`Daemon` turns every value of it into an error type, and a method called on an error type reports
nothing. With both in one pass `ask`, `expect`, `state`, `until` and `quiet` looked unused; alone,
they have 138 uses.

**A rename cannot say which method a test is about**, because most tests reach a handler through
JSON (`daemon.ask(rpc::method::SERVICE_START, …)`), not by its name. So a fifth measurement ran
the tests: `call_method` was given an `eprintln!` of the method it dispatches, and
`cargo test -p mixengine-daemon --bins api:: -- --show-output` recorded, per test, every method
it reached. 79 of the 87 tests reached at least one. Of the other eight, four never reach
`call_method` by design (a body that is not JSON, a request that is not one, an empty batch, a
protocol version from elsewhere), one calls a function in `create.rs` directly, and three return
early on Windows (`cfg!(windows)`); those three name their methods in their source as
`rpc::method::UPDATE_*` constants.

Nothing in `rpc.rs` is `#[cfg]`-gated on an operating system, so the graph measured on Windows is
the whole graph (see D8 for the `cfg!(windows)` expressions inside tests).

## D1 — `rpc.rs` is the routing table, and forty-seven items that are not

What is in it today:

- **The protocol**: `answer`, `dispatch`, `call_method` — whose `match` is the table, 114 method
  arms, the test-only `daemon.__panic` arm and the `unknown` arm, 813 lines on its own — and the
  helpers the table uses: `no_params`, `arguments`, `refused`, `encode_result`, `Failure` and its
  `impl`, `encode`, and `on_a_blocking_thread`.
- **Handlers and their helpers**: 37 methods in one `impl Api` block (lines 1146–2368) and 8 free
  functions after it (2370–2530), plus `busy` and `JOBS_LOOKED_AT` in the middle of the protocol
  helpers (1080–1112).

**The protocol stays; every handler and helper of a handler leaves.** After this task `rpc.rs`
holds no `impl Api`, which is a rule anybody can check by opening it.

**An arm stays exactly as it is, glue included.** About a hundred arms are one call into a
subsystem. Fourteen carry a few lines more: `runtime.uninstall` and `package.uninstall` ask
`not_being_upgraded` first; `site.share` and `site.unshare`; `extension.start` and
`extension.stop` resolve an extension to its service; `cert.issue` and `cert.status` resolve a site
reference; `cert.ca_rotate` and `cert.ca_uninstall` call `certs::authority`; the three
`autostart.*` arms run on `on_a_blocking_thread`; `job.list` wraps a list. Lifting any of that into
an `Api` method would be writing a new function, which is not a move (see Not in scope).

`on_a_blocking_thread` stays too: the `autostart.*` arms use it, its doc points at `call_method`'s
panic containment, and `uninstall.rs`, `uninstall/inventory.rs` and `updates.rs` reach it through
`api::on_a_blocking_thread`, whose re-export in `api/mod.rs` does not change.

## D2 — the mapping, item by item

The same rule as T207b: **one caller module, the item lives there; several, and it is about one
capability's subject, it lives in that capability's module and the others reach it there.** The
modules are the JSON-RPC namespaces, beside the ones `api/` already has (`create.rs`,
`front_end.rs`, `adopt_service.rs`, `restore.rs`, `upgrade.rs`, `apply.rs`, `metrics.rs`).
"Reached from" is the measured graph; `rpc` means `call_method`.

| Module | Items | Reached from |
| --- | --- | --- |
| `service.rs` (new) | `service_list`, `service_status`, `service_start`, `project_roots`, `service_stop`, `service_restart`, `service_reset_credential`, `walk`; `start_plan`, `two_subjects`, `refuse_project_scope`, `stop_plan`, `summary`, `role_of`, `walked` | rpc; `service_list` also `adopt_service`; `service_status` also `service_set_autostart`; `service_start` also `front_end` (twice), `upgrade::package`, `upgrade::runtime`; `service_stop` also `adopt_service`, `front_end`, `upgrade::package`, `upgrade::runtime`; `service_restart` also `upgrade::runtime`; `service_reset_credential` also `adopt_service`; `summary` also `create` (twice); `walked` also `stop_everything` |
| `service_settings.rs` (new) | `service_limits`, `service_set_limits`, `service_idle`, `service_set_idle`, `service_save_resources`, `service_set_save_resources`, `service_set_autostart`; `exemptions_for`, `named_service`, `spec_of`, `watchdog_of` | rpc; the last four only from inside this module |
| `daemon.rs` (new) | `status`, `version`, `doctor_report`, `bundle`, `set_credential_store`, `daemon_shutdown`, `stop_everything` | rpc; `status`, `version`, `doctor_report` also `bundle`; `stop_everything` from `daemon_shutdown`, `update_apply`, `update_finish` |
| `self_update.rs` (new) | `update_status`, `update_check`, `update_decide`, `update_apply`, `update_hand_over`, `update_finish`; `running_services` | rpc; `running_services` only from the six |
| `uninstall.rs` (new) | `uninstall_now` | rpc |
| `disk.rs` (new) | `cleanup_now`; `busy`, `JOBS_LOOKED_AT` | rpc; `busy` only from `cleanup_now`, the constant only from `busy` |
| `cert.rs` (new) | `ca_status` | rpc |
| `metrics.rs` (exists) | `metrics_snapshot`, `metrics_history` | rpc |

Where the rule left a choice, and what was chosen:

| Item | Choice | Why |
| --- | --- | --- |
| `walked` | `service.rs` | a walk is `service.*`'s; `stop_everything` builds one and imports it |
| `summary`, `role_of` | `service.rs` | `create.rs` reports the service it made or removed as `service.list` does |
| `stop_everything` | `daemon.rs` | `daemon.shutdown` is what it was written for (its doc calls it *the ordered half of `Api::daemon_shutdown`*); an update stops the same way and calls it |
| limits, idle, save-resources, autostart setting | `service_settings.rs`, not `service.rs` | together they would be about 1,400 lines; apart, `service.rs` is the walks and what a service is, and `service_settings.rs` what may be set on one. All seven are `service.set_*` or their readings |
| `ca_status` | a `cert.rs` of its own, about 20 lines | it is `cert.ca_status`'s handler, and leaving it would break "`rpc.rs` has no `impl Api`" for one wrapper. The other four `cert.*` arms stay in the table (D1) |
| `metrics_snapshot`, `metrics_history` | the existing `metrics.rs` | the `metrics` capability already has a file in `api/`, for `GET /metrics`; its `//!` gains one paragraph naming the two methods. A second `metrics_*.rs` would split one capability across two names |
| `uninstall_now` | `uninstall.rs` beside `crate::uninstall` | the same naming as `api::metrics` beside `crate::metrics`; `api::` says which |
| `daemon.uninstall_plan`, `daemon.disk_usage` | stay arms | each is one call into `crate::uninstall` / `crate::disk`; there is no handler to move, only their tests (D4) |

Each file's length after the move, measured by running the whole sequence on a scratch copy
(D7), with its `use` lines, headers and test module wrappers in:

| File | Lines | Unit tests |
| --- | --- | --- |
| `rpc.rs` | 1,328 (of which `call_method` 813) | 15 |
| `rpc/arms.rs` (new, test-only) | 220 | 7 |
| `service.rs` | 1,082 | 20 |
| `service_settings.rs` | 314 | 0 |
| `daemon.rs` | 553 | 8 |
| `self_update.rs` | 734 | 17 |
| `uninstall.rs` | 610 | 9 |
| `disk.rs` | 147 | 2 |
| `cert.rs` | 17 | 0 |
| `metrics.rs` | 208 (121 today) | 4 (1 today) |
| `front_end.rs` | 994 (700 today) | 5 (0 today) |
| `create.rs` | 495 (450 today) | 2 (1 today) |
| `test_daemon.rs` (new, test-only) | 472 | 0 |
| `api/mod.rs` | 786 (777 today) | 3 |

The largest new file is `service.rs`; nothing enters the baseline, and `rpc.rs` leaves it.

## D3 — `rpc/arms.rs`: the tests of arms with no handler to sit beside

Seven tests are about arms that call straight into a subsystem with no `Api` method of their own:
`path.*` (1 test), `autostart.*` (2) and `database.*` (4). Left in `rpc.rs` they bring it to 1,537 lines
(measured: its length after the `create` commit) — over the limit, still in the baseline, against this task's goal.

They cannot go to the subsystem: every one of them needs `test_daemon` (D4), which builds an `Api`
from its private fields and so can only be seen inside `api`. So they go to **`api/rpc/arms.rs`**,
declared in `rpc.rs` as `#[cfg(test)] mod arms;` — a child of the module holding the arms they
test; Rust allows a file module with a directory of children beside it (`rpc.rs` and `rpc/`), as
`render.rs` had during T207b. It holds tests and nothing else; its tests are `api::rpc::arms::<name>`.

The other arm-only tests go where their capability has a module after the split:
`daemon.uninstall_plan`'s two tests to `uninstall.rs`, `daemon.disk_usage`'s to `disk.rs`.

## D4 — a test goes with the function it is written about

**Every test moves into the `#[cfg(test)] mod tests` of the module holding the handler whose answer
it asserts on**, in the same commit as that handler, as the test run in *How the mapping was
measured* recorded it. A test that reaches a
second method only to set the scene (`service.start` before a `service.stop`, `update.check` before
an `update.hand_over`, `job.wait` after a `daemon.uninstall`, `elevation.status` after an uninstall
plan) is not about the second.

| Module | Tests |
| --- | --- |
| `rpc.rs` (stays) | 15 — the protocol: a string id, an unknown method, parameters to a method that takes none, the four spellings of none, a body that is not JSON, a request that is not one, notifications (3), batches (4), a panicking handler, another protocol version |
| `rpc/arms.rs` | 7 — `the_path_is_reported_then_taken_and_then_given_back`, the two `autostart` tests, the four `database` tests |
| `service.rs` | 20 — `service.list` (5, including `a_source_that_cannot_answer_is_reported_with_the_failure_s_own_code`), `service.status` (2), `service.start` (5, including `a_target_says_nothing_and_means_every_declared_service`), the project scope (3, including `a_stop_and_a_restart_have_no_project_scope`), `service.reset_credential` (2), `service.stop` and `service.restart` (3) |
| `daemon.rs` | 8 — `status_answers_with_what_the_daemon_knows_about_itself`, `the_status_line_says_nothing_about_updates_until_there_is_something_to_say`, the six shutdown tests |
| `self_update.rs` | 17 — every `update.*` test |
| `uninstall.rs` | 9 — the two plan tests and seven uninstall tests |
| `disk.rs` | 2 — the disk read, the cleanup refusal |
| `metrics.rs` | 3 — two history reads, the snapshot with no loop |
| `front_end.rs` | 5 — every `service.set_front_end` switch test |
| `create.rs` | 1 — `only_a_front_end_change_waits_for_another_one`, which calls `one_front_end_change_at_a_time` (defined in `create.rs`) directly |

87 before, 87 after. Two comments in the test module stand alone between tests, introducing a group
(`` // `service.*` — T19a`` and `// The project scope — roadmap task **T125**`): each moves with the
first test of its group, to `service.rs`.

`metrics.rs` already has a `mod tests` with one test of the stream; the three move into it.
`create.rs` already has a test module, `autostart_tests`, named for what it tests; the new test goes
into a new `mod tests` beside it rather than into a module whose name it does not fit. That module
has no `use super::*;`: its one test uses only fixtures, and the glob would be an unused import.
`front_end.rs` has no test module and gains one.

### Fixtures

The test module's fixtures, by who uses them (passes C and D):

| Fixture | Used by tests in | Goes to |
| --- | --- | --- |
| `Daemon` (+ `call`, `ask`, `expect`, `state`, `until`, `quiet`), `daemon`, `undeclared` | every module | **`test_daemon.rs`** |
| `daemon_on` | `uninstall`, `front_end` | `test_daemon.rs` |
| `daemon_reading` | `self_update` (through `installed_by_pkg`) | `test_daemon.rs` |
| `call` (the free function) | `rpc`, `rpc::arms`, `metrics`, `daemon` | `test_daemon.rs` |
| `answer_json` | `Daemon::call`, and `rpc`'s panic test | `test_daemon.rs` |
| `web_and_db` | `service`, `daemon`, `self_update` | `test_daemon.rs` |
| `SHUTDOWN_GRACE` | `daemon_reading` only | `test_daemon.rs`, private |
| `succeeded` | `rpc` batch tests | stays in `rpc.rs` |
| `slow_to_stop` | one shutdown test | `daemon.rs` |
| `kill`, `uninstall_result`, `finished_job` | uninstall tests | `uninstall.rs` |
| `PKG_BYTES`, `PKG_OFFERED`, `installed_by_pkg`, `checked`, `hand_over_params`, `install_over` | update tests | `self_update.rs` |
| `front_end`, `packages_root`, `installed`, `switched` | switch tests | `front_end.rs` |

**`api/test_daemon.rs`** is declared in `api/mod.rs` as `#[cfg(test)] mod test_daemon;`. It is not
called `fixture` because the tests already import `crate::services::fixture` under that name; it is
named for what it builds. It has to be inside `api`: `daemon_reading` writes an `Api` struct literal,
which names every private field. Its shared items are `pub(super)` — from `api::test_daemon` that is
`pub(in crate::api)`, visible to every test module under `api` (`api::service::tests`,
`api::rpc::arms`) and nowhere else in the crate. All five of `Daemon`'s fields become
`pub(super)` with it, because tests in other modules read them (`daemon.api.store`,
`daemon.host`) — `_home` included, which
`a_complete_uninstall_keeps_the_home_exactly_when_the_machine_is_not_clear` reads for the home's
path. Its name keeps the underscore: a move renames nothing. `answer_json` calls the protocol's
`answer`, which `test_daemon.rs` imports from `super::rpc`, where it stays.

## D5 — visibility, the narrowest that compiles

- **26 `Api` methods go from private to `pub(super)`**, because `call_method` stays in `rpc.rs` and
  a private inherent method is visible only in the module holding its `impl`: the 25 handlers
  `call_method` calls that are private today (`status`, `version`, `doctor_report`, `bundle`,
  `set_credential_store`, `daemon_shutdown`, the six `update_*`, `uninstall_now`, `cleanup_now`,
  `ca_status`, `metrics_snapshot`, `metrics_history`, `service_status`, `service_limits`,
  `service_set_limits`, `service_idle`, `service_set_idle`, `service_save_resources`,
  `service_set_save_resources`, `service_set_autostart`), and `stop_everything`, which
  `self_update.rs` calls.
- **One free function**: `walked`, which `daemon.rs` imports.
- Already `pub(super)` and unchanged: `service_list`, `service_start`, `service_stop`,
  `service_restart`, `service_reset_credential`, `summary`.
- Everything else stays private to its new file: `project_roots`, `walk`, `start_plan`,
  `two_subjects`, `refuse_project_scope`, `stop_plan`, `role_of`, `running_services`,
  `exemptions_for`, `named_service`, `spec_of`, `watchdog_of`, `busy`, `JOBS_LOOKED_AT`.
- The new modules are plain `mod` in `api/mod.rs`: nothing in them is reached from outside `api`.

No `pub(super) use` re-exports from `rpc.rs`: a caller names where the item is, as T207a and T207b
chose.

## D6 — what callers change

Almost nothing, because a method is called on `self` or `api` and its file is not in the call:

- `create.rs`: `super::rpc::summary(` twice → `super::service::summary(`.
- `daemon.rs`: `use super::service::walked;`. Between the `service` commit and the `daemon`
  commit, `stop_everything` is still in `rpc.rs` and calls `walked`, so `rpc.rs` carries the same
  import for those two commits and drops it when `stop_everything` leaves.
- `api/mod.rs`: eight `mod` lines (seven new modules and `#[cfg(test)] mod test_daemon;`), none
  removed. `rpc/arms.rs` is declared in `rpc.rs`.
- `front_end.rs:48`: `JOBS_LOOKED_AT`'s doc reads ``[`super::rpc`]'s constant, for its reason``;
  the constant moves to `disk.rs`, so the link becomes ``[`super::disk`]``. It is a link whose
  target moved — D7's fourth kind — and is edited in the `disk` commit.
- In the tests, `super::super::Events`, `super::super::Armed`, `super::super::Started` and
  `super::super::Shutdown` in `daemon_reading` become `super::…`, one level up from where they
  were; `Daemon::call` reaches `super::rpc::answer` (via `answer_json`), which stays.

## D7 — a move, and only a move

What a commit may change, and nothing else:

1. Where an item is, with every line above it that belongs to it: its `///` lines, a `//` comment
   directly above it, and its attributes (`#[derive(Debug)]` on `Failure`, which stays;
   `#[tokio::test]` on every test). There is no multi-line attribute in the file.
2. Its visibility, per D5.
3. `use` lines, and a path that names a moved item (D6); a module's `//!` header, which a new file
   needs; `metrics.rs`'s one added paragraph.
4. Links whose target moved, and intra-doc links that now cross a file: written as reference links
   at the end of the doc block (`` /// [`walked`]: super::service::walked ``), so that the line
   that uses them does not change and does not pass 100 columns. The dry run's `cargo doc` with
   `-D warnings` lists every one.
5. Test module wrappers: `#[cfg(test)] mod tests {`, `use super::*;`, and the `mixengine_proto`
   names and fixtures a moved test uses.
6. `rpc.rs`'s entry in `scripts/file-size-baseline.json`, lowered by
   `node scripts/check-file-sizes.mjs --shrink` once every other check of the commit is green, and
   removed by it when the file is under 1,500.

No rename, no signature, no body, no reordering inside a file: items keep their order from `rpc.rs`,
so `git diff --color-moved=dimmed-zebra` shows each one as a block, and anything it does not dim is
one of the six kinds above. Where rustfmt re-wraps a line because a path or `pub(super)` made it
longer, that re-wrap is part of the same change, read line by line.

**Order of commits: what others import first, so that no new file ever imports from `rpc.rs`
something that is going to move.**

1. `test_daemon.rs` — every later test module imports from it. From here until the last move,
   `rpc.rs`'s own test module imports the fixture from it too.
2. `service.rs` — `walked` (for `daemon.rs`), `summary` (for `create.rs`) and `service_status` (for
   `service_settings.rs`) leave before anything that reaches them.
3. `service_settings.rs`.
4. `daemon.rs` — `stop_everything` leaves before `self_update.rs` calls it.
5. `self_update.rs`, 6. `uninstall.rs`, 7. `disk.rs`, 8. `cert.rs`, 9. `metrics.rs` — the rest of the
   handlers, in the order they sit in `rpc.rs`.
10. `front_end.rs`'s tests, 11. `create.rs`'s test, 12. `rpc/arms.rs`.

Measured by running all twelve on a scratch copy, each green under clippy and rustdoc with
`-D warnings` and each listing 705 unit tests: `rpc.rs` is 5,683 → 5,240, 4,188, 3,899, 3,372,
2,665, 2,076, 1,950, 1,941, 1,862, 1,576, 1,537, and 1,328 after the twelfth commit, where
`--shrink` takes it out of the baseline.

**One commit per module moved**, each green on its own; the pull request is squash-merged, as
T207a's and T207b's were.

## D8 — what a move could change, and why here it does not

- **State.** `Api`'s state — the `front_end` and `upgrading` mutexes, `Armed`, the `Shutdown`
  token, every `Arc` it holds — is fields declared in `api/mod.rs` and built in `Api::new`. None of
  it moves, and `Api::new` is not touched, so construction order is what it is today.
- **Locks and their order** live inside handler bodies (`daemon_shutdown`'s `Going` guard before the
  registry latch, `update_apply`'s guard after the swap, `walk`'s spawned task bounded by the root
  token). Bodies do not change, so neither does any order.
- **Statics and one-time initialisation.** `rpc.rs` has no `static`, `LazyLock`, `OnceLock`,
  `once_cell` or `thread_local!` (searched), so there is no initialisation order a file boundary
  could change.
- **Method resolution.** Inherent methods of `Api` may sit in any module of the crate and resolve
  the same; their names are already unique on `Api` (a duplicate would not compile today). The one
  thing a file boundary changes is privacy, which D5 handles and the compiler checks.
- **Spawned work.** `call_method`'s `tokio::spawn` (the panic containment) stays in `rpc.rs`;
  `walk`'s and `uninstall_now`'s spawns move inside their bodies, unchanged.
- **Tests.** Each test builds its own `Home` (a temporary directory with its own database). No test
  sets an environment variable or the working directory (searched). A test's path changes
  (`api::rpc::tests::x` → `api::service::tests::x`), and nothing selects tests by that path:
  `.config/nextest.toml`'s overrides name `services::`, `activation::` and `services::ports::`, and
  no workflow or script names `api::rpc`.
- **Operating systems.** No `#[cfg]` on an OS in the file. Seven `cfg!(windows)` *expressions*
  inside tests (the uninstall plan's row count, the uninstall tests' process launcher and
  `kill`, `packages_root`, three update tests) compile on every OS and choose at run time. On Windows the
  three update tests return early and the uninstall tests take their Windows branch, so a Windows run
  does not execute those halves: the moved `self_update`, `uninstall` and `front_end` tests
  are run once on Linux through WSL, and `cargo check --all-targets --target aarch64-apple-darwin`
  covers macOS, as the `before-pushing` skill describes.
- **The test-only arm** `#[cfg(test)] "daemon.__panic"` is part of the table and stays in it.

## D9 — comments that will be wrong after the move

Found while measuring. Prose is not edited in a move; these are fixed in one commit of their own
after the last move, so that every move commit stays "the same code, somewhere else":

| Where | Says | After the move |
| --- | --- | --- |
| `crates/mixengine-daemon/src/services/reset.rs:25` | what a reset *is* "is `api::rpc`" | `api::service` |
| `crates/mixengine-daemon/tests/metrics.rs:4` | "the two methods against a fixture in `api::rpc`" | `api::metrics`, with the fixture in `api::test_daemon` |
| `crates/mixengine-cli/tests/self_update.rs:7` | "the refusals in `api/rpc.rs`" | `api/self_update.rs` |
| `crates/mixengine-cli/tests/daemon.rs:15` | "`api/rpc.rs` drives the same case … and `render.rs` proves the sentence" | `api/daemon.rs`; and `render.rs` has been `render/daemon.rs` since T207b, which that commit missed |
| `crates/mixengine-daemon/src/api/create.rs:3` | its own file "rather than more of `super::rpc`, which is already the longest in this crate" | the reason stays true; "already the longest" stops being. Reworded to keep the reason in the past tense |

Still true after the move, and left alone: `crash.rs:4` and `services/mod.rs:327` (both about
`call_method`, which stays), `front_end.rs:6` ("beside it rather than in `super::rpc`"), and
`api/mod.rs`'s `//!` line for `rpc` (*"the protocol. Batches, notifications, method dispatch, panic
containment"* — which is what it becomes). ADR 0022, implemented specs, `docs/reviews/` and the
roadmap name `rpc.rs` as records of their day and stay as written; none of them links to a line
that moves except `docs/reviews/2026-08-27.md`'s `#L…` anchors, which were already stale.

No doc comment in the moved code was found attached to the wrong item; the dry run's rustdoc pass
checks that again.

## MixLab

No part in it. This changes how the daemon's source is laid out and nothing it answers: no method,
no parameter, no type in `mixengine-proto` or `bindings/`, no error text. MixLab's `mixengine`
module talks to the daemon over JSON-RPC, typed against `bindings/`, and never sees this file;
`node scripts/check-client-surface.mjs` gives the same answer before and after.

## Testing

What proves nothing changed, before the first commit and after each:

- **The unit-test count.** `cargo test -p mixengine-daemon --bins -- --list | grep -c ': test$'` is
  **705** before and after. (`--lib --bins` fails: the package has no library target.) Of them,
  `grep -c '^api::rpc::tests::'` is 87 before; after, `api::rpc::` holds 22 (15 in `tests`, 7 in
  `arms`), and the per-module counts are D4's.
- **The integration suites** under `crates/mixengine-daemon/tests/` drive a built daemon and read
  none of this source; `cargo test -p mixengine-daemon --tests` runs them unchanged.
- **`git diff --color-moved=dimmed-zebra`** over each commit leaves undimmed only D7's six kinds.
- **`bash scripts/gate.sh`**: clippy `-D warnings` catches an import left unused or an item made
  dead by a wrong visibility; rustdoc `-D warnings` catches a link that no longer resolves; the
  ratchet catches a baseline entry left behind.
- **Linux and macOS** as D8 says, before the push.

No new tests: there is no new behaviour, and a test of the file layout tests nothing a user sees.

## Not in scope

- **Reshaping the table.** Splitting `call_method` into one `match` per namespace, or lifting the
  fourteen arms with glue into `Api` methods, would make `rpc.rs` shorter still and is a change to
  code, not a move. `rpc.rs` ends 172 lines under the limit — some thirty five-line arms of
  headroom — and when that runs out, it is a design of its own.
- `services/mod.rs` (4,613) and `api/apply.rs` (1,729), still in the baseline.
- `create.rs`'s `autostart_tests` keeps its name.
- Fixing anything noticed while moving, other than D9's comments.

## Is there an ADR here? No

The rule is the parent design's D7, and the standard behind it is
[docs/standards/rust.md](../standards/rust.md)'s *modules by capability* and its 1,500-line limit.
Nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207c added with this spec (`[ ]`), ticked
  when it lands; the goal line and **M45** gain `api/rpc.rs`. `docs/roadmap/todo.md`'s row for
  phase 45 says `T207–T207c` and counts four tasks.
- This spec flipped to `implemented` in the commit that ticks T207c, and `docs/specs/README.md`
  regenerated.
- Nothing in `docs/standards/rust.md`, `docs/architecture/` or `CLAUDE.md`: none of them names the
  file's layout.
