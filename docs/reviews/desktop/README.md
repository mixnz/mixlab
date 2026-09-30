# Technical review notes

**These ids are their own namespace.** This folder numbers its findings from `R1`, as the standalone client did,
independently of [the workspace's reviews](../README.md). Cite one as "desktop R3"; a bare `R3`
means the workspace's.

Every full review of the repository is one `YYYY-MM-DD.md` file in this folder. The folder records
**the state of the code at a point in time** — which nothing else here does: a spec in
[docs/specs/](../../specs/) says *what to build*, the
[CHANGELOG](../../../apps/desktop/CHANGELOG.md) says *what shipped*, this says *how good what was built turned out
to be*.

The scope is purely technical, on both sides of the `invoke` boundary: correctness, safety,
testing, structure, style — in `src/` and in `src-tauri/src/` alike. Anything that belongs to
releasing or to end users (installers, the updater, the public README, market) is **not recorded
here** — it has its own place in [build-and-release.md](../../operations/build-and-release.md) and the root
[README](../../README.md).

## Conventions

- Every finding carries an id `R<n>` that is **global and never reused**: a later review citing
  `R7` means that finding, whether or not it has been fixed since. Numbering continues from the
  last id of the previous review.
- Three groups, in this order: **Bugs** (wrong against what the code or its documentation claims),
  **Improvements** (correct, but expensive, fragile, or off the standard the rest of the project
  keeps), **Done well** (what to preserve when refactoring).
- Status: `[ ]` open · `[x]` fixed (name the commit/PR) · `[-]` deliberately not fixed (say why).
- A later review **updates the status in the older file first**, then writes a new file holding
  only *new* or *recurring* findings; open findings are not copied forward.
- Every finding cites `file:line` as of the review; line numbers drift, the function or constant
  named beside them is what to search for.
- A finding that turns into work worth planning gets a spec or a decision, not a longer entry
  here — see [plans-and-specs](../../standards/plans-and-specs.md). A finding that is a lasting
  rule becomes a convention. The review only points at it.
- Reviews are not changelog material: fixing a finding follows the usual
  [changelog](../../standards/desktop/changelog.md) rule — a `Fixed` line only if a released version had
  the bug.

## Reviews

| Date | Scope | Open / total |
| --- | --- | --- |
| [2026-08-27](2026-08-27.md) | The whole repository at `24be307` (v0.0.18), 214 commits | 39 / 39 |
