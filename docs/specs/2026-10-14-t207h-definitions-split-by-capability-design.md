---
status: draft
date: 2026-10-14
task: T207h
---

# Four definition files past the limit, each giving one capability a file of its own (design)

Four files in `scripts/file-size-baseline.json` have code, not tests, past 1,500 lines, and little
state: proto `service.rs` (code 1,814), core `generate/recipe.rs` (1,521), platform `process.rs`
(1,508) and platform `windows/process.rs` (1,590). Each holds one block that is a subject of its
own, and moving that block is enough. This design names the four blocks, what each reaches back
for, and why the public paths of three crates other crates build against do not change.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207h. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 holds: a commit only moves.

## D1 — the four blocks

Measured by listing every item of each block and searching the workspace for it, as in T207g.

| File | Block | To | Lines |
| --- | --- | --- | --- |
| proto `service.rs` | how a service is run rather than which one it is: `ReadyCheck`, `HealthCheck`, `HealthProbe`, `Backoff`, `RestartPolicy`, `StopBehaviour`, `ReloadBehaviour`, `ReloadSignal`, `Priority`, `ResourceLimits`, `IdleProbe`, `IdlePolicy`, `IdleSource`, `IdleExemption`, `LogPolicy`, with every `impl` of each (two of `RestartPolicy`) | `service/policy.rs` | 690 |
| core `generate/recipe.rs` | `Context` — what a recipe is handed — with its two `impl`s (one `#[cfg(test)]`), `TEST_HOME`, `bound`, and the private structs only `Context::rendering` builds: `Rendering`, `CertificateView`, `Instance`, `Origin`, `Layout` | `generate/context.rs` | 700 |
| platform `process.rs` | running a program once and keeping what it said: `LAST_WORDS`, `Ran` and its `impl`, `run_once`, `run_once_with_input`, `run`, `drain`, `said` | `process/once.rs` | 359 |
| platform `windows/process.rs` | starting a child and reading it: `RawChild` and its `impl`, `spawn_child`, `spawn_shell_child`, `DEFAULT_PATHEXT`, `pathext`, `find_program`, `shell`, `run_child`, `Reading` and its `impl`, `read_on_a_thread` | `windows/process/child.rs` | 385 |

After: `service.rs` 1,124 lines of code and 818 of tests — past the limit together, under it apart
— so its test module then moves to `service/tests.rs` by T207e's convention; `recipe.rs` about
1,300 with its tests; `process.rs` about 1,255; `windows/process.rs` about 1,395. All four leave the
baseline.

## D2 — what each block reaches back for, and the visibility that changes

- **`service/policy.rs`** reaches `ServiceId` (`pub`). Nothing changes visibility.
- **`process/once.rs`** reaches `hide_stdio_from_children`, `Exit`, `describe` and the private
  `state_the_whole_environment`. A child module sees its parent's private items, so nothing
  changes.
- **`windows/process/child.rs`** reaches `Group` and the private
  `stop_handing_on_the_standard_handles`, which the rest of the file also calls and so stays.
  Nothing changes.
- **`generate/context.rs`** is a sibling of `recipe.rs`, not its child: it reaches
  `FrontEndAddition`, `ServiceCertificate`, `Endpoints` and `program`, all `pub`. The other way,
  `recipe.rs`'s `render` calls `Context::rendering` and `bound`, so those two and `Rendering` (its
  return type) become `pub(super)` — the one place in this task where visibility widens, to
  `generate` and no further.

## D3 — paths: what other crates build against does not move

Three of the four crates are built against by others, so their paths are a contract:

- **proto**: `lib.rs` re-exports `service::{Backoff, …, StopBehaviour}` by name. `service.rs`
  declares `mod policy;` and `pub use policy::{…}` for the fifteen types, so `service::Backoff`
  still names it, `lib.rs` is unchanged, and so is every `mixengine_proto::Backoff` in the
  workspace. The `ts` feature's `#[ts(export)]` writes each binding by type name, not module path;
  `bash packaging/bindings.sh --check` proves `bindings/` unchanged.
- **platform**: `process.rs` re-exports `pub use once::{Ran, run_once, run_once_with_input}` and
  `pub(crate) use once::LAST_WORDS`, so `mixengine_platform::process::run_once` and
  `crate::process::LAST_WORDS` are where they were. `windows/process.rs` re-exports its five
  `pub(crate)` items from `child`, so `process.rs`'s `sys::spawn_child` and the rest still resolve.
- **core**: `generate.rs` re-exports `Context` from `context` instead of `recipe`, so
  `crate::generate::Context` is unchanged. The sixteen imports and six doc links that name
  `generate::recipe::Context`, and the five test paths naming `recipe::TEST_HOME`, change to
  `generate::context::` — core's callers name where an item is, as T207a–d chose, since nothing
  outside core sees `recipe`.

These are re-exports from a module to its own parent, which is what a directory module does; the
"no re-exports" rule of T207c is about a module answering for a sibling's items.

## D4 — the tests

- proto: nearly every test builds a `ServiceSpec` and asserts on what `ServiceSpec::validate`
  says about it — the policies included, since their rules are enforced there. The module stays
  whole and moves to `service/tests.rs` (T207e), one commit after the code.
- core: `a_context_with_no_certificate_renders_a_falsy_group` and `a_template_cannot_see_a_secret`
  are about `Context`'s rendering and go with it; the other twelve stay.
- platform: the two test modules are about spawning and supervising, and stay.

Unit-test counts per crate are unchanged, counted by `--list` before and after (proto, core,
platform `--lib`).

## D5 — what a move could change here

- **Multi-line attributes.** Unlike T207c's file, `service.rs` has `#[derive(` lists spread over
  lines. An item moves with its whole attribute block, which the move script finds by walking up
  from the item to the line that opens each `#[`.
- **Operating systems.** `windows/process.rs` compiles only on Windows; `process.rs` holds
  `#[cfg(not(windows))]` items (`drain`) that compile only elsewhere. Checked on Windows here, on
  Linux through WSL (`cargo clippy -p mixengine-platform --all-targets --all-features`), and for
  macOS with the `before-pushing` skill's `--target aarch64-apple-darwin` commands for `platform`
  and `elevate`.
- **The privileged helper.** proto and platform are in its closure, so its fingerprint moves;
  `HELPER_VERSION` is already 0.1.8 against a 0.1.7 release (T207e), so `helper-lock.sh --check`
  passes without a second bump. `cargo clippy -p mixengine-elevate` checks its `elevated`-only
  build, where `process` is absent.
- **No `static`, no `Drop` moves** in any block (searched); `windows/process.rs`'s `HIDING` static
  and `impl Drop for Group` stay where they are.

## D6 — commits

One per block, then proto's tests, the comments that name an old place (found by search after the
last move), and the roadmap tick: 1. `service/policy.rs`, 2. `service/tests.rs`,
3. `generate/context.rs`, 4. `process/once.rs`, 5. `windows/process/child.rs`, 6. comments,
7. roadmap. Each move commit: counts unchanged; clippy and rustdoc with `-D warnings`; for proto
`bindings.sh --check`; `git diff --color-moved` leaving only `use` lines, paths, the three
`pub(super)`, headers and wrappers; `check-file-sizes --shrink`. Then the gate, WSL, `ask-ci`, one
pull request, squash-merged.

## MixLab

No part in it. The types and functions keep their names, their paths through each crate's public
surface, their `serde` shapes and their TypeScript bindings; nothing the daemon answers changes.

## Not in scope

- Splitting the policies further, or `ServiceSpec`'s 760 lines, below the limit.
- T207i's daemon files, T207j's `lib.rs`, T207k's `SqlTable.tsx`.

## Is there an ADR here? No

Nothing cross-cutting is decided; D3's re-exports are a module's ordinary way of keeping its own
surface.

## Documentation, when it lands

`docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207h ticked with this spec, which flips to
`implemented`; `docs/roadmap/todo.md` counts it.
