---
status: implemented
date: 2026-10-15
task: T208c
---

# The `db` module's folders follow its subjects and its engines (design)

`apps/desktop/src/modules/db/` has 41 files at its root beside eleven folders. The saved list
has six of them, the Query tab's drafts, history and snippets four, the sync of connections three,
and the connection form and its arrival eight more that `tab/`'s hooks are built on. The engine
folders hold each engine's `api.ts` and dialect, but eight components only Mongo or Redis draws
sit in `components/` with the ones every engine shares.

This task moves files and nothing else. No identifier, string or line of logic changes. Each
import path is rewritten to the file's new place. The vitest count stays at 2,902.

## D1 — the root keeps the module's entry points and what everything imports

| File | Why it stays |
| --- | --- |
| `index.ts`, `DbTab.tsx` | The module's definition and its tab |
| `api.ts`, `tools.ts` | Every invoke no engine owns, and the dump tools `api.ts` types |
| `types.ts`, `icons.tsx`, `db.css` | Imported from nearly every folder |
| `engines.ts` (+ test) | The SQL engines' registry, which reaches into five engine folders |
| `shortcuts.ts` | Part of the module's definition, like `index.ts` |
| `tabState.ts` (+ test) | Every module keeps its tab's remembered state in `modules/<id>/tabState.ts`, as `docs/standards/desktop/adding-a-module.md` says, and all five do |

That is 12 files, down from 41.

## D2 — four subject folders, and one for the workspaces

| Folder | Files (each with its `.test.ts` where one exists) |
| --- | --- |
| `saved/` | `savedConnections.ts`, `savedConnectionsLoad.test.ts`, `savedConnectionsStore.ts`, `savedListScroll.ts` |
| `queries/` | `queryDrafts.ts`, `queryHistory.ts`, `querySnippets.ts` |
| `sync/` | `sync.ts`, `syncWrite.test.ts` |
| `tab/` (exists) | `handoffArrival.ts`, `badges.ts`, `connectionForm.ts`, `connectionString.ts` |
| `workspace/` (new) | `sidebarWidth.ts`, `workspaceError.ts`, `schemaTokens.ts`, `filters.ts` |

- **`connectionForm.ts` and `connectionString.ts` go to `tab/`.** `tab/` already holds
  `useConnectionForm.ts` and `mongoUri.ts`, which is `connectionString.ts`'s counterpart for Mongo.
  `sql/SqlWorkspace.tsx` and `QueryEditor` import `kindLabel` from it, and that stays an import.
- **`workspace/` is not in the roadmap's list.** It holds what the three workspaces (`sql/`,
  `mongo/`, `redis/`) share and no single one owns: the sidebar's width, which errors fall through
  to the banner, the schema change counter, and the engine-blind half of the filter bar. Leaving
  these four (eight files with tests) at the root is the alternative. The root then has 20 files.
- **File names do not change**, so `sync/sync.ts` keeps its name. A rename would make the diff
  more than a move, and `git log --follow` less reliable.

## D3 — the engine-only components move into their engine

| From `components/` | To |
| --- | --- |
| `Document/`, `DocumentNode/`, `NoSqlTable/`, `InsertDocumentsDialog/` | `mongo/components/` |
| `RedisKeyList/`, `RedisValue/`, `RedisGroupKeys/`, `RedisTypeBadge/` | `redis/components/` |

Each folder moves whole, with its `index.ts`, its `.module.css` and, for `NoSqlTable/`, its
`request.ts` and test. Only the Mongo or Redis workspace and these components themselves import
them today. After this, `components/` holds 23 folders. Each is drawn by more than one engine, or
by the SQL workspace every SQL engine shares (`SkipIndexDialog` is ClickHouse's, but
`TableStructure` opens it).

`<engine>/components/` is a parent `docs/standards/desktop/component-structure.md` does not list
yet. Its table gains a row: a component only one engine draws lives under that engine's folder,
in `components/`, with the same folder shape. The rule "import from the folder, never the file"
is unchanged.

`checkDesktopGuide` treats any `modules/db/<x>/api.ts` as an engine folder. None of the new
folders has an `api.ts`, so its list of engines does not change.

## D4 — how the move is made

1. `git mv` each file, so git records a rename (≥ 50% similarity; for most files 100%).
2. A throwaway Node script rewrites the imports. For every `.ts`, `.tsx` and `.mjs` under
   `apps/desktop/src` and `apps/desktop/demo`, it resolves each relative specifier against the file's old place,
   maps the target to its new place, and writes the shortest relative path from the file's new
   place. `vi.mock(import("./x"))` and `await import("./x")` in tests are rewritten the same way.
   The script lives in the scratchpad, not in the repository.
3. Comments name moved files by their base name only (`` `schemaTokens.ts` ``,
   `` `connectionForm.ts` ``, eleven of them, two in `rest/` and `terminal/`). Base names do not
   change, so no comment changes. A `grep` for each old path after the move confirms it.

Outside the module, only `demo/fixtures/db.ts` (`types.ts`, `sql/api.ts`) and
`demo/scenes.test.mjs` (`tabState.ts`) import from it, plus `shell/registry.ts` and
`i18n/dicts.ts`, whose targets do not move. `demo/scenes.test.mjs` is plain JavaScript, so
`tsc` will not catch it, and the script covers `.mjs` too.

No `composes: x from "…"` in the moved stylesheets reaches another file, and the stylesheet tests
(`core/gridRows.test.ts`, `shell/stylesheets.test.ts` and others) read through recursive
`import.meta.glob` patterns, so they find the moved `.module.css` files where they land.

## D5 — what proves nothing changed

- `npm run build` (tsc and Vite), `npm test` with exactly 2,902 tests, and `npm run lint`.
- `git diff -M --stat` shows only renames. A file with a changed line shows only import lines and
  the comments from D4.3. A check script lists every changed line that is neither.
- `npm run screenshots -- --check`, `check-docs`, `check-file-sizes`, `check-strings` and
  `check-client-surface`.
- No DOM recording. The splitting skill's recording proves a rewrite kept the screen. A move
  that type-checks and keeps every import has nothing for it to catch.

## D6 — documentation

`check-docs` fails on a dead link, so every markdown link to a moved file is retargeted. That
includes links in implemented specs (`savedConnections.ts`, `savedConnectionsStore.ts`,
`tabState.ts`, …). Their `#L` anchors stay as they are: they pointed at old lines already.

Paths in backticks are updated in the living documents: `apps/desktop/CLAUDE.md`'s Layout,
`docs/architecture/desktop/`, `docs/standards/desktop/` and `docs/roadmap/desktop/`. Specs,
reviews and phase files that are history keep their backticked paths, as they did after T207's
moves.

In `apps/desktop/CLAUDE.md`'s Layout, `modules/db/` gains `saved/ queries/ sync/ workspace/`,
and `mongo/ redis/` say they carry their own `components/`.

`docs/standards/desktop/component-structure.md` gains the `<engine>/components/` row (D3).
`docs/architecture/desktop/frontend.md`'s list of `src/modules/db/components/` drops `NoSqlTable`,
the document views and the Redis views, and a line names where they went.
`docs/standards/desktop/filter-bar.md` names `src/modules/db/filters.ts` four times; it becomes
`workspace/filters.ts`.

## D7 — commits

On a branch `t208c-db-module-folders`, cut from a pushed `master`:

1. The subject folders (D1, D2), with their import rewrites.
2. The engine components (D3).
3. Documentation (D6), the roadmap tick and this spec's status.

Each commit builds, tests and lints on its own.

## MixLab

This is MixLab's own `db` module, with `mixengined` absent or present alike. No command, daemon
method, string or screen changes. Nothing goes in `CHANGELOG.md`. `mix` has no part in this.

## Not in scope

- The backend's `drivers/` (T208d).
- Renaming files, or merging small ones (`badges.ts` into `useTabBadges.tsx`, for example).
- Splitting `components/` further. `QueryEditor` and `SqlTable` are SQL-only too, but every SQL
  engine draws them. `SkipIndexDialog` belongs to ClickHouse, but it moves with `TableStructure`'s
  dialogs or not at all.

## Is there an ADR here? No

## Documentation, when it lands

The phase file ticks T208c and links this spec, and this spec flips to `implemented`.
