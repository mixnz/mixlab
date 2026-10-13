---
status: draft
date: 2026-10-13
task: T207d
---

# The daemon's `services/mod.rs` split by capability, its tests beside what they test (design)

`crates/mixengine-daemon/src/services/mod.rs` is 4,613 lines, the longest file left in
`scripts/file-size-baseline.json`: about 2,345 lines of code and a test module of 2,268 (from line
2346) holding 42 of the daemon's 705 unit tests. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md) left it out because a move there
touches code that owns state, and [T207c](2026-10-13-t207c-rpc-split-by-capability-design.md) left
it out again for the same reason. This design says what in it is the `Registry` and its state and
stays, which operations and helpers go and where, where each test and each test helper goes, and why
moving them changes nothing the daemon does.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207d. The parent
design's D7 and D8 hold here as they held for T207c: a helper goes where its callers are, and a
commit only moves.

## How the mapping was read

Unlike T207c, the mapping was not measured with renaming passes. It was read: every item and every
`impl Registry` method was listed with its line range, and each method's callers were found with
`grep -rnoE '\.<name>\('` over `crates/mixengine-daemon/src`, each test's calls with the same search
over its body. A name grep over-reports (`.start(` and `.stop(` exist on other types) and never
under-reports a method call, so the risk it carries is a visibility left too wide, never one too
narrow — and **the compiler is the measurement of record**: every commit is checked with
`cargo check --all-targets`, clippy and rustdoc under `-D warnings`, and a visibility that is wrong
in either direction fails one of them (`dead_code` / `unused` for too wide when nothing reaches it,
`E0624` for too narrow).

Private items and private fields need no measurement of their own here, and that is the property of
this file that makes the split cheap: **every new module is a child of `services`**, and Rust lets a
module see every private item and private field of its ancestors. `Registry`'s fields, `Running`,
`Stopping`, `lock`, `now`, `record` and `record_noting` stay private in `mod.rs` and remain visible
to every new file. Only what moves *down* into a child and is still called from `mod.rs` or a
sibling needs a wider visibility (D5).

## D1 — `mod.rs` is the `Registry` and its state; the operations on it leave

What is in it today:

- **The registry and its state** — the module declarations and re-exports, the `Registry` struct
  (its 17 fields and their documentation, ~107 lines), `Running` (the per-service entry),
  `Stopping` and its `impl Drop`, and the vocabulary every walk speaks: `Fault`, `Walk`,
  `Undeclarable`. Its construction (`with_welcome`, `new`) and its read accessors (`store`,
  `holder`, `host`, `welcome`, `supervised`, `logs`, `is_running`). The four writes into a live
  `Running` entry (`set_limits`, `stopping_because`, `over_memory`, `ask_to_reload`). The shared
  primitives: `record`, `record_noting`, `now`, `lock`.
- **Operations**, in five capabilities: what is declared (the graph and what a walk remembers
  from it), starting, stopping, shutting down (and its `Budget`), and recovery.

**The state stays, every operation leaves.** After this task `mod.rs` holds every field, every type
a field is made of, and the methods that read or write one field in one statement; any method that
*walks* — awaits, spawns, waits on a runner — lives in its capability's file.

Kept in `mod.rs` though they are not state, and why:

| Item | Why it stays |
| --- | --- |
| `set_limits`, `stopping_because`, `over_memory`, `ask_to_reload` | each is one lock of `running` and one `send`/`notify` into a `Running` field; they are `Running`'s interface, 78 lines together |
| `restarted`, `stop_then_start`, `restart` | the walk API both `api/service.rs` and `watchdog.rs` call as `services::restarted`, `services::stop_then_start`, `super::restart`; they compose `stop` and `start` and belong to neither. The parent design's D7: several callers and no single capability's subject → the root of the module |
| `record`, `record_noting`, `now`, `lock` | called from recovery, start and `runner.rs` (`super::record_noting`, `use super::now`) |
| `Stopping` and `impl Drop for Stopping` | the guard type of the `stopping` field; built by `claim_stop` (stop) and held by `shut_down` (shutdown), so two siblings use it |

## D2 — the mapping, item by item

New files beside the existing ones (`activate.rs`, `autostart.rs`, `databases.rs`, `hold.rs`,
`idle.rs`, `limits.rs`, `reset.rs`, `runner.rs`, `spec.rs`, `step.rs`, `watchdog.rs`, …). Each
holds its own `impl Registry { … }` block for the methods it takes; an inherent impl may sit in any
module of the crate and resolves the same.

| Module | Items (in their order in `mod.rs`) | Lines today (approx.) | Called from |
| --- | --- | --- | --- |
| `graph.rs` (new) | `reconfigure`, `graph`, `graph_for_sweep`, `forget_graph`, `front_end_program`, `front_end_tls_port`, `remember_rituals`, `ritual_for`, `has_credential_reset`, `reset_credential`, `remember_provisioning`, `provisioning_for`, `hand_over`, `wakeable_at` | 594–871, 999–1015, 1611–1640 | `graph` from every walk and ~15 files outside; `ritual_for` from `begin` (start); `provisioning_for` from `supervise` (start) and `databases.rs`; `remember_*`, `hand_over` only from `graph` |
| `start.rs` (new) | `Start`; `ensure_running`, `start`, `start_because`, `begin`, `supervise`, `block`; `settled`, `settled_after_asking`, `decided_by` | 390–398, 872–920, 1429–1484, 1692–1940, 2048–2113, 2269–2332 | `start` from api, `activate`, `hold`, `idle`, `updates`, `stop_then_start`; `start_because` from `autostart`; `supervise` also from `reconcile` (recovery); `ensure_running` from `databases.rs` |
| `stop.rs` (new) | `stop`, `stop_because`, `stop_one`, `claim_stop` | 1485–1543, 1941–2047 | `stop` from api, `idle`, `extensions`, `stop_then_start`; `claim_stop` also from `shut_down` (shutdown) |
| `shutdown.rs` (new) | `Budget`, `Deadlines`, `impl Budget`; `stopping_within`, `is_shutting_down`, `shut_down` | 64–157, 540–593, 1544–1600 | `Budget` from `mod.rs` (the field) and `runner.rs`; `stopping_within` from `api/daemon.rs` and `main.rs`; `shut_down` from `main.rs`, `api/test_daemon.rs` |
| `recovery.rs` (new) | `Recovery`, `impl Recovery`; `recover`, `reconcile_stranded`, `reconcile`, `discard`; `survivor` | 421–461, 1016–1428, 2115–2143 | `recover` from `main.rs`; `reconcile_stranded` from `repair.rs`; the rest only from inside |

Where the rule left a choice, and what was chosen:

| Item | Choice | Why |
| --- | --- | --- |
| `ensure_running` | `start.rs`, not `graph.rs` where it sits | it builds a plan and hands it to `start`; its own doc says it is "not a second start path" |
| `wakeable_at` | `graph.rs` | it renders the declarations through `spec::generator` and answers from that; it reads no runner |
| `Start` | `start.rs` | used only by `begin`, `settled*`, `decided_by`. `Walk`'s doc links `[`Start::Failed`]`, which gains a reference link (D7 kind 4) |
| `Recovery` | `recovery.rs` | named only by `recover`, `reconcile_stranded`, `reconcile` and its test; outside, `main.rs` reads its fields through the returned value and never names the type |
| `Budget` | `shutdown.rs` | it is what a shutdown grants; its doc is task T9a's shutdown budget. `runner.rs` reads it and names it by its new path (D6) |
| `is_shutting_down` | `shutdown.rs` | read by `begin` and `start_because` and `api/daemon.rs`, but it is the question "has a shutdown begun", answered from the flag `stopping_within` and `shut_down` set |
| The module for the declared set | named `graph.rs`, not `declared.rs` | `services::declared` is already a re-exported function (`pub(crate) use spec::{…, declared}`), and a module of the same name beside it would read as the same thing |

Each file's length after the move, measured by running the move script over the whole sequence on
a scratch copy of `master` and formatting each step with `cargo fmt`:

| File | Lines | Unit tests |
| --- | --- | --- |
| `mod.rs` | 713 | 1 |
| `graph.rs` | 543 | 5 |
| `start.rs` | 1,191 | 14 |
| `stop.rs` | 546 | 6 |
| `shutdown.rs` | 623 | 7 |
| `recovery.rs` | 962 | 8 |
| `watchdog.rs` | 652 (516 today) | +1 |
| `fixture.rs` | 352 (310 today) | 0 |

`mod.rs` after each step: 4,613 → 4,577 (fixture), 4,062 (graph), 2,899 (start), 2,379 (stop),
1,784 (shutdown), 847 (recovery), 713 (watchdog). It leaves the baseline at the recovery step.

The largest is `start.rs`; nothing enters the baseline, and `mod.rs` leaves it.

## D3 — the declarations in `mod.rs`

Five plain `mod` lines, in the alphabetical order the existing list keeps: `mod graph;`,
`mod recovery;`, `mod shutdown;`, `mod start;`, `mod stop;`. None is `pub(crate)`: nothing outside
`services` names a path into them, because every caller outside reaches a method on `Registry`
(whose path is unchanged) — except two doc links, which D6 lists.

## D4 — a test goes with the function it is written about

Every test moves into the `#[cfg(test)] mod tests` of the module holding the method whose answer it
asserts on, in the same commit as that method. A test that walks `graph` and `start` to set the
scene and then asserts on a stop is a stop test. This was read from each test's body, not measured
by running: most tests reach a method by name (`registry.stop(&plan)`), so the read is direct.

| Module | Tests |
| --- | --- |
| `mod.rs` (stays) | 1 — `every_state_change_is_published_as_it_is_persisted` (about `record`) |
| `graph.rs` | 5 — the three sweep tests (`the_sweep_reuses_a_young_graph_and_renders_an_old_one`, `any_walk_refreshes_what_the_sweep_sees`, `a_forgotten_graph_is_rendered_again`), `a_rendering_that_changed_reaches_the_process_already_running` (`hand_over`), `a_source_that_cannot_answer_is_kept_apart_from_a_declaration_that_is_wrong` (`Undeclarable` from `graph`) |
| `start.rs` | 14 — `ensuring_a_service_is_running_is_idempotent`, `ensuring_a_service_nothing_declares_is_refused`, `a_service_starts_runs_and_stops`, `a_walk_carries_the_detail_the_runner_wrote_with_the_reason`, `ensure_running_says_the_runners_detail_before_the_state_machines_reason`, `a_service_that_never_becomes_ready_fails_rather_than_waiting_forever`, `a_service_whose_port_is_held_by_another_program_says_which_one`, `starting_a_service_that_is_already_up_does_not_spawn_a_second_one`, `what_depends_on_a_failure_is_never_spawned`, `a_service_whose_policy_never_gives_up_does_not_hold_the_walk_for_ever`, `a_walk_waits_out_the_retries_a_bounded_policy_is_allowed`, `a_service_in_a_restart_backoff_is_not_reported_as_up`, `an_explicit_start_cuts_short_the_backoff_a_crash_loop_is_sitting_in`, `a_start_asked_for_mid_start_is_taken_by_the_start_that_answers_it` |
| `stop.rs` | 6 — `a_service_with_a_stop_command_is_stopped_by_running_it`, `a_service_that_stopped_while_its_stop_command_failed_keeps_its_exit_code`, `a_stop_command_that_cannot_be_run_still_leaves_the_service_stopped`, `a_shutdown_and_a_person_do_not_leave_the_same_stop_behind` (the claim `stop_one` takes), `a_stop_that_left_the_service_supervised_is_not_reported_as_reached`, `two_stops_of_one_service_are_both_answered_by_the_stop_that_happened` |
| `shutdown.rs` | 7 — `a_shutdown_budget_bounds_every_service_left_to_stop_and_not_each_one`, `a_second_shutdown_can_shorten_a_budget_and_never_extend_one`, `a_shutdown_grace_no_clock_can_hold_bounds_nothing_rather_than_panicking`, `a_stop_during_a_ready_check_does_not_wait_the_check_out` (its stop is `shut_down`), `a_start_that_arrives_once_a_shutdown_has_begun_is_refused`, `a_shutdown_draining_the_map_under_a_walk_does_not_fail_the_rest_of_it`, `a_daemon_on_its_way_out_waits_for_what_it_supervises` |
| `recovery.rs` | 8 — `a_service_that_outlived_the_last_daemon_is_supervised_again`, `an_adopted_service_stops_with_the_environment_entries_that_did_resolve`, `a_row_whose_process_did_not_survive_is_cleared_and_nothing_is_signalled`, `a_survivor_nothing_declares_any_more_is_stopped`, `a_survivor_stopped_because_the_declarations_could_not_be_read_says_so`, `a_survivor_that_would_not_stop_is_not_reported_as_nothing_to_do`, `a_service_left_mid_start_is_stopped_rather_than_taken_for_ready`, `an_adopted_service_that_ends_is_put_back_as_this_daemon_s_own` |
| `watchdog.rs` (exists, has `mod tests`) | 1 — `a_service_over_a_ceiling_this_machine_cannot_hold_is_warned_about_and_restarted`: it drives `Watchdog::minute` and asserts on what the watchdog did |

42 before, 42 after; 705 for the package before and after. Four tests that call `shut_down` only
to clean up (`a_service_whose_policy_never_gives_up…`, `a_service_in_a_restart_backoff…`,
`an_explicit_start_cuts_short…`, `a_start_asked_for_mid_start…`) are start tests: the assertion is
about the start. A test reaching `registry.shutdown` (a private field) from `shutdown::tests` still
compiles: that module is a descendant of `services`.

### Test helpers

| Helper | Used by tests in | Goes to |
| --- | --- | --- |
| `row` | every module but `graph`'s sweep tests | **`fixture.rs`**, `pub(crate)` like everything there |
| `gone` (the test's, not `runner::gone`) | recovery (4), shutdown (1) | `fixture.rs`, `pub(crate)` |
| `SILENCE` | recovery (1), start (1) | `fixture.rs`, `pub(crate)` |
| `crash_looping`, `crashes_after_coming_up` | start only | `start.rs`'s tests |
| `left_running`, `left_running_with` | recovery only | `recovery.rs`'s tests |
| `over_the_line`, `settles_to` | the watchdog test only | `watchdog.rs`'s tests |

`fixture.rs` already exists as `#[cfg(test)] pub(crate) mod fixture`, and is where shared test
scaffolding in `services` lives: no second fixture module. Its `//!` says "two test modules need"
it; after this there are more, but the header is prose and is not edited in a move (D9).

`gone` in `fixture.rs` sits beside `runner::gone`, which `recovery.rs` imports for `discard`. A test
module that does `use super::*;` and also imports `fixture::gone` by name gets the fixture's: an
explicit import shadows a glob one, exactly as the test function shadows the glob today.

## D5 — visibility, the narrowest that compiles

- **`Registry::ritual_for`**: private → `pub(super)` (called by `begin` in `start.rs`).
- **`Registry::supervise`**: private → `pub(super)` (called by `reconcile` in `recovery.rs`).
- **`Registry::claim_stop`**: private → `pub(super)` (called by `shut_down` in `shutdown.rs`).
- **`Budget::narrow_to`**: private → `pub(super)`. `stopping_within` calls it (same file after the
  move) and so do two tests in `runner.rs` (`runner.budget.narrow_to(…)`), which are in
  `services::runner::tests` and lose sight of a private item of `services::shutdown`.
- **`Budget::remaining` and `Budget::reprieve` keep the text `pub(super)`, and its meaning
  narrows**: from `services` it is `pub(in crate)`, from `services::shutdown` it is
  `pub(in crate::services)`. The only callers are in `runner.rs`, inside `services`. This is the
  one change of meaning in the task and it is a narrowing the compiler checks.
- `Deadlines`, `begin`, `block`, `stop_one`, `reconcile`, `discard`, `survivor`, `settled`,
  `settled_after_asking`, `decided_by`, `Start`, `remember_rituals`, `remember_provisioning`,
  `hand_over` stay private: each is called only from its new file.
- Everything already `pub(crate)` stays `pub(crate)`.
- No `pub(crate) use` re-export from `mod.rs`: a caller names where an item is, as T207a–c chose.

## D6 — what callers change

Almost nothing, because a method is called on a `Registry` and its file is not in the call:

- `runner.rs`: `super::Budget` → `super::shutdown::Budget` in the field type (line 433) and in four
  doc links (87, 424, 1708, 1762); `use super::super::Budget;` in its tests →
  `use super::super::shutdown::Budget;`; the doc link `[`super::settled`]` (218) →
  `[`super::start::settled`]`. Same lengths ± a few characters; `runner.rs` is in the baseline and
  does not grow by a line.
- `main.rs:188`: the doc link `services::Budget` → `services::shutdown::Budget`.
- `mod.rs`: `use shutdown::Budget;` for the field; its `use runner::{Readiness, Runner, gone};`
  loses `gone` with recovery and `Runner` with start; `Adopted`, `StartTime`, `Notify`,
  `JoinHandle`, `LogSubject` and the rest of its `use` lines lose whatever no remaining item uses,
  commit by commit, as clippy reports them.
- Links by `Registry::method` (`databases.rs:243`, `runner.rs:397`, `runner.rs:1191`, `spec.rs:107`,
  `doctor.rs:1216`, `api/mod.rs:480`, `api/service.rs:187`) resolve through the type and do not
  change.

## D7 — a move, and only a move

What a commit may change, and nothing else — T207c's six kinds:

1. Where an item is, with every line above it that belongs to it: its `///` lines, a `//` comment
   directly above it, and its attributes (`#[derive(…)]`, `#[tokio::test]`). **"Belongs to" is by
   syntax, not by meaning**: what is attached today stays attached (see *Doc comments attached to
   the wrong item* below).
2. Its visibility, per D5.
3. `use` lines, and a path that names a moved item (D6); a new file's `//!` header, which links to
   no module that does not exist yet at that commit.
4. Links whose target moved, and intra-doc links that now cross a file, written as reference links
   at the end of the doc block so the line using them does not change.
5. Wrappers: `impl Registry {` / `}` around the methods a file takes, `#[cfg(test)] mod tests {`,
   `use super::*;` (only where a test uses something of its parent), and the fixture names a test
   uses.
6. `mod.rs`'s entry in `scripts/file-size-baseline.json`, lowered by
   `node scripts/check-file-sizes.mjs --shrink` once every other check is green, and removed by it
   when the file is under 1,500.

No rename, no signature, no body, no reordering inside a file: items keep their order from
`mod.rs`, so `git diff --color-moved=dimmed-zebra` shows each as a block.

### Doc comments attached to the wrong item

Reading the file found five places where a doc block sits above a different item than the one it
describes, because an item was inserted between a doc and its function at some point:

| Lines | The doc says | Attached to |
| --- | --- | --- |
| 595–611 | `graph`'s ("The declared services, checked and ordered … # Errors") | `reconfigure`, whose own doc follows it in the same block |
| 769–786 | `hand_over`'s ("Tell every service that is running …") | `remember_rituals`, followed by its own |
| 921–925 | `ask_to_reload`'s ("Ask one running service to re-read …") | `set_limits`, followed by its own |
| 2222 | `record`'s first line is a `//`, the rest `///` | `record` (the `//` line is not part of the doc) |
| 362 | `Start`'s ("How a start of one service ended …") | `Fault`, followed by its own |

A move does not fix them. Each stays above the item it is attached to: `reconfigure` and `graph`
both go to `graph.rs` in order; `remember_rituals` and `hand_over` both to `graph.rs`;
`set_limits` and `ask_to_reload` both stay in `mod.rs`; `Fault` stays in `mod.rs` and keeps
`Start`'s line while `Start` goes to `start.rs` without it. They are fixed in a commit of their own
after the last move (D9).

### Order of commits

What a sibling calls first, so no file ever reaches into one that is about to move:

1. **`fixture.rs`** — `row`, `gone`, `SILENCE`; every later test module imports them from there,
   and so does `mod.rs`'s test module until it is down to its one test.
2. **`graph.rs`** — `ritual_for` and `provisioning_for` leave before `start.rs` calls them.
3. **`start.rs`** — `supervise` leaves before `recovery.rs` calls it.
4. **`stop.rs`** — `claim_stop` leaves before `shutdown.rs` calls it.
5. **`shutdown.rs`** — and `runner.rs`'s and `main.rs`'s paths to `Budget` with it.
6. **`recovery.rs`**.
7. **`watchdog.rs`**'s test.

Methods need no import, so the order costs no temporary `use`: a method moved to `start.rs` before
`recovery.rs` exists is called from `mod.rs`'s `reconcile` through `self`, and only its visibility
had to be widened, in the commit that moved it.

**One commit per module**, each green on its own; the pull request is squash-merged, as T207a–c
were.

## D8 — what a move could change, and why here it does not

- **State and construction.** Every field is declared in `mod.rs` and built in `Registry::new`,
  which does not move or change. `Budget::default()` is called there and resolves to the same type
  through `use shutdown::Budget;`.
- **Locks and their order.** The one order that matters — `running` before `stopping`, documented
  on the `stopping` field — is taken inside `stop_one` (stop) and `shut_down` (shutdown), each
  holding `running` while it calls `claim_stop`, which takes `stopping`. Bodies do not change, so
  the order does not; the field's doc that states it stays in `mod.rs` with the field. `last_graph`,
  `rituals`, `provisioning` and `Budget`'s mutex are each taken alone and never across an await,
  and still are.
- **`Drop`.** `impl Drop for Stopping` stays in `mod.rs` beside the type. Where a `Drop` impl is
  written has no effect on when it runs: it runs where a `Stopping` goes out of scope — at the end
  of `stop_one`'s frame (`_stopping`) and at the end of each loop turn in `shut_down` — and those
  bodies move unchanged. A future dropped mid-stop (the case the guard exists for) drops it the same
  way from either file.
- **Private fields read from another module.** Every new module is a child of `services`, which
  sees `Registry`'s, `Running`'s and `Stopping`'s private fields; no field changes visibility. A
  file outside `services` reads none of them today and still does not.
- **Statics and one-time initialisation.** `mod.rs` has none: no `static`, `LazyLock`, `OnceLock`
  or `thread_local!` (searched). `generations` is an `AtomicU64` field, not a static.
- **`#[cfg(debug_assertions)] mod fakeservice;`** is a declaration in `mod.rs` and stays there,
  with its comment. Nothing that moves names `fakeservice` (the one mention, at 2414, is prose in
  `left_running`'s doc about the testkit program, which is a different thing). `cargo check
  --workspace --release --all-targets` is run once at the end to prove the release build, where the
  module is absent, still compiles.
- **`#[cfg(test)] pub(crate) mod fixture;`** stays as declared; it gains three items.
- **Operating systems.** No `#[cfg]` on an OS and no `cfg!` in `mod.rs` (searched). What a
  Windows run cannot see is what the moved code *calls* on other OSes:
  `mixengine_platform::process::sweep_stale_groups` (Linux cgroups) at the top of `recover`, and
  `Adopted::identify`/`stop` in recovery. Those calls do not change, but the recovery and stop tests
  exercise them differently per OS, so they are run on Linux through WSL before the push, and
  `cargo check --all-targets --target aarch64-apple-darwin` covers macOS (`before-pushing` skill).
- **Method resolution.** Inherent methods of `Registry` resolve the same from any module, and
  their names are already unique on the type.
- **Tests and their selection.** Test paths change (`services::tests::x` →
  `services::start::tests::x`). `.config/nextest.toml` selects `services::(hold|activate)::tests::`
  for its `port-windows` group, and none of the 42 is in it today or after; no workflow or script
  names `services::tests`. `mixengine-core/src/jobs.rs:812` mentions
  `services::tests::a_state_this_build_does_not_know_blames_the_row`, which is `mixengine-core`'s
  own `services` module, not this one.

## D9 — comments that will be wrong after the move

Fixed in one commit of their own after the last move, so that every move commit stays "the same
code, somewhere else":

- The five misattached doc blocks above: `graph`'s doc moved from above `reconfigure` to above
  `graph`; `hand_over`'s from above `remember_rituals` to above `hand_over`; `ask_to_reload`'s from
  above `set_limits` to above `ask_to_reload`; `Start`'s line from above `Fault` to above `Start`
  (in `start.rs`); `record`'s `//` made `///`. Pure moves of doc lines, no wording changed; the
  rustdoc of each item then says what it is about.
- `fixture.rs`'s `//!` ("The scaffolding two test modules need … Two callers"): after the move it
  is used by six test modules in `services` plus `api`. Reworded to drop the count.
- `mod.rs`'s `//!` describes the registry, the walk and `Registry::recover`, links by type, and
  stays true; it is left alone.

Comments in other files naming `services/mod.rs` as a place: none in code (searched); the two in
`crash.rs` tests are sample crash data, a file path in a fabricated report, and stay. Plans under
`docs/plans/` and implemented specs are records of their day and stay as written.

## MixLab

No part in it. This changes how the daemon's source is laid out and nothing it answers: no method,
no parameter, no type in `mixengine-proto` or `bindings/`, no error text, no log line. MixLab's
`mixengine` module talks to the daemon over JSON-RPC, typed against `bindings/`, and never sees this
file; `node scripts/check-client-surface.mjs` gives the same answer before and after.

## Testing

What proves nothing changed, before the first commit and after each:

- **The unit-test count.** `cargo test -p mixengine-daemon --bins -- --list | grep -c ': test$'`
  is **705** before (recorded on `master` at `269a05c8`) and after every commit; `services::tests::`
  is 42 before and 1 after, and the per-module counts are D4's.
- **The integration suites** under `crates/mixengine-daemon/tests/` drive a built daemon and read
  none of this source; `cargo test -p mixengine-daemon --tests` runs them unchanged.
- **`git diff --color-moved=dimmed-zebra`** over each commit: the lines it does not dim are printed
  and read, and each is one of D7's six kinds.
- **clippy and rustdoc with `-D warnings`** on every commit, then
  `node scripts/check-file-sizes.mjs --shrink` once those are green.
- **`cargo check --workspace --release --all-targets`**, for `cfg(debug_assertions)`.
- **Linux through WSL** per the `before-pushing` skill (`mixengine-testkit --bins` built first, per
  `local-test-environment`), and macOS by `cargo check --target aarch64-apple-darwin`.

No new tests: there is no new behaviour.

## Not in scope

- Reshaping `Registry`: splitting its fields into sub-structs per capability (a `Supervision` with
  `running`, `stopping`, `generations`; a `Declared` with `rituals`, `provisioning`, `last_graph`)
  would make each file own its state, and is a change to code, not a move.
- `api/apply.rs` (1,729) and `services/runner.rs` (3,219), still in the baseline.
- Fixing anything noticed while moving, other than D9's comments.

## Is there an ADR here? No

The rule is the parent design's D7, and the standard behind it is
[docs/standards/rust.md](../standards/rust.md)'s *modules by capability* and its 1,500-line limit.
Nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207d added with this spec (`[ ]`),
  ticked when it lands; the goal line and **M45** gain `services/mod.rs`.
  `docs/roadmap/todo.md`'s row for phase 45 says `T207–T207d` and counts five tasks.
- This spec flipped to `implemented` in the commit that ticks T207d, and `docs/specs/README.md`
  regenerated.
- `docs/architecture/process-supervision.md` names no file in `services/` and needs nothing.
