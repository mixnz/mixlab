---
status: approved
date: 2026-10-21
task: T209k
---

# One frame for editing rows in the sqlx engines (design)

The grid writes a table through three commands: `update_row` (one edited row), `insert_rows` (the
rows added since the last save) and `delete_rows` (the selected rows, or all of them). MySQL,
PostgreSQL and SQLite each answer them with a copy of one frame in `mysql.rs`, `postgres.rs` and
`sqlite.rs` under `src-tauri/src/modules/db/drivers/`: a transaction, a count before an update, a
rollback on the first failure, `error.rowsMatched` and `error.rowFailed`. Only the SQL differs. A
fix to the frame has to be made three times, and has already drifted once: SQLite rolls back
explicitly when an `UPDATE` fails, the other two leave it to the dropped transaction.

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209k, found by
T209c's measurement (about 174 lines repeated between engines).

## What was read

Every copy does the same walk.

**`update_row`**

1. No updates: return. No key: `error.updateWithoutKey`.
2. Begin a transaction.
3. `SELECT COUNT(*) FROM <table> WHERE <key predicate>`, binding the key's values. A count other
   than 1: roll back, `error.rowsMatched { matched }`.
4. `UPDATE <table> SET <c = ?, …> WHERE <key predicate>`, binding the updates' values, then the
   key's.
5. Commit.

**`insert_rows`**

1. No rows: return.
2. Begin a transaction.
3. Per row `i`: an `INSERT` of the columns the row names, binding its values. A failure: roll back,
   `error.rowFailed { index: i + 1 }` caused by the server's error.
4. Commit.

**`delete_rows`**

1. Nothing to do: return.
2. Begin a transaction.
3. With `all`: `DELETE FROM <table>`. Otherwise per key: an empty key rolls back with
   `error.deleteWithoutKey`; else a `DELETE` aimed at one row, binding the key's values.
4. Commit.
5. With the reset flag: put the table's counter back to 1, outside the transaction.

Every value is bound the same way in all three: a JSON string as `Some(&str)`, anything else as
`None::<&str>`, so the server coerces text to the column's type.

What differs is this, and nothing else:

| | MySQL | PostgreSQL | SQLite |
| --- | --- | --- | --- |
| the table | `` `db`.`t` `` | `"schema"."t"` | `"t"` |
| read before the transaction | nothing | the column types; for a delete, whether there is a primary key | for a delete, whether there is a primary key |
| placeholder | `?` | `$n::<type>`, one numbering through the statement | `?` |
| null-safe key match | `<=>` | `IS NOT DISTINCT FROM` | `IS` |
| a row of nothing but defaults | `() VALUES ()` | `DEFAULT VALUES` | `DEFAULT VALUES` |
| one row's delete, no primary key | `… LIMIT 1` | `(tableoid, ctid) = (SELECT … LIMIT 1)` | `rowid = (SELECT … LIMIT 1)` |
| one row's delete, with one | `… LIMIT 1` too | plain `WHERE` | plain `WHERE` |
| `delete_rows` returns early when | no `all`, no keys | no `all`, no keys, no reset | no `all`, no keys, no reset |
| resetting the counter | `ALTER TABLE … AUTO_INCREMENT = 1` | `restart_sequence` | `reset_sequence` |
| an `UPDATE` that fails | `?`: the dropped transaction rolls back | the same | explicit rollback, then the error |
| reading the count | `query(…).get::<i64>(0)` | `query_scalar` | `query_scalar` |

## D1 — the engine says only the SQL

`drivers/sql/editing.rs`, a new child of `drivers/sql.rs`, gets a trait each engine implements
with what the table above says differs in the statements:

```rust
/// The SQL an engine writes for the grid's edits. Every statement binds its values in one fixed
/// order, which `editing` owns: see each method.
pub trait RowSql {
    type Database: sqlx::Database;

    /// `SELECT COUNT(*)` of the rows `key` matches; binds the key's values.
    fn count(&self, key: &Map<String, Value>) -> String;
    /// The `UPDATE`; binds the updates' values, then the key's.
    fn update(&self, updates: &Map<String, Value>, key: &Map<String, Value>) -> String;
    /// One row's `INSERT`, `row` possibly empty; binds the row's values.
    fn insert(&self, row: &Map<String, Value>) -> String;
    /// The `DELETE` of the one row `key` names; binds the key's values.
    fn delete_one(&self, key: &Map<String, Value>) -> String;
    /// `DELETE FROM <table>`.
    fn delete_all(&self) -> String;
    /// The engine's own `map_error`.
    fn map_error(&self, error: sqlx::Error) -> AppError;
}
```

Each engine holds one struct of it beside its three functions: `MySqlRows { qualified }`,
`PgRows { qualified, types, keyed }`, `SqliteRows { quoted, keyed }`. What PostgreSQL and SQLite
read before the transaction is read by the engine before it builds the struct, so the order of
queries is unchanged. The `key_predicate`, `placeholder` and `has_primary_key` helpers stay where
they are.

**My call:** a trait over strings rather than one closure per statement. The binds' order is the
part a wrong edit breaks without a compiler error (PostgreSQL's numbering has to agree with it), and
with the trait it is written once, in `editing.rs`, beside the comment that says it.

## D2 — the frame, generic over `sqlx::Database`

```rust
pub async fn update_row<R: RowSql>(pool: &Pool<R::Database>, sql: &R,
    updates: &Map<String, Value>, key: &Map<String, Value>) -> Result<(), AppError>;
pub async fn insert_rows<R: RowSql>(pool: &Pool<R::Database>, sql: &R,
    rows: &[Map<String, Value>]) -> Result<(), AppError>;
pub async fn delete_rows<R: RowSql>(pool: &Pool<R::Database>, sql: &R,
    keys: &[Map<String, Value>], all: bool) -> Result<(), AppError>;
```

Steps 2 to 4 of each walk above, and the one `bind` of a JSON value, which replaces the three
`bind_value`s. The bounds are those `run.rs` already uses, plus `Option<&str>` being encodable and
`i64` decodable for the database. The count is read with `query_scalar` in all three.

What stays in each engine's `update_row`, `insert_rows` and `delete_rows`, with their signatures
unchanged, so `engine.rs` and the tests do not move:

- step 1, the early return and the `…WithoutKey` refusal, because MySQL's `delete_rows` returns
  early on a condition the other two do not, and PostgreSQL must not read the column types for a
  call that changes nothing;
- what is read before the transaction, and building the `RowSql`;
- the counter reset after the commit.

## D3 — what the change must not alter

The SQL each engine sends, byte for byte, the order of its binds, the order of its queries, and the
error each failure gives.

One difference is made the same on purpose: a failed `UPDATE` rolls back explicitly in all three,
the way SQLite does now and every engine already does for a failed `INSERT`. When the rollback
succeeds, which is the case that happens, the caller sees exactly the error it saw before. When the
rollback itself fails, MySQL and PostgreSQL now report that failure instead of the `UPDATE`'s, as
SQLite does today and as all three do after a failed `INSERT`.

The proof is written before anything moves. **Commit 1** adds tests that pin the SQL of each
engine's statements: today the text is built inline inside async functions that need a server, so
commit 1 lifts each statement's `format!` into a small private function per engine (`count_sql`,
`update_sql`, `insert_sql`, `delete_sql`), with a table-driven test of each, and changes nothing
else. **Commit 2** turns those functions into the engine's `RowSql` and moves the frame; the
tests from commit 1 then call the trait and assert the same strings unchanged.

The live tests stay as they are and pass unchanged: SQLite's twelve in `sqlite/tests.rs` that
update, insert and delete against a fixture database, `one_rejected_row_means_none_of_them_land`
and `a_key_matching_two_rows_changes_neither` among them. MySQL and PostgreSQL have no live test
here, so the frame is proved on SQLite and the per-engine SQL by commit 1's tests.

Other limits:

- No file goes past 1,000 lines. Every engine file gets shorter; `editing.rs` is about 150 lines.
- No comment written by this task cites a roadmap task number (`check-task-citations.mjs`).
- Nothing the window sends or shows changes, so there is no CHANGELOG line.

## D4 — commits

One branch, `t209k-one-frame-for-editing-rows`, and one pull request. Each commit leaves the build
and the tests green:

1. The statement functions and their tests from D3, against the three engines as they are.
2. `drivers/sql/editing.rs` with `RowSql` and the frame. The three engines switch to it, and their
   copies of the frame and their `bind_value`s go.
3. The phase file ticks T209k, and this spec becomes `implemented`.
   `docs/architecture/desktop/backend.md` gets a row for `drivers/sql/editing.rs`.

## MixLab

Saving an edited cell, added rows or a delete in the grid of a MySQL, PostgreSQL or SQLite
connection runs through the code this task changes, and sends the same SQL as before. Nothing a
user sees changes. This is inside MixLab's `db` module. MixEngine and its daemon have no part in
it, and no API method is added.

## Not in scope

- SQL Server's `mssql/editing.rs`. It runs on tiberius, has no `sqlx::Database`, and keeps its own.
- ClickHouse's mutations, which are `ALTER TABLE … UPDATE` over HTTP with no transaction.
- Making the engines agree where they differ today: MySQL's `delete_rows` returning early before a
  reset with no keys, or MySQL putting `LIMIT 1` on a delete by primary key. Each is a behaviour
  change, and gets its own task if it is wanted.

## Is there an ADR here? No

The frame moves into `drivers/sql/`, which already holds what the SQL engines share (`run.rs`). No
boundary between crates or modules changes.
