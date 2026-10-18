---
status: implemented
date: 2026-10-18
task: T207p
---

# Code past 1,000 lines in the root workspace, split by capability (design)

After T207o, 22 of the files in `scripts/file-size-baseline.json` sit in the root workspace and are
past 1,000 lines because of their code, not their tests. Most of them are one large `impl`, or a
run of free functions over one table, and split the way T207d and T207i split `Registry` and
`Runner`: each group of items that does one thing moves to a child module of the file it came
from. Six of them also still carry an inline `mod tests { … }`, which moves to `foo/tests.rs` under
T207e's convention. One file, the daemon's `api/rpc.rs`, is one 805-line `match` and needs a
function boundary that does not exist yet (D3).

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207p. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 holds: a commit only moves
code. The groups follow [T207i](2026-10-14-t207i-runner-and-doctor-split-design.md)'s rules, and the
test moves follow [T207e](2026-10-14-t207e-test-modules-to-their-own-file-design.md)'s. Measured on
`master` at `f8e719c9`.

## D1 — the rules every file follows

1. **A group is a contiguous run of the original lines where one exists**, starting at the doc
   comment and attributes of its first item. It goes to `foo/<group>.rs`, which opens with a
   one-line `//!`, its own `use` lines and, for methods, `impl Foo { … }` around them. Methods of
   one type may live in any module of the crate, so an inherent `impl` splits without a re-export.
2. **Visibility widens only as far as the file it came from.** A moved item that something outside
   its group calls becomes `pub(super)`; a child already sees its parent's private items, types and
   fields. The compiler decides which items need it, and every commit is checked by it.
3. **A public type that moves is re-exported** from the parent with `pub use <group>::Type;`, so no
   path outside the file changes, in this crate or any other. This applies only in proto and
   platform. A free function the parent calls is imported with `use <group>::…;`.
4. **No test changes its path.** Test modules that are already in their own file stay where they
   are. An inline `mod tests` moves to `foo/tests.rs` under T207e's rule and keeps `foo::tests::x`.
   A test that names a moved free function gains a `use` line and nothing else. No test moves with
   the code it asserts on: T207i moved tests along with their methods, but here 16 of the 22 files
   already have their tests in topic files that T207o laid out, and moving them again would give
   every test a second new path in one week.
5. **Top-level items move with a group only when nothing outside the group uses them**, as in
   T207i. Constants, error types and the struct itself stay.
6. **No `static`, `Drop` or `unsafe impl` is separated from its type.** Where one belongs to a type
   that moves (`Group` in `windows/process.rs`), it moves with that type. Otherwise it stays.

## D2 — where each file splits

Lengths are of the original lines, rounded, before the new headers and before `rustfmt`. "Left" is
what stays in the parent.

### core (six files)

| File | Moves to | What | Lines | Left |
| --- | --- | --- | --- | --- |
| `config.rs` (1,215) | `config/paths.rs` | `[paths]`: `PathOverrides`, `RequestedPaths`, `set_paths`, `replace_atomically`, the relocation check and `names_a_directory` with its two Windows-spelling helpers (597–971) | 375 | ~715 |
| | `config/tests.rs` | the inline tests | 126 | |
| `generate.rs` (1,195) | `generate/declarations.rs` | `declarations`, `declarations_with`, `declared`, `settings`, `drift` | 235 | ~770 |
| | `generate/front_end.rs` | `would_serve`, `activators`, `held_while_stopped`, `documents`, `fragments` | 190 | |
| `blueprints/plan.rs` (1,154) | `plan/steps.rs` | the step builders `plan` calls: `register`, `made`, `runtime`, `package`, `ensure`, `database_step`, `domain_step`, `extension` (500–984) | 485 | ~670 |
| `index/packages.rs` (1,178) | `packages/root.rs` | `cached_root`, `read_root`, `refresh_root` | 245 | ~700 |
| | `packages/kinds.rs` | `read_kind`, `cached_kind`, `fetch_kind`, `fetching`, `fetch_kinds`, `fetch_each`, `store` | 230 | |
| `services.rs` (1,280) | `services/write.rs` | `create`, `delete`, `set_idle`, `set_autostart`, `idle_minutes`, `set_limits` (366–734) | 370 | ~620 |
| | `services/transitions.rs` | `failure_note` through `transition_noting`, `started`, `ended` (970–1262) | 290 | |
| `sites.rs` (1,402) | `sites/rows.rs` | the columns and payloads a kind and a route write, their inverses, the state word, and `write_domains`, `write_links`, `write_routes` (817–988, 1141–1260) | 290 | ~820 |
| | `sites/sharing.rs` | the shared URL and mDNS name, `name_taken`, `web_port`, `read_sharing`, `set_sharing` (990–1139) | 150 | |
| | `sites/checks.rs` | `pool_is_free_for`, `declared_services_exist`, `holder` (1262–1398) | 140 | |

### daemon (nine files)

| File | Moves to | What | Lines | Left |
| --- | --- | --- | --- | --- |
| `api/apply.rs` (1,294) | `apply/prerequisites.rs` | `resolve`, `install_targets`, `blueprint_needs`, `meet_requirements`, `installed_package` | 190 | ~780 |
| | `apply/consent.rs` | `identity`, `questions`, `consent_refusal`, `refusal` | 225 | |
| | `apply/dotenv.rs` | `write_dotenv` and its `impl Api`, `dotenv_refusal`, `dotenv_value` | 100 | |
| `api/rpc.rs` (1,329) | `rpc/table.rs` | the routing table (D3) | 790 | ~540 |
| `elevation.rs` (1,479) | `elevation/require.rs` | what a producer asks for: `require_helper`, `learn_installed_helper`, `require_hosts`, `require_resolver`, `require_trust_store`, `require_port_access`, `preflight` | 375 | ~745 |
| | `elevation/flush.rs` | carrying the queue out: `grant`, `reserve`, `flush`, `judge`, `remember` | 360 | |
| `extensions.rs` (1,167) | `extensions/configure.rs` | `configure`, `ensure_pools`, `make_pool_reachable`, `stop_what_it_runs`, `stop_one`, `configure_one`, `config_secret`, `forget_config_secret` | 345 | ~725 |
| | `extensions/tests.rs` | the inline tests | 94 | |
| `runtimes.rs` (1,372) | `runtimes/install.rs` | `install`, `perform`, `after_recorded` | 420 | ~780 |
| | `runtimes/uninstall.rs` | `uninstall` | 170 | |
| `sites.rs` (1,403) | `sites/checked.rs` | what a request is checked and resolved against: `expect`, `checked`, `settled`, `resolved_pool`, `routes`, `repaired`, and the free route checks `natively` through `route_list_is_writable` | 400 | ~750 |
| | `sites/lookup.rs` | `project`, `project_by_id`, `holder_of`, `editable`, the adopted-site steps, `linked`, `lookup`, `detail` | 255 | |
| `uninstall.rs` (1,305) | `uninstall/outcome.rs` | what a run leaves and how it is answered: `drop_helper_installations`, `aligned`, `forgotten`, `keep_credentials`, `needs_the_helper`, `remove_completed`, `settle` | 250 | ~815 |
| | `uninstall/tests.rs` | the inline tests | 240 | |
| `uninstall/inventory.rs` (1,218) | `inventory/machine.rs` | the machine's rows: `hosts_block`, `resolver_wiring`, `port_access`, `firewall_rules`, `certificate_rows`, `browser_row` | 330 | ~750 |
| | `inventory/credential_store.rs` | `credentials`, `credential_rows` | 140 | |
| `updates.rs` (1,255) | `updates/install.rs` | `complete_install`, `restore_after_update`, `start_again`, `release`, `installer_of` | 235 | ~810 |
| | `updates/hand_over.rs` | `hand_over`, `fetch_package`, `installed_version`, `settle_handover`, `finishable` | 210 | |

### platform (three files)

| File | Moves to | What | Lines | Left |
| --- | --- | --- | --- | --- |
| `process.rs` (1,250) | `process/adopted.rs` | `Adopted` and its `impl`, re-exported | 150 | ~900 |
| | `process/table.rs` | what the OS says about a pid: `StartTime`, the start-time read, the parent table, `Usage` and its read, re-exported | 95 | |
| | `process/tests.rs` | the inline tests | 106 | |
| `windows/process.rs` (1,384) | `windows/process/job.rs` | the job object: `Group`, its `unsafe impl Send`/`Sync`, `group()`, its `impl` and its `impl Drop` | 540 | ~845 |
| `windows/restricted.rs` (1,313) | `restricted/spawn.rs` | starting a child under a token: `spawn`, `spawn_raw`, `spawn_from`, `pipe`, `command_line`, `append`, `AttributeList` with its `impl Drop` | 455 | ~590 |
| | `restricted/tests.rs` | the inline tests | 268 | |

`windows/process.rs` keeps its 191 lines of inline tests: the file is under 1,000 without moving
them.

### proto (three files)

| File | Moves to | What | Lines | Left |
| --- | --- | --- | --- | --- |
| `privileged.rs` (1,174) | `privileged/describe.rs` | `PrivilegedOp::describe` in an `impl` of its own, and the `describe_*` functions | 240 | ~920 |
| `rpc.rs` (1,413) | `rpc/method.rs` | the inline `pub mod method { … }`, dedented under T207e's rule | 942 | ~470 |
| `service.rs` (1,136) | `service/validate.rs` | `ServiceSpec::validate` in an `impl` of its own, and `check_program` | 265 | ~870 |

`HELPER_VERSION` stays in `privileged.rs`, where `packaging/common.sh` reads it with `sed`.

### cli (one file)

| File | Moves to | What | Lines | Left |
| --- | --- | --- | --- | --- |
| `render/service.rs` (1,251) | `render/service/tests.rs` | the inline tests | 450 | ~800 |

Afterwards the longest of these files is the daemon's `rpc/table.rs` at about 800. All 22 leave the
baseline, and no new file enters it.

**As built.** Five children have a different name from the first draft of this table. A child named
after a function in its parent (`group`, `transition`, `settle`, `credentials`) makes a doc link to
that function ambiguous, and `apply/requirements` would clash with the `crate::requirements` that
`apply.rs` imports. They are `job`, `transitions`, `outcome`, `credential_store` and
`prerequisites`. `PrivilegedResponse` and `OpOutcome` stay in `privileged.rs`: `ts-rs` names each
type's export test after the module it is defined in, so moving them would give two tests new
paths, against D1.4. `privileged.rs` is about 920 lines without that move.

## D3 — the routing table, the one change that is not a move

`call_method` in `api/rpc.rs` is 805 lines: a `tokio::spawn` that contains panics, and inside it
one `match method.as_str() { … }` with an arm per daemon method. A `match` does not span files, and
moving only the tests and the helpers would leave the file at about 990 and growing by five lines
with every method the daemon gains.

So the `match` becomes the body of a new function in `api/rpc/table.rs`:

```rust
pub(super) async fn route(api: Arc<Api>, method: &str, params: Option<Value>) -> Result<Value, Failure>
```

`call_method` keeps the spawn, the tracing span and the panic containment, and calls
`table::route(api, &method, params).await` inside the spawned task. The arms move unchanged, with the
long `use mixengine_proto::{…}` list they need. `no_params`, `arguments`, `refused`, `encode_result`
and `Failure` stay in `rpc.rs`, where `table` sees them as a child. The protocol tests stay inline,
and `rpc/arms.rs` stays where it is. Only the function signature and the one call are new code.

`table.rs` will reach 1,000 lines again after about forty more methods. Splitting it by capability
into several smaller `match`es is a refactor, and is not part of this task.

## D4 — what a move could change here, and why it does not

- **Paths outside the file.** Types re-exported (D1.3); inherent methods need no re-export. The
  `ts-rs` derives move with their types, and the exported file names come from the type names, so
  `bash packaging/bindings.sh` regenerates `bindings/` byte for byte, which is checked.
- **`scripts/check-client-surface.mjs` reads the method names from `crates/mixengine-proto/src/rpc.rs`.**
  Its `RPC` constant changes to `rpc/method.rs` in the proto commit. The script's tests give it
  source text directly, so they do not change.
- **The privileged helper.** proto and platform are in its closure, so `helper-lock.sh --check`
  sees a new fingerprint. It decides: if it asks for `--bump`, the bump is part of the proto commit.
- **Operating systems.** `windows/process.rs` and `restricted.rs` compile only on Windows.
  `process.rs` has `#[cfg(not(windows))]` items. Checked on Windows here, on Linux through WSL for
  platform, core and daemon, and for macOS with the `before-pushing` skill's
  `--target aarch64-apple-darwin` clippy for platform and elevate.
- **Order and state.** No method's call moves, only its body. No lock is held across a moved
  boundary, and no `Drop` is separated from its type.
- **Test paths.** None change (D1.4). `.config/nextest.toml` selects only
  `services::(hold|activate)::tests::`, `activation::tests::` and `services::ports::tests::`, none of
  which is here.

## D5 — commits and verification

There are five commits, one per crate in dependency order, then the roadmap tick:

1. **proto** (three files, `check-client-surface.mjs`, any helper bump).
2. **platform** (three files).
3. **core** (six files).
4. **daemon** (nine files).
5. **cli** (one file).

Each commit is checked before it is made:

- `cargo test -p <crate> --all-targets -- --list`, before and after: the sorted lists are
  identical.
- `git diff --color-moved=zebra --color-moved-ws=allow-indentation-change` leaves undimmed only `use`
  lines, `mod` lines, `pub use` re-exports, the `impl Foo {` wrappers, `pub(super)`s, the one-line
  headers, T207e's `mod tests;` lines, D3's signature and call, and whatever `rustfmt` rewraps.
- Every string literal is compared before and after, as in T207m, because `rpc/method.rs` and
  three `tests.rs` are dedented.
- `cargo fmt --check`, `cargo clippy -p <crate> --all-targets -- -D warnings`, rustdoc with
  `-D warnings`, and `node scripts/check-file-sizes.mjs --shrink`, which removes the crate's
  entries.
- For proto: `helper-lock.sh --check`, `packaging/bindings.sh` with no diff, and
  `node scripts/check-client-surface.mjs`.

Then `bash scripts/gate.sh`, the WSL runs from D4, `scripts/ask-ci.sh` for every job, and one pull
request, squash-merged.

## MixLab

No part in it. Nothing the daemon answers changes, `bindings/` regenerates unchanged, and nothing
MixLab draws changes.

## Not in scope

- Splitting `rpc/table.rs` into several smaller `match`es (D3).
- Moving tests to follow the code they assert on (D1.4).
- The ten files outside the root workspace, which are T207q.

## Is there an ADR here? No

It applies T207d's, T207i's and T207e's conventions again. Nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207p ticked, linking this spec, which
  flips to `implemented`. `docs/roadmap/todo.md` counts it.
- `docs/specs/README.md` regenerated with `node scripts/check-docs.mjs --write-index`.
