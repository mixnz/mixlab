# Plans and specs

Two kinds of working document, treated very differently.

## `docs/specs/` — tracked, linkable, with a status

A spec is the design for one piece of work: `YYYY-MM-DD-<slug>-design.md`, with the roadmap id in
the slug when there is one (`2026-08-25-t51-web-server-tls-design.md`). It describes what the work
is and why it is shaped that way, and it outlives the branch. It is committed, and any document may
link to it — a roadmap task pointing at its design is the normal case.

Every spec opens with this header, and nothing else in it states its status:

```yaml
---
status: approved
date: 2026-09-19
task: T169
---
```

| Field | Required | Value |
| --- | --- | --- |
| `status` | yes | one of the five below |
| `date` | yes | the date in the filename |
| `task` | when there is one | the roadmap id (`T51`, `T120a`); several as a YAML list |
| `superseded_by` | with `superseded` | the replacing spec's filename, or `ADR 00NN` |

| Status | Means | Set by |
| --- | --- | --- |
| `draft` | written, not yet agreed | the author, when the spec is first written |
| `approved` | agreed; not built yet, or being built | the commit that records the agreement, or the first commit of the work |
| `implemented` | built as agreed; later change lives in newer specs and in `features/` and `architecture/` | **the commit that ticks the task `[x]` in the roadmap** |
| `superseded` | replaced by `superseded_by` | the commit that adds the replacing spec |
| `abandoned` | deliberately not built; its last paragraph says why | the commit that drops the work |

Once a spec is implemented, it is not edited to follow the code; a later change gets its own spec.
The header is the one exception. [specs/README.md](../specs/README.md) is generated from the headers
(`node scripts/check-docs.mjs --write-index`). CI fails when:

- a header is missing or invalid;
- a spec whose tasks are all ticked is not `implemented`;
- an `implemented` spec's task is still open;
- the index is stale;
- a `draft` or `approved` spec dated 2026-09-27 or later has no `## MixLab` section. MixLab is the
  product, so every design says which screen does what it describes, or why the window has no part
  in it. A method only `mix` can reach is a gap, and `scripts/check-client-surface.mjs` lists it.

## `docs/plans/` — local only, never referenced

Step-by-step implementation plans (`YYYY-MM-DD-slug.md`). They are scaffolding for one stretch of
work: long, quickly stale, and meaningless once the branch lands. They are gitignored — they exist
on the machine that wrote them and nowhere else.

**Never link to a file under `docs/plans/`.** Not from `CLAUDE.md`, `apps/desktop/CLAUDE.md`,
`docs/`, `README.md`, `CHANGELOG.md`, a commit message, a PR body, or a code comment. A link to a
path nobody else has is a dead link for every other reader, and `node scripts/check-docs.mjs`
fails on one.

When you want to point at the reasoning behind a feature, point at its spec, or record the decision
in [decisions/](../decisions/README.md). If something in a plan is worth keeping, move it into a
spec, a decision, a standard or a [roadmap](../roadmap/todo.md) entry — do not leave it in the plan
and link there.

## Where agent tooling writes them

Skills that write specs and plans default to `docs/superpowers/specs/` and
`docs/superpowers/plans/`. In this repository they are `docs/specs/` and `docs/plans/`:
`CLAUDE.md` says so, which overrides the default, and the check fails on any file under
`docs/superpowers/`.
