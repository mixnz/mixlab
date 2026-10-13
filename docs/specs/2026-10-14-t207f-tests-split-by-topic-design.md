---
status: draft
date: 2026-10-14
task: T207f
---

# Tests past the limit on their own, split by topic at the boundaries they already have (design)

Three files in `scripts/file-size-baseline.json` have tests that would not fit under 1,500 lines
even in a file of their own, so [T207e](2026-10-14-t207e-test-modules-to-their-own-file-design.md)'s
move is not enough: core `blueprints/plan.rs` (code 1,152, tests 2,437), core
`generate/recipes/nginx.rs` (code 875, tests 1,597), and the daemon's integration suite
`tests/runtimes.rs` (2,224, all of it tests). This design splits their tests by topic, and the
topics are the clusters the tests already sit in.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207f. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 holds: a commit only moves.

## D1 — the shape

**A unit test module** takes T207e's form and goes one level further:

```text
plan.rs                 #[cfg(test)] mod tests;              (as in T207e)
plan/tests.rs           the hub: the module's `use` lines, the helpers several topics use,
                        and `mod resolving; mod scaffold; mod names;`
plan/tests/scaffold.rs  //! one line saying what it holds
                        use super::*;
                        the tests of that topic, and the helpers only they use
```

**An integration test** becomes a directory Cargo already understands: `tests/runtimes.rs` →
`tests/runtimes/main.rs` (the hub) beside `tests/runtimes/<topic>.rs`. Cargo names the test target
after the directory, so it is still `--test runtimes`, one binary, built and run as before.

A topic module sees every private item of the hub and of the file above it, so nothing changes
visibility. `docs/standards/rust.md`'s T207e paragraph gains one sentence: tests that would not fit
in one file of their own split by topic under the hub.

## D2 — where the cuts go: the boundaries the tests already have

**Nothing is reordered.** Each topic is a contiguous run of the original lines, from a test's first
attribute or doc line to the line before the next topic, so `git diff --color-moved` shows each as
one block. The cut points are where a file's tests already change subject:

| File | Hub | Topic, from the test named | Lines | Tests |
| --- | --- | --- | --- | --- |
| `plan.rs` | 222 | `resolving`, from `a_name_is_planned_the_way_it_will_be_stored`: the project, runtimes, packages, extensions, sites and pins a plan finds done or to do | 1,137 | 26 |
| | | `scaffold`, from `the_project_name_reaches_the_scaffold_command`: the command or archive, its directory, npm's names, `.env` lines | 723 | 21 |
| | | `names`, from `only_the_steps_that_need_a_password_say_so`: slugs and tokens, service ids, database names, domains | 349 | 8 |
| `nginx.rs` | 66 | `sites`, from `the_package_supplies_the_fastcgi_parameters_a_php_site_needs`: server blocks, routes, welcome page, pools, authority, sharing, TLS, redirects | 1,284 | 42 |
| | | `server`, from the `context` helper on: the nginx process's spec and `nginx.conf` | 243 | 12 |
| `tests/runtimes.rs` | 526 | `install`, from `a_version_the_index_offers_is_installed_listed_chosen_and_removed`: install, list, choose, remove, pins, pools, extensions, markers, `bin/` | 1,062 | 24 |
| | | `upgrade`, from `two_patches`: the catalogue's newest per line, upgrade plans and upgrades | 536 | 10 |
| | | `listing`, from `index_with_a_node`: a list that cannot read one kind, a deleted runtime folder | 98 | 3 |

Lengths before each file's two-line header (`//!`, `use super::*;`) and before `rustfmt`; the
hub's include its `mod` lines. 55, 54 and 37 tests, the counts each file has today. `plan.rs` and
`nginx.rs` keep their code and end at 1,153 and 876 lines; all three leave the baseline.

**A helper two topics use moves to the hub**, which is where T207d put `row` and `gone`. Found by
listing every item each topic defines and searching the others' code for it:

- `plan.rs`: `planned_for` (in `scaffold`, used by `resolving` too).
- `nginx.rs`: `root` and `nginx_binary` (in `sites`, used by `server`), `context` and `context_on`
  (in `server`, used by `sites`).

They go to the end of the hub, in their original order. Everything else a topic defines, only that
topic uses.

## D3 — what a move could change here

- **Test names gain a segment** — `blueprints::plan::tests::a_dotenv_key_…` becomes
  `blueprints::plan::tests::scaffold::a_dotenv_key_…`, and `runtimes`' tests gain `install::` and
  the like. Nothing selects these by path: `.config/nextest.toml` names
  `services::(hold|activate)::tests::` and `services::ports::tests::`, and no workflow or script
  names `plan::tests`, `nginx::tests` or a `runtimes` test. The per-crate counts are unchanged.
- **The integration target.** `crates/mixengine-daemon/Cargo.toml` declares no `[[test]]`, so
  Cargo discovers `tests/runtimes/main.rs` as `runtimes` exactly as it discovered `runtimes.rs`;
  its other `.rs` files are modules of that binary, not targets.
- **Indentation and strings**, as T207e: a unit module's body is dedented except lines that begin
  inside a string literal, and every literal is compared before and after. The integration file
  is not indented and moves as it is.
- **The helper lock.** None of the three is in a crate the privileged helper compiles from
  (`helper.lock`'s list), so `HELPER_VERSION` does not move.
- **Operating systems.** `runtimes`' tests spawn a real daemon; they run on Windows here and on
  Linux through WSL, with `cargo build --workspace --bins` first for the shim (T207d's lesson).

## D4 — commits and verification

One commit per file — `plan.rs`, `nginx.rs`, `runtimes.rs` — then the roadmap tick. Each commit:
the counts unchanged by `--list` (`mixengine-core --lib` 1,194; `mixengine-daemon --test runtimes`
37); every string literal equal; `git diff --color-moved` leaving only the hubs' `mod` lines, the
headers and `use super::*;`, the five lifted helpers, and `rustfmt`'s re-wraps; clippy and rustdoc
with `-D warnings`; `check-file-sizes --shrink`. Then the gate, WSL, `ask-ci`, one pull request,
squash-merged.

## MixLab

No part in it. Where tests sit changes nothing the daemon answers, nothing in `mixengine-proto` or
`bindings/`, nothing MixLab draws.

## Not in scope

- Reordering tests into cleaner topics than the file already has. A test sitting in a neighbour's
  cluster stays there; moving it would turn a block move into a reorder a reviewer has to check.
- The remaining twelve baseline files (T207g–T207j).

## Is there an ADR here? No

It extends T207e's convention in [docs/standards/rust.md](../standards/rust.md); nothing
cross-cutting is decided.

## Documentation, when it lands

- `docs/standards/rust.md`: D1's sentence.
- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207f ticked, linking this spec, which
  flips to `implemented`; `docs/roadmap/todo.md` counts it.
