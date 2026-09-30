# 0043. Documentation for people lives under `docs/`; `.claude/` holds only agent configuration

**Status**: Accepted
**Date**: 2026-09-19

## Context

Until T169 the engineering documentation lived in `.claude/`: architecture, features, standards,
operations, decisions, roadmap and reviews, beside the agent tooling's own configuration, with a
second tree under `.claude/desktop/` carried over from the standalone client when
[ADR 0027](0027-the-desktop-client-lives-in-this-repository.md) brought the application home. Dated
designs lived in a third place, `docs/superpowers/specs/`, named after the plugin that first wrote
them, and only about ten of 137 said whether they had been built, in five different spellings.

A folder that is hidden and named after one vendor's tool is not where a contributor looks. The
indexes had drifted (the map stopped at phase 13 of 22), review ids collided between the two trees,
1,284 references in 488 files pointed into these places, nothing checked them, and 29 links were
already dead. The full account is in
[the design](../specs/2026-09-19-t169-one-home-for-the-documentation-design.md).

## Decision

- Every document written for people lives under `docs/`: `guide/` for whoever uses MixEngine;
  `architecture/`, `features/`, `specs/`, `decisions/`, `standards/`, `operations/`, `roadmap/`
  and `reviews/` for whoever builds it. Local implementation plans live in `docs/plans/`, which is
  gitignored.
- `.claude/` holds only agent configuration — `commands/`, `skills/`, settings — and a README that
  points here.
- The desktop application's documents are a `desktop/` subfolder of the folder that fits. Its
  review ids are their own namespace, cited as "desktop R3"; its four dated decisions from the standalone client
  keep their names.
- A new top-level folder is created once three documents fit nowhere else.
- Every spec opens with a `status`/`date` header. The status is one of `draft`, `approved`,
  `implemented`, `superseded` or `abandoned`, and the commit that ticks a task flips its spec to
  `implemented`.
- `node scripts/check-docs.mjs`, in CI's `lint` job, fails on a dead link into the documentation,
  on a spec header that is missing, invalid or contradicts the roadmap, on a stale
  `docs/specs/README.md`, and on documentation under `.claude/` or `docs/superpowers/`.

## Consequences

- A contributor who does not use the agent tooling finds the documentation where they look first.
- Whether a design was built is read from one header, and the check keeps it agreeing with the
  roadmap.
- The tooling's default paths are overridden in `CLAUDE.md`, and a file written to the default
  anyway fails the check.
- Links from outside the repository into `.claude/…` break; `.claude/README.md` stays as a pointer.
- A later move is a script and a green check. `git log --follow` still reaches every file's history.

## Alternatives considered

- **Leave the files where they are and fix the indexes.** Stale indexes were the symptom; without
  a check they drift again, and the location stays hidden and vendor-named.
- **Move, but keep `docs/superpowers/specs/`.** The name belongs to a plugin, not to the project.
- **Adopt a reference layout's names: `product/`, `planning/`, `brand/`, `modules/`.** "Feature"
  and "roadmap" are vocabulary in more than a thousand comments, not only paths; `brand/` and
  `modules/` would hold one or two files each.
- **Keep statuses as prose `**Status:**` lines.** That is what existed, in five spellings, and no
  tool could read or check it.
