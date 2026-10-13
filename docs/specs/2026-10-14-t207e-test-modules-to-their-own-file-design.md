---
status: implemented
date: 2026-10-14
task: T207e
---

# Test modules to their own file, where only the tests push a file past the limit (design)

Fifteen of the thirty files left in `scripts/file-size-baseline.json` after T207d have code under
1,500 lines and a test module that pushes them over it. For them there is nothing to design per
file: the module moves whole to `foo/tests.rs`, and `foo.rs` keeps `mod tests;`. This design states
that convention, which was agreed on 2026-10-14 and is new to this repository, says what it does to
the one check that reads where a test module is, and lists the fifteen.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207e. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 holds: a commit only moves.

## D1 — the convention

```rust
// foo.rs, at the end, where the module was
#[cfg(test)]
mod tests;
```

and `foo/tests.rs` holds what was between `mod tests {` and its closing `}`. Whatever sat above
`mod tests {` — `#[cfg(test)]`, and a doc comment or `#[allow]` where there is one — stays above
`mod tests;` in `foo.rs`. None of the fifteen has anything but `#[cfg(test)]` there.

**Nothing a test is called changes.** The module is still `foo::tests`, so every test keeps its
path (`generate::tests::a_rendering_…`), `use super::*;` still names `foo`, nextest's filters in
`.config/nextest.toml` select what they selected, and the unit-test counts per crate are unchanged
by construction.

**The body is dedented by four spaces, except where that would change a value.** A line that
begins inside a string literal — the second line of a multi-line `"…"` or `r#"…"#`, a TOML or
Caddyfile fixture written inline — keeps its indentation, because those spaces are part of the
string. The move script finds them with a small lexer (strings, raw strings, escapes, char
literals, block comments) and counted them on `master` at `098db820`: 221 such lines across
eleven of the files, none in `postgres.rs`, `mariadb.rs` or the daemon's `sites.rs` and
`uninstall/inventory.rs`. `rustfmt` then re-wraps the lines that four columns less now let
fit in 100 — string contents untouched — and that re-wrap is part of the move, read line by line.

When to use it, for `docs/standards/rust.md`: **a file whose code fits under the limit and whose
tests do not** moves its test module to `foo/tests.rs` rather than splitting code that has no reason
to split. A file whose *code* is past the limit is split by capability first, as T207a–d were, and
its tests go with what they test.

## D2 — the fifteen, and their lengths after

Measured by the move script's dry run on `master` at `098db820` (`foo.rs` counts `mod tests;`):

| File | Now | `foo.rs` after | `foo/tests.rs` |
| --- | --- | --- | --- |
| core `generate.rs` | 2,336 | 1,181 | 1,154 |
| core `services.rs` | 2,294 | 1,267 | 1,026 |
| core `sites.rs` | 2,671 | 1,389 | 1,281 |
| core `manifest.rs` | 1,970 | 919 | 1,050 |
| core `elevation.rs` | 1,522 | 756 | 765 |
| core `blueprints/manifest.rs` | 1,750 | 984 | 765 |
| core `generate/recipes/caddy.rs` | 2,216 | 787 | 1,428 |
| core `generate/recipes/postgres.rs` | 1,897 | 894 | 1,002 |
| core `generate/recipes/mariadb.rs` | 1,784 | 804 | 979 |
| core `generate/recipes/mysql.rs` | 1,825 | 980 | 844 |
| proto `privileged.rs` | 2,049 | 1,173 | 875 |
| daemon `elevation.rs` | 2,825 | 1,475 | 1,349 |
| daemon `sites.rs` | 1,575 | 1,402 | 172 |
| daemon `uninstall/inventory.rs` | 1,745 | 1,217 | 527 |
| daemon `api/apply.rs` | 1,729 | 1,293 | 435 |

All fifteen leave the baseline; no new file enters it. Two leave with little room:
daemon `elevation.rs` at 1,475 and `sites.rs` at 1,402. The next change that grows either one is
the moment to split it by capability, which the ratchet will say.

Where `foo/` already exists it gains a file beside its children (`generate/`, `services/`,
`api/apply/`, `sites/`, and the recipe directories, which hold templates read by `include_str!`
from the *code*, so no path moves). Where it does not, it is created.

## D3 — the one check that reads where a test module is

`crates/mixengine-proto/tests/workspace_layering.rs`'s
`no_crate_but_platform_compiles_a_line_away_by_operating_system` lets five files hold an OS
`#[cfg]` because it is test code, and proves that by line: a permitted file must have exactly one
`#[cfg(test)]`, and every OS `cfg` must come after it. Two of the five are in this task —
`generate/recipes/postgres.rs` and `uninstall/inventory.rs` — and after the move their OS `cfg`s
are in `postgres/tests.rs` and `inventory/tests.rs`, files with no `#[cfg(test)]` line at all,
because that attribute is on `mod tests;` in the parent.

The check learns the convention rather than losing its guarantee:

- `PERMITTED` names the two `tests.rs` files instead of their parents, with the same reasons.
- A permitted path ending in `/tests.rs` is test code from its first line **when its parent file
  (`postgres.rs` beside `postgres/`) has a line `mod tests;` directly under a line
  `#[cfg(test)]`**. The check asserts exactly that pair in the parent, and asserts the parent itself
  has no OS `cfg`, so the permission still cannot widen into the product.
- Every other permitted file keeps today's rule.

This is the one change to code in the task. It lands in the commit that moves the first of the two
files, so that commit is green on its own; `docs/standards/rust.md`'s sentence "a listed file has
to keep the attribute inside its test module" gains "or in a `tests.rs` its parent declares under
`#[cfg(test)]`".

## D4 — commits and verification

One commit per crate, because every file in a crate is the same move and the review is the same
read:

1. **core** (ten files) — with D3's change to `workspace_layering.rs` and the convention in
   `docs/standards/rust.md`.
2. **daemon** (four files) — `PERMITTED`'s `inventory.rs` entry renamed.
3. **proto** (`privileged.rs`) — last, because it is the one file of the fifteen built into the
   privileged helper. `packaging/helper-lock.sh` fingerprints the *contents* of every file the
   helper compiles, so taking the test module out of `privileged.rs` changes the fingerprint
   though nothing in the helper's behaviour does, and the pre-commit hook asks for a bump:
   `HELPER_VERSION` 0.1.7 → 0.1.8, agreed on 2026-10-14. The lock compares against the last
   release, so later tasks that touch the helper before the next one (T207h) need no second
   bump. None of T207a–d touched a file the helper compiles.

Each commit is checked before it is made:

- **Unit-test counts unchanged**, by `--list`, recorded on `master` at `098db820`:
  `mixengine-core --lib` **1,194**, `mixengine-proto --lib` **379**, `mixengine-daemon --bins`
  **705**; and the moved tests run (`cargo test -p <crate> --lib`/`--bins`).
- **Only indentation moved**:
  `git diff --color-moved=zebra --color-moved-ws=allow-indentation-change` leaves undimmed only `mod tests;`, the `{`/`}` that became it, and D3's lines.
- `cargo fmt --check`, clippy and rustdoc with `-D warnings`, `workspace_layering`, then
  `node scripts/check-file-sizes.mjs --shrink`.

Then `bash scripts/gate.sh`, the daemon's and core's suites on Linux through WSL (two of the files
hold `#[cfg(unix)]` tests that a Windows run skips), `scripts/ask-ci.sh`, and one pull request,
squash-merged.

## MixLab

No part in it. Where tests sit in the source changes nothing the daemon answers, nothing in
`mixengine-proto`'s types or `bindings/`, and nothing MixLab draws; `privileged.rs`'s types and
their `ts` exports are in the code half, which does not move.

## Not in scope

- The other fifteen files in the baseline (T207f–T207j).
- Moving the test module of any file *not* past the limit. The convention is for files the ratchet
  names; the rest keep their tests inline until a reason arrives.

## Is there an ADR here? No

It is a layout convention under the standard in [docs/standards/rust.md](../standards/rust.md),
written there; nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/standards/rust.md`: D1's when-to-use paragraph beside the 1,500-line rule, and D3's
  sentence.
- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207e ticked, linking this spec, which
  flips to `implemented`; `docs/roadmap/todo.md` counts it.
