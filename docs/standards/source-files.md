# Source files

A source file is short enough to read in one go. This is the one place the rule is written down;
everything else links here.

## The limit

**No source file over 1,000 lines** — `.rs`, `.ts`, `.tsx`, `.js`, `.mjs` and `.css` alike, in every
workspace of the repository. Not counted: what is generated (`bindings/`, `.sqlx/`), what is never
edited once shipped (migrations), data written as code (`fixtures/`), and `.d.ts` files.

`node scripts/check-file-sizes.mjs` is the check. `.githooks/pre-commit`, `scripts/gate.sh` and
CI's `lint` job run it. Design: [the spec](../specs/2026-10-13-files-that-fit-in-one-read-design.md).

## Splitting before, not after

An edit that would take a file past the limit splits it first and then makes the change, in the
same task; it does not land and leave the split for later. How a file splits depends on what it is:

- **Rust code** splits by capability, never by layer, as [rust.md](rust.md#files-and-modules)
  says.
- **Tests** move out of the file they test without changing their path, as
  [testing.md](testing.md#where-a-test-file-goes) says. A file whose *code* is past the limit
  splits by capability, and its tests go with what they test.
- **A React component** becomes hooks and parts, as
  [component-structure.md](desktop/component-structure.md) says.

## The baseline

`scripts/file-size-baseline.json` names the files that were already past the limit when the check
landed, each at its length then, and they may not grow. Making one shorter means
`node scripts/check-file-sizes.mjs --shrink` in the same commit, which lowers its entry, and a file
back under the limit leaves the list.

Nothing raises an entry. A file that has to grow is a hand edit to the JSON, argued in review — the
usual answer is a new module. A file new to the limit is never added to make room.
