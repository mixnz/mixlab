---
status: draft
date: 2026-10-20
task: T209
---

# The daemon's walks as named steps (design)

Seven functions in `mixengined` decide, in one body, what happens next, with a `match` per failure
and a `tokio::select!` in the middle. Nothing is broken. The risk is the next change: an edit to one
arm can quietly change another, and the compiler says nothing. This task brings the seven up to the
rule in [`docs/standards/rust.md`](../standards/rust.md), *Control flow*, before that happens.

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209.

## What was measured

`cargo clippy -p mixengine-daemon -- -W clippy::cognitive_complexity` on `master` at `b5501d6f`
(limit 25):

| Walk | Where | Score |
| --- | --- | ---: |
| `repair_home` | `serve/repairs.rs:12` | 83 |
| `Runner::attempt` | `services/runner/attempt.rs:85` | 73 |
| `certificates` | `serve/machine.rs:74` | 71 |
| `Api::runtime_walk` | `api/upgrade/runtime.rs:347` | 70 |
| `Runtimes::after_recorded` | `runtimes/install.rs:320` | 65 |
| `Runner::ask_to_stop` | `services/runner/stop.rs:125` | 64 |
| `run` | `main.rs:125` | 61 |

`Runner::supervise` (`attempt.rs:330`, 53) is included too. It is the second half of `attempt`, and
its watch loop holds the `select!` the roadmap describes.

## The seven are two kinds of walk

- **Walks that decide**, where a step's result picks the next step: `attempt` + `supervise`,
  `ask_to_stop`, `runtime_walk`.
- **Lists of best-effort steps**, where every step runs, logs its own failure, and no later step
  reads its result: `repair_home`, `certificates`, `after_recorded`, and `run` (which only adds
  three early exits). Their score comes from inline `match`es that only log, and from `async move`
  blocks given to `background.spawn`.

## D1 — a walk that decides: a state `enum` and one function per step

- The states are an `enum`. Each step is a method that takes one state and returns the next. The
  walk is `loop { state = match state { … } }`.
- **A resource that must be given back goes in the variant that owns it** (`Supervised`, `Capture`,
  `Adopted`), so a state that loses it cannot compile. **A value several later steps read goes in
  one context `struct`** the walk owns. For `attempt`, that context is the `Runner` itself.
- **No shared `Walk` trait or macro.** The seven run in different contexts, and a plain `match`
  reads top to bottom. **No `Result<T, After>` with `?`** either: a normal end of life must not look
  like an error.

Two walks get a shape of their own:

- **`supervise`** stays in one state for hours and reacts to what arrives. Its `select!` becomes a
  step that returns what woke it (`Cancelled`, `Reload`, `Limits`, `Tick`). Each wake has a handler,
  then the checks run as named steps: exit, fold, probe. What the loop remembers becomes one
  `struct`.
- **`ask_to_stop`** is a straight line: plan, ask, wait. It gets an `enum` for the plan
  (`Kill` / `Signal` / `Command`), one step per plan, and the wait as a step. It does not get a loop
  over states that always come in the same order.

## D2 — a list of best-effort steps: named functions, no `enum`

Each step becomes a named function, and the walk is the list of calls. A spawned task becomes a
named `async fn`. An `enum` here would have one variant per line and decide nothing, which is the
"flat table" `rust.md` already exempts. **This departs from the roadmap's wording** ("each becomes
an explicit state `enum`").

`repair_home` and `after_recorded` run the same repairs (`pools::ensure`, `activation::ensure`,
`hold_all`, `conf.d`). They end up sharing one set of step functions. Each caller keeps its own log
messages.

## D3 — what a step decides is a pure function

The step gathers the facts (ports, the capture, the command's result). A pure function, with no
`self`, no `.await` and no process, says what they mean. The step then does it. These functions
get unit tests for every arm.

Platform constants become parameters. For example, `CAN_ASK_TO_STOP` becomes `can_ask: bool`, so
the Windows branch (ADR 0008) is tested on every OS.

## D4 — no behaviour change, proven by tests written first

- The same states, reasons, order of row writes and events, log lines, and cancel points. Every
  `select!` keeps its arms, their order and its `biased;`.
- **Characterization tests come before each walk's move.** They are written against the current
  code and must pass on it. The move must not change them.
- No existing test is deleted or edited. Task numbers in comments that move are dropped, and the
  reason they give stays.

## D5 — the limit stays enforced

Each refactored walk and each of its steps gets `#[warn(clippy::cognitive_complexity)]`, which
CI's `-D warnings` makes an error. The workspace does not get it, so tables are not hit. The target
is 15 or less, which leaves room because this nursery lint can rescore after a toolchain upgrade.
If an upgrade pushes a function over 25, that function is split in its own commit. The fix is never
an `#[allow]` or a raised threshold.

## D6 — one branch, one pull request

T209 is one branch and one pull request, pushed once. Inside it are five stages, each leaving the
workspace green, in this order:

1. `ask_to_stop`
2. `attempt` + `supervise`
3. `runtime_walk`
4. `repair_home`, `after_recorded`, `certificates`
5. `run`

`stop` comes before `attempt`, because `attempt` ends in it. The roadmap entry says the same.

## MixLab

The window has no part in this. No API method, type, event or log line changes (D4), and
`bindings/` stays as it is.

## Not in scope

- The daemon's other functions above 25. Tables stay. The rest follow the rule when next changed.
- `mixengine-core`'s `generate/served.rs` (58), which is near T209g.
- Turning the lint on for the workspace.
- Any change in behaviour. Those are filed as separate tasks.

## Is there an ADR here? No

The rule is already in `rust.md`. D2 is how its sentence about flat tables applies to these walks,
and stage 4 adds one line to `rust.md` saying so.
