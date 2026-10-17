---
status: approved
date: 2026-10-17
task: T207o
---

# Test files past 1,000 lines, split by topic at the boundaries they already have (design)

After T207n lowered the limit to 1,000, sixteen of the 48 files in
`scripts/file-size-baseline.json` are test files: unit-test hubs and topics under T207e's and
T207f's conventions, three integration suites per crate, and one shared test harness. None of them
has code to split by capability, so this task applies T207f's move again: every file is cut at
places where its tests already change subject, and no test is reordered.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207o. The convention is
[T207f's](2026-10-14-t207f-tests-split-by-topic-design.md) D1 and D2, and the
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 still applies: a commit only
moves code. Measured on `master` at `1e2c0fa5`.

## D1 — the shapes, and three cases T207f did not meet

As in T207f, a unit-test hub `foo/tests.rs` holds the `use` lines, the helpers that more than one
topic uses, and `mod <topic>;`. Each topic `foo/tests/<topic>.rs` starts with a one-line `//!` and
`use super::*;`. An integration suite `tests/<name>.rs` becomes `tests/<name>/main.rs` beside its
topics, and stays the same binary.

1. **A topic past the limit splits into siblings under the hub it already has.** `resolving.rs`
   under `blueprints/plan/tests/`, `sites.rs` under `nginx/tests/` and `install.rs` under the
   daemon's `tests/runtimes/` each keep their first part and give the rest to a new sibling. The
   hub gains one `mod` line. A test in the new sibling gains one path segment and loses none.
2. **An integration suite that uses a shared module.** `mod harness;` in `tests/mariadb.rs` names
   `tests/harness/mod.rs`, but in `tests/mariadb/main.rs` it would name `tests/mariadb/harness.rs`.
   The hubs of `mariadb` and `uninstall` therefore declare
   `#[path = "../harness/mod.rs"] mod harness;`. That is the one line in this task that is not
   moved code, and `docs/standards/rust.md` gains a sentence saying so.
3. **The shared harness is not a test module.** `tests/harness/frontend.rs` (1,205 lines) holds
   two tests and helpers that seventeen cli suites call as `harness::frontend::<fn>`. Its first 464
   lines (the `FrontEnd` handle, ports, requests and both tests) stay where they are. Lines 465 to
   the end (the nginx and Caddy scenarios and the helpers only they use) move to
   `harness/frontend/scenarios.rs`, and `frontend.rs` re-exports them with
   `pub(crate) use scenarios::*;`. No caller changes, and no test changes its path. Not every
   suite calls every scenario, so the re-export carries `#[allow(unused_imports)]` beside the
   `#![allow(dead_code)]` that `harness/mod.rs` already has for the same reason.

`uninstall.rs` also holds an inline `mod machine { … }` of 378 lines. It moves to
`tests/uninstall/machine.rs` under T207e's rule: dedented, except lines that begin inside a string
literal, with its doc comment staying above `mod machine;`.

## D2 — where the cuts go

Each topic is a contiguous run of the original lines, starting at the attributes and doc lines of
the test named. Lengths are measured before the two-line header and before `rustfmt`. The hub's
length does not count its `mod` lines or the helpers lifted to it.

| File | Hub | Topic, from the test named | Lines | Tests |
| --- | --- | --- | --- | --- |
| core `blueprints/plan/tests/resolving.rs` (1,142) | `plan/tests.rs` | `resolving`, kept: the project, runtimes, a blocked domain, step order, several sites | 763 | 17 |
| | | `installed`, from `a_package_this_home_does_not_have_is_planned_as_an_install`: packages, the project this apply made, pins and the answers | 379 | 9 |
| core `generate/recipes/caddy/tests.rs` (1,425) | 12 | `sites`, from `the_global_block_carries_the_port_…`: kinds, sockets, authority, sharing, fragments, welcome | 687 | 23 |
| | | `routing`, from the `a_site_with_a_certificate` helper: pools that wake, routes, HTTPS and redirects | 432 | 15 |
| | | `server`, from the `context` helper: the process's spec, admin endpoint and `Caddyfile` | 294 | 11 |
| core `generate/recipes/nginx/tests/sites.rs` (1,287) | `nginx/tests.rs` | `sites`, kept: server blocks, welcome page, PHP handling, routes | 604 | 19 |
| | | `listening`, from the `a_php_site` helper: pools, authority, LAN sharing, TLS | 683 | 23 |
| core `generate/recipes/postgres/tests.rs` (1,008) | 37 | `server`, from `the_spec_declares_the_port_the_cluster_will_bind`: configuration, sockets, readiness, stop and reload | 492 | 22 |
| | | `bootstrap`, from the `initialised` helper: the credential, the ritual, reset and provisioning | 479 | 18 |
| core `generate/tests.rs` (1,154) | 284 | `front_end`, from `a_front_end_renders_the_sites_…`: sites, fragments, addresses | 279 | 5 |
| | | `services`, from `a_service_belonging_to_an_extension_renders_its_manifest`: idle, rows, settings, overrides, drift | 431 | 14 |
| | | `certificates`, from the `fakeservice` helper: recipes that want a certificate | 160 | 7 |
| core `manifest/tests.rs` (1,051) | 37 | `reading`, from `routes_survive_an_export_and_come_back` | 395 | 13 |
| | | `writing`, from `writing_a_manifest_keeps_the_comments_…`: the file's forms, entries found by alias | 395 | 16 |
| | | `choosing`, from the `declaring` helper: which entry a domain picks | 224 | 12 |
| core `services/tests.rs` (1,030) | 64 | `state`, from `idle_minutes_round_trips_through_all_three_states`: idle, transitions, failures, stops, the process | 446 | 17 |
| | | `declarations`, from `a_row_reads_back_as_the_declaration_that_wrote_it`: rows, ports, data directories, versions, parents | 520 | 14 |
| core `sites/tests.rs` (1,289) | 28 | `pools`, from `routes_round_trip_and_replace_as_a_whole`: routes, pools, services, links | 467 | 9 |
| | | `sharing`, from `sharing_is_written_and_taken_away_as_one_value`: sharing, creation, HTTPS redirects | 422 | 10 |
| | | `domains`, from `a_domain_moves_between_sites_…`: domains, payloads, shared names, extensions' sites | 372 | 12 |
| core `tests/index.rs` (1,191) | 95 | `signed`, from `a_signed_index_is_fetched_verified_and_read` | 396 | 9 |
| | | `schema_2`, from the `// --- Index schema 2` comment the file already has | 467 | 17 |
| | | `schema_1`, from the `schema_1_home` helper: reading through schema 1, limits | 233 | 8 |
| daemon `elevation/tests.rs` (1,348) | 0 | `queue`, from the first test: batches, enqueue, drop, grants | 469 | 14 |
| | | `asking`, from `a_machine_that_already_agrees_is_not_asked_for_permission`: what each producer asks for | 593 | 20 |
| | | `withdrawal`, from `a_hosts_entry_nobody_declares_any_more_leaves_the_queue` | 286 | 8 |
| daemon `tests/api.rs` (1,072) | 269 | `protocol`, from `health_is_answerable_…`: HTTP, batches, errors, routes, the event stream | 350 | 15 |
| | | `home`, from `a_repair_answers_a_record_of_what_it_did`: repair, bundles, doctor, the authority, blueprints | 453 | 9 |
| daemon `tests/packages.rs` (1,275) | 314 | `services`, from `a_service_package_is_offered_installed_listed_and_removed` | 467 | 13 |
| | | `markers`, from `refresh_bypasses_a_fresh_cache`: markers, adoption, found instances | 223 | 6 |
| | | `upgrades`, from the `two_patches` helper | 271 | 6 |
| daemon `tests/runtimes/install.rs` (1,067) | `runtimes/main.rs` | `install`, kept | 703 | 13 |
| | | `markers`, from `refresh_bypasses_a_fresh_cache`: markers, adoption, restore, `bin/` | 364 | 11 |
| cli `tests/harness/frontend.rs` (1,205) | 464 (D1.3) | `scenarios`, from `const NGINX` | 741 | 0 |
| cli `tests/mariadb.rs` (1,235) | 490 | `first_run`, from `a_database_is_bootstrapped_…` | 451 | 2 |
| | | `kept`, from the `set_the_root_password_behind_mixengines_back` helper | 294 | 3 |
| cli `tests/uninstall.rs` (1,147) | 17 | `plan`, from `a_dry_run_names_every_row_and_changes_nothing` | 368 | 14 |
| | | `removal`, from `keeping_the_home_leaves_the_home_and_ends_the_daemon` | 300 | 5 |
| | | `machine`, the inline module (D1) | 378 | 0 |
| | | `kept`, from `a_kept_home_keeps_its_passwords` | 83 | 2 |

The longest file afterwards is `resolving.rs` at about 765 lines. All sixteen leave the baseline,
and no new file enters it.

**A helper that two topics use moves to the hub**, at its end and in its original order. These were
found by searching each topic's code, with comments and strings blanked out, for every item another
topic defines:

- `plan/tests.rs`: `a_project`.
- `caddy/tests.rs`: `doc_root`, `a_shared_site`, `a_site_with_a_certificate`, `render_site`,
  `context`, `root`.
- `nginx/tests.rs`: `doc_root`, `a_php_site`, `a_static_site`, `render_site`.
- `postgres/tests.rs`: `context`, `with_provides`, `built`.
- `generate/tests.rs`: `home_of`, `home`.
- `manifest/tests.rs`: `pins`. `services/tests.rs`: `store`.
- `elevation/tests.rs`: `registry`, `registry_resolving`, `with_an_installed_helper`, `a_site_named`.
- `uninstall/main.rs`: `needs_an_administrator`.

Every other item a topic defines is used only by that topic.

## D3 — what a move could change here

- **Test names gain one segment**, such as `generate::tests::services::a_row_becomes_…` or
  `api::protocol::health_is_answerable_…` in the `api` binary. No test loses a segment, and the
  harness's two tests keep their paths. `.config/nextest.toml` selects only
  `services::(hold|activate)::tests::`, `activation::tests::` and `services::ports::tests::`,
  none of which is here. The workflows name these suites by binary (`--test mariadb`,
  `--test uninstall`, and `--no-run --message-format=json` in `_system.yml` for the executable),
  and those names do not change.
- **The binaries.** No crate declares `[[test]]`, so Cargo finds `tests/<name>/main.rs` as
  `<name>`, as it did for T207f's `runtimes`.
- **Ignored suites.** `mariadb` (6 `#[ignore]`), `uninstall` (2) and `index` (2) run in full only
  in CI's `services` and `system` jobs. Here they are compiled, listed and run as far as a plain
  run goes, and the CI run asked for before the pull request runs every job.
- **Strings.** Only `machine` is dedented. Every other topic moves at the indentation it already
  has. Every string literal is compared before and after in all sixteen files, as in T207m.
- **The helper lock.** None of these files is compiled into the privileged helper.

## D4 — commits and verification

There are three commits, one per crate, then the roadmap tick:

1. **core** (nine files), with the sentence for `docs/standards/rust.md`.
2. **daemon** (four files).
3. **cli** (three files).

Each commit is checked before it is made:

- **Every test is still there under its new path.** `cargo test -p <crate> --all-targets -- --list`
  before and after. After removing the one inserted topic segment, the sorted lists are identical,
  and each binary's count is unchanged.
- **Only moves.** `git diff --color-moved=zebra --color-moved-ws=allow-indentation-change` leaves
  undimmed only the `mod` lines, each topic's two-line header, the lifted helpers moving up, the
  `#[path]` and re-export lines from D1, and whatever `rustfmt` rewraps.
- **Strings unchanged**, by the literal comparison, and the tests pass.
- `cargo fmt --check`, `cargo clippy -p <crate> --all-targets -- -D warnings` (this catches an
  unused re-export), rustdoc with `-D warnings`, and `node scripts/check-file-sizes.mjs --shrink`,
  which removes the crate's entries.

Then `bash scripts/gate.sh`, the core and daemon suites on Linux through WSL, `scripts/ask-ci.sh`
for every job, and one pull request, squash-merged.

## MixLab

No part in it. Where tests sit changes nothing the daemon answers, nothing in `mixengine-proto` or
`bindings/`, and nothing MixLab draws.

## Not in scope

- Reordering tests into cleaner topics than a file already has. A test sitting in its neighbour's
  cluster stays there, for T207f's reason.
- Splitting `scenarios`' two long functions (`is_generated_validated_started_reloaded_and_stopped`,
  320 lines, and `serves_what_an_extension_s_fragment_adds`, 152). They are each one scenario, and
  the file they are in is 741 lines.
- The 22 code files of T207p and the ten files of T207q.

## Is there an ADR here? No

It extends T207e's and T207f's convention in [docs/standards/rust.md](../standards/rust.md);
nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/standards/rust.md`: one sentence. An integration suite moved into a directory names a
  shared module with `#[path = "../<module>/mod.rs"]`, and a topic past the limit splits into
  siblings under its hub.
- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207o ticked, linking this spec, which
  flips to `implemented`. `docs/roadmap/todo.md` counts it.
