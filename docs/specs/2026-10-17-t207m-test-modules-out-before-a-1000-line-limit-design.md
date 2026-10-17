---
status: approved
date: 2026-10-17
task: T207m
---

# Test modules out to their own file, ahead of a 1,000-line limit (design)

Lowering `scripts/file-size-baseline.json`'s limit from 1,500 to 1,000 would put 76 files in the
baseline on `master` at `6f6c2a15`. In 28 of them the code is under 1,000 lines, and only the inline
test module pushes the file over. This task moves those test modules out under T207e's convention.
After that, the baseline a 1,000-line limit would need drops to about 48 files, each of which has to
be split by capability. Changing the limit itself is not part of this task.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207m. The convention
is [T207e's](2026-10-14-t207e-test-modules-to-their-own-file-design.md) D1, unchanged, and the
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 still applies: a commit only
moves code.

## D1 — the convention, and the one thing T207e did not meet

`foo.rs` keeps `#[cfg(test)] mod tests;` where the module was, and `foo/tests.rs` holds the body,
dedented by four spaces. The exception is a line that begins inside a string literal, which keeps
its indentation. Test paths stay `foo::tests::x`. Under `mod.rs` and `main.rs` the file is
`tests.rs` beside them. No target file exists today.

T207e's fifteen files each had one test module. **Five of the 28 have a second one**: `mssql/ddl.rs`
(`index_tests`), `ssh/mod.rs` (`expand_home_tests`), `generate/recipe.rs` (`client_tests`),
`runtimes/extensions.rs` (`trust_tests`) and `shims.rs` (`claim_tests`). Each module goes to its own
file named after it, such as `shims/claim_tests.rs`. Merging them into `tests.rs` would rename every
test in them. Only `shims.rs` actually needs it: with `claim_tests` left inline, the file is 1,005
lines. The other four move too, so that a file does not keep half its tests inline.

Two files keep code after the move. In `proto/service_api.rs` the test module sits in the middle,
so `mod tests;` takes its place and the 266 lines after it stay. In `ssh/mod.rs`,
`expand_home_tests` sits before `tests`. Neither changes the rule.

## D2 — the 28, and their lengths after

These are from a dry run on `6f6c2a15`. A lexer finds each module's closing brace, skipping
comments, strings, raw strings and char literals. A regex is not enough: `generate/document.rs` has
raw strings with a `}` in column 0. "After" counts `mod …;` in place of the module. "Literal" counts
the lines that keep their indentation. `rustfmt` may rewrap a few lines afterwards.

| File | Now | After | Moved out (literal lines) |
| --- | --- | --- | --- |
| desktop `db/drivers/clickhouse/ddl.rs` | 1,027 | 653 | `tests` 373 |
| desktop `db/drivers/mongo.rs` | 1,246 | **1,000** | `tests` 245 |
| desktop `db/drivers/mssql/ddl.rs` | 1,095 | 911 | `tests` 103 (4), `index_tests` 79 |
| desktop `db/drivers/tools.rs` | 1,081 | 813 | `tests` 267 |
| desktop `secrets.rs` | 1,014 | 516 | `tests` 497 |
| desktop `ssh/mod.rs` | 1,065 | 855 | `expand_home_tests` 26, `tests` 182 |
| desktop `sync/engine.rs` | 1,007 | 312 | `tests` 694 |
| core `extensions/render.rs` | 1,307 | 804 | `tests` 502 |
| core `generate/document.rs` | 1,256 | 706 | `tests` 549 (8) |
| core `generate/recipe.rs` | 1,290 | 814 | `tests` 294, `client_tests` 180 |
| core `generate/recipes/php_fpm.rs` | 1,198 | 593 | `tests` 604 (1) |
| core `generate/served.rs` | 1,201 | 627 | `tests` 573 (20) |
| core `index/format.rs` | 1,142 | 716 | `tests` 425 (20) |
| core `install.rs` | 1,160 | **991** | `tests` 168 |
| core `projects.rs` | 1,250 | 726 | `tests` 523 (5) |
| core `runtimes.rs` | 1,225 | 690 | `tests` 534 (4) |
| core `runtimes/extensions.rs` | 1,278 | 693 | `tests` 517 (10), `trust_tests` 66 |
| core `services/graph.rs` | 1,037 | 615 | `tests` 421 |
| core `shims.rs` | 1,108 | 878 | `tests` 102, `claim_tests` 126 |
| daemon `api/service.rs` | 1,082 | 560 | `tests` 521 |
| daemon `certs.rs` | 1,227 | 756 | `tests` 470 (1) |
| daemon `doctor.rs` | 1,110 | 834 | `tests` 275 |
| daemon `error.rs` | 1,387 | **973** | `tests` 413 |
| daemon `services/start.rs` | 1,184 | 517 | `tests` 666 |
| proto `blueprint.rs` | 1,089 | 732 | `tests` 356 |
| proto `cert_api.rs` | 1,179 | 684 | `tests` 494 |
| proto `service_api.rs` | 1,339 | 871 | `tests` 467 (8) |
| shim `main.rs` | 1,197 | **955** | `tests` 241 |

The four files in bold are under 1,000 with less than 50 lines to spare. Under a 1,000-line limit,
the next change to any of them would have to split it by capability. Fitting them now would be a
split, not a move, so it is not in this task.

## D3 — what the move touches beyond the files

- **`workspace_layering.rs`**: none of the 28 holds an OS `#[cfg]`. Otherwise the check would
  already fail, since none is in `PERMITTED`. `PERMITTED` stays as it is. The desktop workspace is
  not scanned.
- **The privileged helper**: `proto/blueprint.rs`, `cert_api.rs` and `service_api.rs` are among the
  files `crates/mixengine-elevate/helper.lock` fingerprints. The lock already says 0.1.8 against
  the 0.1.7 release, so under T207e's D4 no second bump is due. `bash packaging/helper-lock.sh
  --check` will confirm it on the proto commit.
- **`docs/standards/rust.md`** gains one sentence: a module with a second test module moves each of
  them to a file named after it.

## D4 — commits and verification

There is one commit per workspace or crate. Each is the same move, so each reads the same in review:

1. **core** (twelve files), with the sentence for `docs/standards/rust.md`.
2. **daemon** (five files).
3. **proto** (three files), with `helper-lock.sh --check`.
4. **shim** (`main.rs`).
5. **desktop** (seven files), in `apps/desktop/src-tauri`'s own workspace.

Each commit is checked before it is made:

- **Test lists unchanged.** `cargo test -p <crate> --lib`/`--bins -- --list` (and in
  `apps/desktop/src-tauri`, `cargo test -- --list`) gives the same sorted list before and after.
  The moved tests then run.
- **Only indentation moved.** `git diff --color-moved=zebra --color-moved-ws=allow-indentation-change`
  leaves undimmed only the `mod …;` lines and the `{`/`}` that became them.
- `cargo fmt --check`, clippy and rustdoc with `-D warnings`, `workspace_layering`, then
  `node scripts/check-file-sizes.mjs` (the baseline is empty, and stays so).

Then `bash scripts/gate.sh`, the core and daemon suites on Linux through WSL, `scripts/ask-ci.sh`,
and one pull request, squash-merged.

## MixLab

The window has no part in this. Where tests sit in the source changes nothing the daemon answers,
nothing in `mixengine-proto`'s types or `bindings/`, and nothing MixLab draws. The proto types and
their `ts` exports are in the code half, which does not move. The seven desktop files are MixLab's
own Rust, and their commands keep their names and bodies.

## Not in scope

- **Lowering the limit to 1,000.** That change also needs a baseline of about 48 files (76 minus
  these 28), written once, since `--shrink` never adds an entry. It gets a design of its own, and
  that design says whether the 1,000 applies to `.ts`, `.tsx` and `.css` too.
- The 27 files whose code alone is past 1,000, and the 16 that are already test files.
- Fitting the four files in bold in D2.

## Is there an ADR here? No

This applies the existing convention in [docs/standards/rust.md](../standards/rust.md) to more files.
Nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/standards/rust.md`: D3's sentence.
- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207m ticked, linking this spec, which
  flips to `implemented`. `docs/roadmap/todo.md` counts it.
