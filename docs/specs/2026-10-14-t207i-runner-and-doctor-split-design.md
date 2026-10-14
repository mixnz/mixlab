---
status: implemented
date: 2026-10-14
task: T207i
---

# The service runner and the doctor, each one `impl` split by what its methods do (design)

Two daemon files in `scripts/file-size-baseline.json` are, for most of their length, one `impl`
block: `services/runner.rs` (3,219 lines; `impl Runner` is lines 547–2328, 34 methods) and
`doctor.rs` (2,055; `impl Doctor` is lines 71–1274, 26 methods). Methods of one type may live in
any module of the crate, so each `impl` splits into several, in child modules grouped by what the
methods do — T207d's move for `Registry`. The daemon's `main.rs`, the third file the roadmap
grouped here, is one 1,180-line function and needs a refactor rather than a move; it is task
**T207l** of its own (agreed on 2026-10-14).

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207i. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 holds: a commit only moves.

## How the groups were measured

Every method of each `impl` was listed with its range, and every `self.<method>(` in every other
method's body recorded as a call — the call graph a grouping has to respect. A group's method that
something outside the group calls becomes `pub(super)` (its `impl` is now in a child module, and a
private method is visible only there); a method that stays is visible to every child, as are the
file's private items, types and fields. The compiler checks the result in every commit.

## D1 — `runner.rs`: four groups and what stays

| Child module | Methods | Lines | Called from outside the group, so `pub(super)` |
| --- | --- | --- | --- |
| `runner/attempt.rs` — one life of the process: spawn, readiness, watching, and what its end means | `relay`, `attempt`, `supervise`, `port_conflict`, `refused_superuser`, `after_exit`, `answered_by_this_start`, `give_up` | 641 | `attempt` (by `live`), `after_exit` (by `watch_adopted`) |
| `runner/stop.rs` — asking a process to stop, within a budget, and killing it | `stop_adopted`, `stop`, `ask_to_stop`, `stopping_because`, `grace_for`, `within_budget`, `seeing_it_go`, `ended_meanwhile`, `kill` | 454 | `stop_adopted`, `stop`, `stopping_because`, `within_budget`, `kill` |
| `runner/environment.rs` — the surroundings a command runs in, the keyring reads among them | `where_commands_run`, `environment`, `walk_environment`, `spawn_environment` | 261 | `where_commands_run`, `environment` |
| `runner/reload.rs` — handing a running process its new configuration | `reload`, `reload_by_command`, `reload_by_signal` | 144 | `reload` |

Ten methods widen to `pub(super)`, which from `services::runner::<child>` means `services::runner`
and its children — no further than the file they came from.

**Top-level items go with a group only when nothing else uses them**: the `Stoppable` trait and
its two `impl`s (stop), `uncheckable` (attempt) and `repair_for` (environment). Everything else at
the top — the constants (`GONE`, `FLUSH`, `ENVIRONMENT`, …), `After`, `Released`, `Readiness`,
`Over`, `fold`, `worth_recording`, `OnFailure`, `Resolved`, the `Runner` struct and its fields —
is read by more than one group, or by `services/`, and stays.

**What stays in `runner.rs`**: the struct, the types and constants, `run`, `adopt`, `live`,
`wait_before_starting_again`, `watch_adopted`, `wait_out`, `stopped_before_running`, `record_exit`,
`move_to`, `move_to_noting`, `gone` (which `services/recovery.rs` imports), and the fold tests:
about 1,070 lines with its tests, out of the baseline.

### Tests

Measured by which `Runner` method each test calls:

- `runner.rs` keeps the four `fold` tests and what more than one child's tests use — `SLACK`,
  `runner`, `adopted_runner` — which become `pub(super)` so the children's test modules reach them
  as `super::super::tests::…`.
- `stop.rs`: `a_grace_the_budget_clamps_…`, `a_budget_that_is_already_spent_…`,
  `without_a_budget_a_specs_grace_period_…`, `a_spent_budget_still_leaves_…` (4), and what only
  they use: `SETTLE`, `BUDGET`, `NeverStops` with its `Stoppable` impl, `stopped_by_a_command`.
- `environment.rs`: the two environment-deadline tests and the five keyring tests (7).
- `attempt.rs`: `a_spawn_that_failed_leaves_its_sentence_on_the_row` (1).

16 before, 16 after.

## D2 — `doctor.rs`: two groups of checks

Every check is called by `report` alone, so every check that moves becomes `pub(super)`. Each
`*_outcome` function is the pure half of one check and moves with it, and its tests with it.

| Child module | Checks | Their pure halves | Tests |
| --- | --- | --- | --- |
| `doctor/installs.rs` — what is installed and what is on the path | `missing_installs`, `unrecorded`, `go_toolchain`, `java_pin`, `commands` | `missing_outcome`, `go_outcome`, `java_outcome`, `runtimes_hidden`, `elsewhere_on_the_path` | the missing, Go and Java tests, and the `shadowing_tests` module |
| `doctor/trust.rs` — who trusts this home's authority | `trust_store`, `browsers`, `site_certificates`, `trust_bundle`, `java_trust` | `java_trust_outcome` | the two `java_trust` tests |

Ten checks widen to `pub(super)`. What stays — `new`, `report`, the network and elevation checks,
the machine checks (`home_permissions` through `application_control`), `generated_config`,
`unsupervised`, `stranded` and their tests — is about 1,250 lines; out of the baseline.

## D3 — what a move could change here, and why it does not

- **Order of checks.** `report` calls the checks in its own order and stays where it is; moving a
  method's body does not move its call.
- **State and `Drop`.** `Runner` and `Doctor` own no lock across these methods and implement no
  `Drop`; their fields stay declared in the parent, which children read as they are.
- **Spawned tasks** (`spawn_environment`'s, the readiness watch in `supervise`) move inside their
  bodies unchanged.
- **Operating systems.** `runner.rs` has `cfg!` *values*, which compile everywhere; a move changes
  none. Checked on Windows here and on Linux through WSL, where the stop and keyring tests take
  their Unix branch.
- **Test paths** gain a segment (`services::runner::tests::x` → `services::runner::stop::tests::x`).
  `.config/nextest.toml` names neither file's tests; no workflow does. Unit-test count: 705 before
  and after.

## D4 — commits

1. `runner/environment.rs`, 2. `runner/reload.rs`, 3. `runner/stop.rs`, 4. `runner/attempt.rs`
(each moving its tests, and the first making the shared helpers `pub(super)`),
5. `doctor/installs.rs`, 6. `doctor/trust.rs`, 7. roadmap — no comment named an old place, so the
comments commit had nothing to hold. Each: 705 unit tests by `--list`; clippy and rustdoc with
`-D warnings`; `git diff --color-moved` leaving only `use` lines, the
`impl Runner {`/`impl Doctor {` wrappers, the `pub(super)`s, headers, test wrappers, reference links, and the eight `super::` paths
one module deeper now (inline doc links and calls such as `super::limits::from_proto`);
`check-file-sizes --shrink`. Then the gate, WSL, `ask-ci`, one pull request, squash-merged.

## MixLab

No part in it: `mix doctor` and the window's doctor screen read the same `DoctorReport`, built by
the same checks in the same order; nothing a service does changes.

## Not in scope

- `main.rs` — T207l. `doctor.rs`'s machine and network checks, which fit once the two groups go.

## Is there an ADR here? No

## Documentation, when it lands

`docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207i ticked and narrowed to the two files,
T207l added for `main.rs`; this spec flips to `implemented`.
