# Filter bar

The condition bar above a table (MySQL) or document list (Mongo) is split in two: the
database-agnostic mechanics in `src/modules/db/workspace/filters.ts`, and one operator list per *kind* in
`src/modules/db/sql/filters.ts` (MySQL and PostgreSQL share it, the way they share the workspace)
and `src/modules/db/mongo/filters.ts`.

## The shared half — `src/modules/db/workspace/filters.ts`

- `FilterArity` — how many values an operator reads out of its single text box: `"none"`
  (`IS NULL`, box disabled), `"one"`, `"list"` (comma-separated, for `IN`), `"pair"` (`min,max`,
  for `BETWEEN`).
- `FilterRow` — a row while it is being edited: the condition plus a synthetic `id` (rows can be
  identical and positions shift) and an `enabled` checkbox, so a condition can be kept written down
  but unapplied.
- `QueryFilter` — what is actually sent: `{ column, operator, value }`. Values are **always text**;
  the operator says how to read them. Conditions are ANDed together.

## The per-database half

`FILTER_OPERATORS` is a `const satisfies readonly FilterOperatorSpec[]` array, ordered as the
dropdown offers them (comparisons, text matches, set/range, value-less last). Each `id` does triple
duty, and adding an operator means touching all three:

1. The frontend id in `sql/filters.ts` or `mongo/filters.ts`.
2. Its label in **both** `en.ts` and `vi.ts` — under `sqlTable.op.*` for MySQL, `noSqlTable.op.*`
   for Mongo.
3. The matching arm in the backend's WHERE builder — `build_where`, which each SQL driver has its
   own of (`drivers/mysql.rs` and `drivers/postgres.rs`, since the placeholders differ), with value
   splitting shared from `src-tauri/src/modules/db/drivers/filters.rs`.

An id present in the frontend list but missing from the backend match silently drops the condition;
missing from the dictionaries it shows as the raw id.

## Where the bar is remembered

What each table (or collection) was left carrying is a `FilterCache` — a `Map` keyed
`"<db> :: <table>"`, exported by `SqlTable`/`NoSqlTable` and **owned by the workspace**, which
passes it back down as a prop. It lives up there rather than in a ref inside the grid because
leaving the Data tab unmounts the grid entirely: a cache inside it would only survive switching
table, not switching tab. The grid seeds its state from the cache when it mounts, writes the
outgoing entry during the render that first sees a new table, and writes the current one again on
unmount. One cache per connection tab, gone when the tab closes.
