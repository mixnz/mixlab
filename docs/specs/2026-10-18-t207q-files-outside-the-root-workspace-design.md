---
status: implemented
date: 2026-10-18
task: T207q
---

# The ten files outside the root workspace, split (design)

After T207p, `scripts/file-size-baseline.json` names ten files, and none of them is in the root
Cargo workspace: four are the desktop's Rust (`apps/desktop/src-tauri`, a workspace of its own),
four its frontend, and two the sync server's (`server/native`, a third workspace, and
`server/worker`). Each is checked by its own project's tools, which is why they are one task of their
own. When it lands the baseline is empty again, as it was after T207j.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207q. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 holds: a commit only moves
code — with one exception, the Worker's `Account` class (D5), where a move is not possible. Measured
on `master` at `82496891`.

| File | Lines |
| --- | --- |
| `apps/desktop/src-tauri/src/modules/db/drivers/clickhouse.rs` | 1,053 |
| `apps/desktop/src-tauri/src/modules/db/drivers/mssql.rs` | 1,144 |
| `apps/desktop/src-tauri/src/modules/db/drivers/postgres.rs` | 1,152 |
| `apps/desktop/src-tauri/src/sync/session.rs` | 1,322 |
| `apps/desktop/src/modules/db/db.css` | 1,128 |
| `apps/desktop/src/shell/App.css` | 1,294 |
| `apps/desktop/src/modules/db/i18n/en.ts` | 1,156 |
| `apps/desktop/src/modules/db/i18n/vi.ts` | 1,131 |
| `server/native/src/accounts.rs` | 1,013 |
| `server/worker/src/account.ts` | 1,083 |

## D1 — the rules

The Rust files follow [T207p](2026-10-18-t207p-code-split-by-capability-design.md)'s D1 unchanged:
a group goes to `foo/<group>.rs` with a one-line `//!` and its own `use` lines; visibility widens
only to `pub(super)`; a public item that moves is re-exported from the parent, so no path outside
the file changes; no test changes its path, and a test that names a moved free function gains a
`use` line and nothing else. A child is never named after a function in its parent (T207p's "as
built" note: it makes a doc link ambiguous).

The frontend files have rules of their own, in D3 and D4.

## D2 — the desktop's Rust

### The three SQL drivers: one page of a table

The roadmap's line: `table_data` and `build_where` move into the engine's folder. Both serve one
thing, reading one page of a table for the grid, and so do the private helpers only they call. The
child is `<engine>/page.rs` — not `table_data.rs`, which would share a name with the function it
holds. `table_data` is re-exported (`pub use page::table_data;`), so each `engine.rs` that calls
`clickhouse::table_data(…)` is untouched. `build_where` becomes `pub(super)`, and each engine's
`tests.rs` imports it from `super::page`.

| File | Moves to `page.rs` | Lines | Left |
| --- | --- | --- | --- |
| `clickhouse.rs` (1,053) | `build_where`, `table_data` (810–1050) | 240 | ~815 |
| `mssql.rs` (1,144) | `build_where`, `order_by_clause`, `foreign_keys`, `table_data` (576–689, 733–758, 857–1026) | 310 | ~835 |
| `postgres.rs` (1,152) | `foreign_keys`, `build_where`, `is_decodable`, `table_data` (350–531, 567–597, 643–756) | 325 | ~830 |

What stays, and why: `table_columns`, `primary_key`, `select_expr`, `row_to_json` and
`column_value` are also called by the writes, `dump.rs`, `editing.rs` or `script.rs`. ClickHouse's
`is_decodable` is called by `key_clause`, which the writes use. The compiler settles any item this
table puts on the wrong side.

### `sync/session.rs`: `impl SyncState` (1,082) split by what its methods do

| Moves to | What | Lines |
| --- | --- | --- |
| `session/recovery.rs` | starting again without the password: `ask_reset`, `open_reset`, `reset_keeping`, `prepare_start_over`, `start_over`, and `finish`, which only `start_over` calls (606–757, 1025–1058) | 185 |
| `session/moving.rs` | moving the account to another server: `move_begin`, `move_confirm`, `move_finish`, `move_abandon` (849–1024) | 175 |
| `session/records.rs` | what the engine asks of a session: `notice`, `heads`, `pull_page`, `push`, `push_reporting`, `commit_pull`, `commit_push`, `commit` (1059–1320) | 260 |

The parent keeps the types, `Inner`, the free helpers, and the account's own life: `new`, `status`,
`register`, `verify`, `login`, the session plumbing, `logout`, `devices`, `revoke`,
`change_password`, `delete_account` and the freeze methods — about 700 lines.
`session/tests.rs` stays where it is.

## D3 — the stylesheets: split where they already divide, not into CSS Modules

Both files are global stylesheets, and their classes are global names that components spell as
strings. Turning a section into a CSS Module would rename every class it holds — a change to the
components, not a move. So each splits into global stylesheets, at the seams their own comments
already draw, and the new files are imported **in the same place and order** as the one they came
from. The cascade then sees the same rules in the same order, which is what makes this a move.

| File | Moves to | What | Lines | Left |
| --- | --- | --- | --- | --- |
| `db/db.css` (1,128) | `db/connectionForm.css` | `.saved-elsewhere` through `.connection-string`: the connection editor, its engine picker, the tunnel test and the route drawing (280–719) | 440 | ~280 |
| | `db/workspaces.css` | `.workspace` through `.db-sidebar-list`: the workspace frame, its method tabs, and the SQL, Mongo and Redis workspaces (720–1128) | 410 | |
| `shell/App.css` (1,294) | `shell/chrome.css` | `html, body` through the focus rules: the page, the tab bar and its marks, the tab panels, menus, and the base look of links, code, buttons and fields (291–709) | 420 | ~875 |

`db.css` keeps its file comment, the tab marks, the saved-connection list and the engine colours.
`DbTab.tsx` imports `./db.css`, `./connectionForm.css` and `./workspaces.css` on consecutive lines;
`App.tsx` and `tray.tsx`, the two places that import `App.css`, import `./chrome.css` straight
after it. Each new file opens with one comment saying what it styles, as `db.css` does.

**As built.** The first draft moved the colour themes out of `App.css` into a `palettes.css`.
`colourLiterals.test.ts` refused it, and rightly: the redesign's decision record says colour lives
in `App.css`, and that test is how it is held. So `App.css` keeps every token, the dark theme and
the colour themes, and the chrome moves out instead — rules that name no colour of their own. That
puts the chrome after the themes rather than between the light and dark tokens. It cannot change a
computed value: no property the chrome sets is set by the dark theme or a colour theme, which was
checked over the two sets of declarations before the move. `contrast.test.ts` and
`colourLiterals.test.ts` read the same text as before and are unchanged; `fonts.test.ts` and
`core/gridRows.test.ts` glob `**/*.css` and pick up the new files on their own. The comments and
documents that named `App.css` for a chrome rule (`.tab-panel`, `.context-menu`, the theme's
cross-fade) name `chrome.css` now.

Verified by the rule text, comments aside, being the same before and after; by
`npm run screenshots -- --check`; and by rendering all 24 promotional images before and after the
split, which came out byte for byte the same.

## D4 — the dictionaries: split by screen, the same keys in both

`db/i18n/en.ts` is one object of 31 top-level groups, and `t("connection.host")` reaches a group by
its name. So groups move whole, nothing is renamed, and the object `en.ts` exports holds the same
groups in the same order. Two runs of groups belong to one kind of screen:

| Moves to | Groups | Lines (en) |
| --- | --- | --- |
| `db/i18n/sql/en.ts`, `sql/vi.ts` | `sql` through `insertRows`: the SQL workspace, its Structure and Statistics tabs, its dialogs, the Query tab, lint, the filter bar and the grid (130–656) | 525 |
| `db/i18n/nosql/en.ts`, `nosql/vi.ts` | `insertDocuments` through `redisGroup`: the Mongo and Redis workspaces (657–880) | 225 |

`en.ts` becomes `{ connection: {…}, ...sqlEn, ...nosqlEn, tools: {…}, …, dbSync: {…} }` — about
400 lines — and `DbDict` stays `typeof dbEn`. Each part exports its own type (`SqlDict = typeof
sqlEn`), and its `vi.ts` is typed by it, so `tsc` still fails on a key that is in one language and
not the other. `i18n/dicts.ts` is unchanged.

Checked by `tsc` (the `vi` types are what keep the two languages' keys equal), `npm test`, and by
comparing `JSON.stringify` of the assembled `EN` and `VI` before and after: identical.

## D5 — the sync server

### `server/native/src/accounts.rs` (1,013): a move

| Moves to | What | Lines |
| --- | --- | --- |
| `accounts/limits.rs` | the rate limits: `window`, `source_for`, `may_send_letter`, `retry_after`, `too_many_attempts`, `throttled` (96–209) | 115 |
| `accounts/verifier.rs` | proving the password again: `prove_verifier`, `check_verifier`, `delete_account` (877–1013) | 135 |

`lib.rs`, `recovery.rs` and the router call these as `crate::accounts::window` and
`accounts::check_verifier`, so the parent re-exports each with its current visibility
(`pub(crate) use limits::{…};`, `pub use verifier::{check_verifier, delete_account};`). No path
changes. Left: about 770.

### `server/worker/src/account.ts` (1,083): not a move

`Account` is one class, and a TypeScript class does not span files. The handlers that are easiest to
take out are the record ones — `readRecords`, `readHeads`, `writeRecord`, `batch` (299–431) — with
the two free helpers only they use, `readPrecondition` and `outcome` (68–91). That is also how
`server/native/` is laid out: its record handlers are in a `records.rs` of their own.

They become exported functions in `src/accountRecords.ts` that take the object as their first
argument, `(object: Account, config, request, …)`, and `Account.fetch`'s `switch` calls
`readRecords(this, config, request, url)` where it called `this.readRecords(config, request, url)`.
Inside them `this.` becomes `object.`, and the members they use — `sql`, `authenticate`,
`frozen`, `account`, `scheduleReaping` — lose `private`, since a function outside the class cannot
reach a private member. (`authenticate` is already public.) No body changes beyond `this` →
`object`. Left: about 930.

**As built.** The parameter is `object` rather than the draft's `account`: `batch` already has a
local `account`, the account's row.

The name is `accountRecords.ts`, not `records.ts`, which already holds the record table's own
logic.

Checked by `npm run typecheck` and `npm test` in `server/worker`, and by the conformance suite run
locally as `server.yml` runs it, against both servers and both of their instances (the reaping one
with the cursor-expiry tests).

## D6 — commits and verification

Four commits, then the roadmap tick:

1. **The desktop's Rust** (four files). `cargo test --all-targets -- --list` in
   `apps/desktop/src-tauri` before and after, sorted and identical; `cargo fmt --check`;
   `cargo clippy --locked --all-targets -- -D warnings`; the moved-lines check with
   `git diff --color-moved=zebra --color-moved-ws=allow-indentation-change`.
2. **The desktop's stylesheets** (two files). `npm run build`, `npm test`, `npm run lint`,
   `npm run screenshots -- --check`.
3. **The desktop's dictionaries** (two files). `npm run build`, `npm test`, `npm run lint`, and the
   `EN`/`VI` comparison from D4.
4. **The sync server** (two files). `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D
   warnings` and `cargo test` in `server/native`; `npm run typecheck` and `npm test` in
   `server/worker`; the conformance suite against both servers locally, and again in CI's `server`
   workflow.

Each commit ends with `node scripts/check-file-sizes.mjs --shrink`, which removes its entries; the
last one leaves `"files": {}`. Then `bash scripts/gate.sh`, `scripts/ask-ci.sh` for every job, one
pull request, squash-merged.

## MixLab

No visible part. Every screen draws what it drew, from the same rules (D3) and the same strings
(D4); the promotional images, rendered before and after, are what says so. Sync answers what it answered,
and the conformance suite says so for both servers.

## Not in scope

- Turning `db.css`'s sections into CSS Modules beside their components. That renames classes, and
  is a refactor of its own.
- Splitting `Account` further, or into a class per capability.
- Lowering the limit below 1,000.

## Is there an ADR here? No

It applies T207p's conventions to three more projects. D5's worker change is local to one class.

## Documentation, when it lands

- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207q ticked, linking this spec, which
  flips to `implemented`, and the line after T207n saying the baseline is empty again.
  `docs/roadmap/todo.md` counts it.
- `apps/desktop/CLAUDE.md`'s layout lists `db.css` and `App.css`; it gains the new stylesheets.
- `docs/specs/README.md` regenerated with `node scripts/check-docs.mjs --write-index`.
