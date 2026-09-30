---
status: implemented
date: 2026-09-19
task: T169
---

# T169 — One home for the documentation

The roadmap entry for T169 lands with the plan. The decision is recorded as ADR 0043, which is
written as part of this work.

## Problem

MixEngine's engineering documentation is good. What is wrong is where it lives.

1. **It lives in `.claude/`.** That folder is hidden, it is named after one vendor's tool, and it
   mixes design documents with tool configuration: `settings.local.json`, `commands/`, `skills/`,
   `worktrees/`. A contributor who does not use that tool has no reason to look there. Some editors
   and search tools skip dotfolders by default.
2. **There are three roots for design material, with no rule for which one wins.**
   - `.claude/features/` holds 10 feature specs and calls them authoritative.
   - `docs/superpowers/specs/` holds 137 dated designs, and most of the real design work is there.
     The folder is named after a plugin, and a reader cannot tell which designs were built, which
     are still open and which were replaced. Only about ten carry a status at all, written five
     different ways: `**Status:** design, agreed 2026-08-26`, `**Status**: design`,
     `**Status:** accepted`, `**Status:** draft 3, 2026-09-19`, and so on. None of those says
     whether the design was ever built.
   - `.claude/desktop/architecture/` covers the desktop application.
3. **`.claude/desktop/` is a second, parallel tree**, carried over from the standalone client when ADR 0027 brought
   the application home. It has its own `architecture/`, `conventions/`, `decisions/`, `reviews/`
   and `notes/`. Several files exist twice: `plans-and-specs.md`, `changelog.md` and `icons.md`.
   Its README still calls it `.agent/`, and `apps/desktop/CLAUDE.md` still labels links `.agent/…`.
4. **Review ids collide.** `.claude/reviews/README.md` says `R<n>` is global and never reused, but
   `.claude/desktop/reviews/2026-08-27.md` numbers its findings from `R1` again.
5. **The indexes are stale.** `.claude/README.md` lists roadmap phases up to 13, but phases 14–22
   exist. `docs/releasing.md` is an orphan beside `operations/build-and-release.md`.
6. **Links already rot, and nothing notices.** A scan of the tree today finds:
   - 29 dead Markdown links, some of them only `file:line` anchors;
   - 5 path mentions to files that do not exist, among them `.claude/CLAUDE.md`, which is cited in
     `bindings/CertIssue.ts`, and `.claude/features/gui.md`.

   No check runs on any of this.

The move is expensive: **1,284 path references in 488 tracked files**.

| Where | Files that hold references |
| --- | --- |
| `crates/` | 211 |
| `docs/` | 128 |
| `.claude/` | 55 |
| `apps/` | 46 |
| `bindings/` | 30 |
| `.github/` | 4 |
| `packaging/` | 7 |
| `scripts/` | 2 |
| `Cargo.toml`, `deny.toml`, `.cargo/`, `.gitignore`, `CLAUDE.md` | 1 each |

Almost no code reads a documentation file at build or test time outside `docs/guide/`.
`crates/mixengine-docs/build.rs` and `packaging/docs.sh` read `docs/guide/{en,vi}` and nothing else.
The one exception, found during the move, is `crates/mixengine-core/tests/packaging.rs`: it
`include_str!`s `build-and-release.md`. Every other reference is one of the following:

- a comment or a doc comment;
- a Markdown link or a rustdoc reference link;
- an absolute GitHub URL.

That is what makes a scripted move safe, once the rewrite also covers rustdoc reference links, links
that were already written at the wrong depth, and URLs.

## Goals

- All documentation for people lives under `docs/`, in folders that are obvious from their names.
- `.claude/` holds only configuration for the agent tooling.
- There is one place for each kind of document, and a duplicate is merged rather than moved.
- Every existing reference keeps pointing at the same content after the move. Git history stays
  followable with `git log --follow`.
- Every spec states its status in the same machine-readable header, and the header stays true.
- A CI check fails on a dead documentation link, or on a spec without a valid header, from now on.

## Non-goals

- Rewriting or shortening any document's content. Only links, paths and the duplicates listed
  below change.
- Moving `docs/guide/`. It is the published handbook (ADR 0021), and its path is compiled into
  `mixengine-docs`.
- Renaming `roadmap/` to `planning/` or `features/` to `product/`. See *Deviations from the
  template*.
- Archiving finished roadmap phases, or splitting oversized files (`todo.md` is 50 KB, and some
  phase files run 40–77 KB). These are follow-ups, listed at the end.

## Target layout

```
docs/
  README.md              the map: what each folder answers, reading order (was .claude/README.md)
  guide/{en,vi}/         unchanged — the user handbook
  architecture/          how the system is put together
    desktop/             how MixLab is put together
  features/              what each user-facing feature must do; authoritative
  specs/                 dated designs, one per piece of work (was docs/superpowers/specs/)
  decisions/             ADRs 0001–0043
    desktop/             the four decisions the standalone client recorded, dated, kept as written
  standards/             how we write code here
    desktop/             MixLab's conventions
  operations/            build, packaging, release
  roadmap/               todo.md + one file per phase + parked.md
    desktop/             module roadmaps (mixengine module, query editor)
  reviews/               dated reviews of the workspace
    desktop/             dated reviews of MixLab, its own R<n> namespace
  plans/                 gitignored — local implementation plans (was docs/superpowers/plans/)
.claude/
  README.md              stub: "the documentation moved to docs/" + link
  settings.local.json, commands/, skills/, worktrees/
```

### Deviations from the template

The reference layout has `architecture`, `brand`, `decisions`, `modules`, `operations`,
`planning`, `product`, `reviews` and `specs`. This design keeps the idea and departs from it in
four places:

- **`features/` stays `features/`, and `roadmap/` stays `roadmap/`.** Both names are more precise
  than `product/` and `planning/`. The project `CLAUDE.md` and more than a thousand comments use
  them as vocabulary ("the roadmap", "a feature spec"), not only as paths. Renaming the folders
  would force a second rewrite of prose, not just of links.
- **No `brand/` and no `modules/` yet.** Today each would hold one or two files: the app-icon page;
  the query-editor roadmap. Rule: a top-level folder is created once it has three documents that
  fit nowhere else.
- **`guide/` is added.** It already exists and is published.
- **`standards/` is added.** Coding conventions are the most-read documents in the tree.

## Move map

Every move is a `git mv`. Paths are relative to the repository root.

### Whole folders

| From | To |
| --- | --- |
| `.claude/architecture/` | `docs/architecture/` |
| `.claude/features/` | `docs/features/` |
| `.claude/standards/` | `docs/standards/` |
| `.claude/operations/` | `docs/operations/` |
| `.claude/decisions/` | `docs/decisions/` |
| `.claude/roadmap/` | `docs/roadmap/` |
| `.claude/reviews/` | `docs/reviews/` |
| `docs/superpowers/specs/` | `docs/specs/` |
| `docs/superpowers/plans/` (untracked, a plain move) | `docs/plans/` |

### Single files

| From | To |
| --- | --- |
| `.claude/README.md` | `docs/README.md`, rewritten as the map; a stub stays behind |
| `docs/releasing.md` | `docs/operations/releasing.md` |

### `.claude/desktop/`, file by file

| From | To |
| --- | --- |
| `architecture/{overview,frontend,backend}.md` | `docs/architecture/desktop/` |
| `conventions/*.md` except the three below | `docs/standards/desktop/` |
| `conventions/plans-and-specs.md` | **merged into** `docs/standards/plans-and-specs.md`, then deleted |
| `conventions/changelog.md` | `docs/standards/desktop/changelog.md`; it adds to the root rule, so it is kept |
| `conventions/icons.md` | `docs/standards/desktop/icons.md` (UI glyphs) |
| `icons.md` (the app icon) | `docs/standards/desktop/app-icon.md`, renamed so it no longer clashes with the file above |
| `decisions/*.md` (4 dated files) | `docs/decisions/desktop/` |
| `reviews/README.md`, `reviews/2026-08-27.md` | `docs/reviews/desktop/` |
| `roadmap-mixengine-module.md` | `docs/roadmap/desktop/mixengine-module.md` |
| `notes/query-editor-roadmap.md` | `docs/roadmap/desktop/query-editor.md` |
| `README.md` (the `.agent/` readme) | deleted; what it says moves into `docs/README.md` |

### What stays in `.claude/`

- `settings.local.json`, `commands/`, `skills/` and `worktrees/`;
- the stub `README.md`.

The stub exists so that existing links into `.claude/README.md` from outside the repository —
issues, the `mixengine-packages` repository, bookmarks — land on a pointer and not on a 404.

## Content changes the move forces

These are the only prose edits this work makes.

- **`docs/README.md`**
  - Updated to the new layout.
  - Lists phases 0–22.
  - Lists both workspace reviews.
  - Gains a `desktop/` sub-entry under architecture, standards, decisions, roadmap and reviews.
- **`docs/standards/plans-and-specs.md`**
  - Says `docs/specs/` and `docs/plans/`.
  - Absorbs the desktop copy; its extra "not from `CHANGELOG.md`" clause is kept.
- **`docs/decisions/README.md`** gains a *Desktop (recorded in the standalone client)* section that indexes the four
  dated files. They keep their dated names: renumbering them would make them look like decisions
  this repository took.
- **`docs/reviews/desktop/README.md`** states that ids in this folder are their own namespace and
  are cited as "desktop R3". The review bodies are not edited: a review records what was found, as
  it was written.
- **`apps/desktop/CLAUDE.md`, `apps/desktop/README.md`, `apps/desktop/CHANGELOG.md`**: the link
  labels `.agent/…` and `AGENT.md` become real paths. The dead `docs/RELEASING.md` and
  `docs/ICONS.md` links point at `docs/operations/releasing.md` and
  `docs/standards/desktop/app-icon.md`.
- **`.gitignore`**: `docs/superpowers/plans/` becomes `docs/plans/`.
- **Agent tooling**
  - The root `CLAUDE.md` gains one line: specs are written to `docs/specs/`, plans to
    `docs/plans/`. This overrides the default path the superpowers skills use.
  - `.claude/commands/ship.md` and `.claude/skills/using-shared-components/SKILL.md` follow the
    rewrite.
- **The five mentions that already point at nothing** are fixed:

  | Mention | Fix |
  | --- | --- |
  | `.claude/CLAUDE.md` | becomes `CLAUDE.md` |
  | `.claude/features/gui.md` | becomes `docs/features/client-surface.md`, its successor |
  | `go-packaging-design.md`, `java-packaging-design.md` | point at `docs/specs/…-t27d-go-runtime-design.md` and `…-t27e-java-runtime-design.md` |
  | the literal `2026-09-05-....md` placeholder | left alone; it is prose, not a link |

## Spec status

Every file in `docs/specs/` opens with a YAML front-matter block. GitHub renders it as a small
table above the document, and a script can read it without parsing prose.

```yaml
---
status: implemented
date: 2026-08-25
task: T51
---
```

| Field | Required | Value |
| --- | --- | --- |
| `status` | yes | one of the five values below |
| `date` | yes | the date in the filename, `YYYY-MM-DD`: the day the design was written |
| `task` | when there is one | the roadmap id, e.g. `T51` or `T120a`; several are written as a list |
| `superseded_by` | when `status: superseded` | the replacing spec's filename, or `ADR 00NN` |

The five statuses, and when a spec moves between them:

| Status | Means | Moves on when |
| --- | --- | --- |
| `draft` | being written or reviewed; not agreed yet | the user approves it → `approved` |
| `approved` | agreed and not yet built, or being built now | its roadmap task is ticked `[x]` → `implemented` |
| `implemented` | built. The spec describes the design as it was agreed; later changes live in newer specs, and in `features/` and `architecture/` | a later spec replaces it → `superseded` |
| `superseded` | replaced; `superseded_by` names the replacement | — |
| `abandoned` | agreed or drafted, then deliberately not built; the body's last paragraph says why | — |

Two things change the status: ticking a roadmap task, and merging the spec that replaces an older
one. The rule "the commit that ticks the task flips its spec to `implemented`" goes into
`docs/standards/plans-and-specs.md`, next to the rule on keeping the roadmap current.

### Backfilling the 137 existing specs

The date comes from the filename. The status is decided in three passes, and each pass is recorded
in the plan so it can be reviewed:

1. **Specs with a task id in their name** (about 107). The id is looked up in the roadmap:
   - a `[x]` tick gives `implemented`;
   - a `[ ]` tick gives `approved`;
   - no line at all means the spec is read by hand.

   `[~]` counts as open. Three specs have no exact line: `t167`, `t168` and this one. Their work
   is written as lettered sub-tasks (`T168a`–`T168g`), so when there is no exact line the lookup
   reads every sub-task: the task is done only when all of them are `[x]`.
2. **Specs without a task id** (30). Most are designs from the standalone client's period: the REST client, the
   terminal, the tools module, ClickHouse, MSSQL. Each is read by hand against the code it
   describes, and gets `implemented`, `abandoned` or `superseded`.
3. **Pairs where one design replaced another**, read by hand. Candidates:
   - `t40-elevate` and `t40a-elevation`;
   - `mixengine-connection-handoff` and `t83-a-connection-handed-to-the-window`;
   - `module-architecture` and the phase 11 design;
   - the four `mixengine-*` client designs of 2026-09-06/07 and the phase 11–12 specs that
     replaced them.

   The older spec of each confirmed pair gets `superseded` and a `superseded_by`.

The old inline status lines are removed; the header replaces them. There are ten of them, such as
`**Status:** design, agreed 2026-08-26` and `**Status:** accepted`. Removing them is the only body
edit the backfill makes.

A generated `docs/specs/README.md` lists every spec by date, with its status and task. It is
regenerated by the same script the link check calls, and the check fails when the committed index
differs from the generated one. The index therefore cannot go stale the way `.claude/README.md`
did.

## Rewriting references

A script does the rewrite. It lives in the scratchpad, is run once, and is **not committed**: after
the move there is nothing left for it to do. It works from the move map above and does three
separate things.

1. **Repository-rooted mentions** in any tracked text file, such as `.claude/features/tls.md` in a
   Rust comment. The script applies the map longest prefix first, so that
   `.claude/desktop/conventions/` is matched before `.claude/desktop/` and `.claude/`.
2. **Relative Markdown links** such as `](../decisions/0005-….md)`. A prefix replace is wrong here:
   when the *source* file moves, links from it break even if the target stays put. For each link,
   the script:
   1. resolves it against the source file's **old** location;
   2. maps the resolved target through the move map;
   3. writes it back relative to the source file's **new** location.

   `#anchor` and `:line` suffixes pass through unchanged.
   One exception: a link that was dead before the move, because it was written for the file's
   *new* depth, is left as it is when it resolves from the new location. One spec already links
   `../../crates/…` from `docs/superpowers/specs/`, which is the right depth for `docs/specs/`.
3. **`bindings/` is not edited.** It is generated from doc comments in `mixengine-proto`. After
   step 1 rewrites those comments, `bash packaging/bindings.sh` regenerates it. Every line that
   changes in `bindings/` must be a line that names a documentation path; any other change means
   the rewrite touched something it should not have.

The bare folder names `.claude`, `.claude/`, `.claude/desktop/` and `docs/superpowers/` are mapped
in Markdown only. In code they may be something other than a link to the documentation.

Excluded from the rewrite:

- `docs/plans/`: it is local, and nothing links there by rule.
- `.claude/settings.local.json`.
- This spec. It describes the old layout on purpose.

Changelogs are **not** excluded. Only one changelog line names a documentation path, a dead
`.agent/` link in `apps/desktop/CHANGELOG.md`, and it is fixed by hand.

## Guarding it: a link check in CI

A new `scripts/check-docs.mjs` runs in the `lint` job. It uses Node and no dependencies, like
`scripts/set-version.mjs`. `ubuntu-latest` ships Node, and every developer here already has it
for `apps/desktop`, so a check that parses Markdown links and YAML headers is simpler in Node than
in bash. Its pure functions are tested by `scripts/check-docs.test.mjs` with `node --test`.

The check fails on any of the following:

1. a relative Markdown link, in any tracked `.md` outside `docs/plans/`, whose target does not
   exist, ignoring `#anchor` and `:line`. In other text files, only links that resolve into
   `docs/` or `.claude/` are checked;
2. a repository-rooted mention matching `(docs|\.claude)/[\w./-]+\.md` in any tracked file that
   names a file that does not exist. Placeholders (`YYYY`, `NNNN`, `....`) and a short allowlist
   of other repositories' paths are skipped;
3. a link to, or a mention of, a *file* under `docs/plans/`, the rule from `plans-and-specs.md`,
   now enforced;
4. a `.md` file under `.claude/` other than `README.md`, `commands/*` and `skills/**`, or any
   file under `docs/superpowers/`, so that documentation cannot drift back in;
5. a file in `docs/specs/` whose header is missing or invalid:
   - no front-matter, or an unknown `status`;
   - a `date` that differs from its filename;
   - `superseded` without `superseded_by`, or a `superseded_by` naming a file that does not exist;
6. a `docs/specs/README.md` that differs from the one the script generates;
7. a spec whose header contradicts the roadmap:
   - its `task` is on no roadmap line;
   - it is `draft` or `approved` while every task is ticked `[x]`;
   - it is `implemented` while a task is still open.

On today's tree the check has to fail. It is committed last, once the tree passes it.

## ADR 0043

**Title**: *Documentation for people lives under `docs/`; `.claude/` holds only agent
configuration.*

It records:

- the problem above;
- the layout;
- the three-document rule for new top-level folders;
- the desktop sub-namespaces;
- the link check.

It supersedes nothing. ADR 0021 (the handbook) and ADR 0027 (the application in this repository)
remain true as written.

## Order of work

The work goes on one branch, `docs/one-home-for-the-documentation`, in six commits, so that the
rename commit is a pure rename:

0. `docs(specs): add the design for T169`. This spec, at its current path, so that the move
   carries it like every other spec.
1. `docs: move documentation from .claude/ into docs/`. Only `git mv`s, no content change, so Git
   detects every rename at 100 % similarity. `.gitignore` changes here too, so that the moved
   local plans never appear as untracked files.
2. `docs: rewrite references to the moved documentation`. The script, then
   `bash packaging/bindings.sh`, then the forced content changes listed above.
3. `docs: add ADR 0043 and phase 23 for the documentation move`. This comes before the statuses,
   because check 7 needs the roadmap line T169 is ticked on.
4. `docs(specs): give every spec a status header`. The backfill, plus the generated
   `docs/specs/README.md`.
5. `ci: check documentation links and spec status in the lint job`.

The branch merges by squash as usual. The rename survives the squash, because each moved file
changes only in its links and stays far above Git's 50 % similarity threshold.

**Timing.** `fix/apply-recreates-deleted-project-dir` is the only other branch. Merge or rebase it
first. Any branch opened later that edits a moved file resolves as a rename conflict, which Git
handles. A branch that *adds* a spec under `docs/superpowers/specs/` has to move that spec by hand.

## Verification

- The link check passes. It also fails on a deliberately broken link, tried once and then reverted.
- After commit 2, `bash packaging/bindings.sh` changes only lines that name a documentation path.
- Every spec has a valid header.
  - The generated index lists 138 specs, and none of them is left without a status.
  - For each task id, the status in the header agrees with its tick in the roadmap.
- `cargo fmt --all --check` and `cargo clippy --workspace -- -D warnings` pass, since comments
  moved. `RUSTDOCFLAGS="-D warnings" cargo doc …` passes, since intra-doc text changed.
- `cd apps/desktop && npm run lint` passes.
- `bash packaging/docs.sh` builds the site with exactly the same pages as before.
- `git log --follow docs/decisions/0005-on-demand-elevation.md` reaches the ADR's first commit.
- `git grep -E '\.claude/(architecture|features|standards|operations|decisions|roadmap|reviews|desktop)|docs/superpowers'`
  returns only:
  - this spec and ADR 0043;
  - the section of `plans-and-specs.md` that names the skills' default path;
  - review bodies, which are records.
- CI is requested for the branch, which is not `master`.

## Risks

| Risk | Handling |
| --- | --- |
| Links from outside the repository (issues, `mixengine-packages`, bookmarks) into `.claude/…` break; GitHub has no redirects | The stub `.claude/README.md` points at `docs/`. `gallery.yml`'s error text is rewritten with the rest. Links into `mixengine-packages` are checked by hand once. |
| The superpowers skills keep writing to `docs/superpowers/specs/` | The line added to `CLAUDE.md`, plus check 3 and a check on the path's existence: a new file there fails the check. |
| The rewrite changes something that was not a path | The rewrite is purely map-driven. Commit 2 is reviewed as a diff, and the `bindings/` check is a tripwire: after regeneration, every changed line there must name a documentation path. |
| The move collides with work in flight | Merge the one open branch first, and do the move in one sitting. |

## Follow-ups, not part of T169

- **Archive finished roadmap phases** into `docs/roadmap/archive/`, and cut `todo.md` down to an
  index.
- **Split the largest documents**: `runtime-packaging.md` (64 KB) and the 93 KB desktop review.
- **Revisit `brand/` and `modules/`** when either reaches three documents.

## Settled questions

1. **Changelogs.** No released changelog entry names a documentation path, so there is nothing to
   exclude.
2. **Desktop review ids** are namespaced by their folder and cited as "desktop R3". No `DR<n>`
   prefix.
3. **ADRs keep their `**Status**:` line.** It is already consistent, and an ADR is immutable once
   accepted. Moving ADRs to front-matter later is a mechanical change.
