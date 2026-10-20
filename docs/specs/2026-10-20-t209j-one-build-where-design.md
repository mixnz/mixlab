---
status: approved
date: 2026-10-20
task: T209j
---

# One `build_where` for the SQL engines (design)

The grid's filter bar sends its rows to the backend, and each SQL engine turns them into a `WHERE`
clause and the values bound into it. Five engines do this with five copies of one function:
`build_where` in `mysql.rs`, `sqlite.rs`, and the `page.rs` of `postgres/`, `mssql/` and
`clickhouse/`, under `src-tauri/src/modules/db/drivers/`. The copies have the same 22 arms, in the
same order, with the same rules for a row that has too little to say. A fix to one arm has to be
made five times, and nothing but review says it was.

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209j, found by
T209c's measurement (about 205 lines repeated between engines).

## What was read

Every copy does the same walk:

1. A filter on a column the table does not have is refused with `error.unknownFilterColumn`. An
   operator no arm knows is refused with `error.unknownFilterOperator`.
2. The column appears in one of two forms. The **text form** is the column as compared with text.
   The **raw form** is the quoted column as it stands.
3. Each value is bound, never written into the SQL. The placeholder is spelled the engine's way.
4. `in`/`notIn` with no items and `between`/`notBetween` with fewer than two are dropped, not
   refused.
5. The clauses are joined with ` AND ` behind a leading ` WHERE `. With none, the clause is empty.

Which form each operator uses is the same in all five:

| operator | column | value |
| --- | --- | --- |
| `eq`, `ne` | text | as text |
| `gt`, `gte`, `lt`, `lte` | raw | as the column's type |
| `contains`, `notContains`, `startsWith`, `endsWith` | text | as text, wildcards escaped |
| `like`, `notLike` | text | as text, as typed |
| `regexp`, `notRegexp` | text | as text, as typed |
| `in`, `notIn` | text | each item as text |
| `between`, `notBetween` | raw | both bounds as the column's type |
| `isNull`, `isNotNull` | raw | none |
| `isEmpty`, `isNotEmpty` | text | none, compared with `''` |

What differs is this, and nothing else:

| | MySQL | SQLite | PostgreSQL | SQL Server | ClickHouse |
| --- | --- | --- | --- | --- | --- |
| quoted name | `` `id` `` | `"id"` | `"id"` | `[id]` | `` `id` `` |
| text form | the raw form | `CAST(c AS TEXT)` | `c::text` | `CAST(c AS nvarchar(max))` | `toString(c)` |
| placeholder | `?` | `?` | `$1`, `$2` … | `@P1`, `@P2` … | `{p0:String}`, `{p1:String}` … |
| a value "as the column's type" | the bare placeholder | the bare placeholder | `CAST($n AS <type>)` | the bare placeholder | `{pN:<type>}` |
| what is bound | text | text | text | text | a name and text |
| `regexp`, `notRegexp` | `REGEXP`, `NOT REGEXP` | refused | `~`, `!~` | refused | refused |
| escaping a `LIKE` value | `escape_like` | `escape_like` | `escape_like` | `escape_like_mssql`, which also escapes `[` | `escape_like` |
| `ESCAPE '\'` written after | never | the four escaped operators | never | every `LIKE` | never |
| columns known from | a list of names | a list of names | name → type | name → type | name → type |

The "refused" regex cells are the same error a misspelled operator gets. The window does not offer
the operator on those three engines (`regexpFilter: false` on their dialects), so it is only reached
by a request the window would not send.

## D1 — a `WhereSyntax` per engine

`drivers/filters.rs` gets a struct that holds what the table above says differs, apart from the
placeholders (D2):

- `quote_ident: fn(&str) -> String`, the engine's own `quote_ident`
- `as_text: fn(&str) -> String`, from the raw form to the text form
- `regexp: Option<(&'static str, &'static str)>`, the operator and its negation, `None` to refuse
- `escape_like: fn(&str) -> String`
- `like_escape: LikeEscape`, an enum: `Never`, `Escaped` (the four operators whose value this code
  escaped), `Always` (every `LIKE` and `NOT LIKE`)

Each engine holds one constant of it beside its `build_where`: `mysql.rs`, `sqlite.rs` and the
three `page.rs`. It is not a constant in `filters.rs` per engine, because `quote_ident` and
`escape_like_mssql` belong to the engine and stay where they are.

## D2 — placeholders as a trait

How a placeholder is spelled and what is bound into it differ in their state as well as their
text: PostgreSQL and SQL Server count, ClickHouse names, and ClickHouse binds pairs. So it is a
trait rather than a field:

```rust
pub trait Placeholders {
    type Bind;
    /// Binds a value compared as text, and returns what stands for it in the SQL.
    fn text(&mut self, value: String) -> String;
    /// Binds a value compared as the column's own type.
    fn typed(&mut self, value: String, column_type: &str) -> String;
    fn into_binds(self) -> Vec<Self::Bind>;
}
```

`filters.rs` has two implementations: `QuestionMarks` for MySQL and SQLite, and
`Numbered { prefix, cast_typed }` for PostgreSQL (`"$"`, casting) and SQL Server (`"@P"`, not
casting). ClickHouse's `{pN:Type}` stays in `clickhouse/page.rs` as a third. Its type is a
ClickHouse type name, and nothing else uses one.

**My call:** the trait rather than a closure per spelling. A closure cannot also say what the bind
list holds, and ClickHouse's differs.

## D3 — one `build_where`

```rust
pub fn build_where<P: Placeholders>(
    filters: &[Filter],
    column_type: impl Fn(&str) -> Option<&str>,
    syntax: &WhereSyntax,
    placeholders: P,
) -> Result<(String, Vec<P::Bind>), AppError>
```

`column_type` answers `None` for a column the table does not have. MySQL and SQLite know only
names, and answer `Some("")`: their `typed` never reads the type. The engines with types pass a
lookup into the map they already build.

Each engine's own `build_where` stays, with its signature unchanged, as one call to the shared one
with its constant and its placeholders. Its callers and its tests do not change. Its doc comment
keeps what is particular to the engine, such as why PostgreSQL casts the value and SQL Server does
not, or why SQLite has no regex. What is common moves to the shared function's comment.

## D4 — what the change must not alter

The SQL text and the binds each engine produces, byte for byte, for every operator.

The proof is written before anything moves. **Commit 1** adds one table-driven test per engine. It
calls that engine's `build_where` with every one of the 22 operators, plus an unknown operator, an
unknown column, an empty `in` and a one-item `between`, and asserts the exact clause and binds. It
passes against today's code. **Commit 2** moves the code and does not touch those tests.

They live in one file, `drivers/filters/where_clauses.rs`, a test-only child module of
`filters.rs`. `mysql.rs` is at 927 lines and could not take its share. The engines' private
`build_where` in `mysql.rs` and `sqlite.rs` become `pub(super)` so the file can reach them. The
filter tests each engine has today stay where they are and pass unchanged, SQLite's three that run
a filter against a fixture database among them.

`filters.rs` also gets tests of its own for the shared function with a made-up syntax: an arm per
`LikeEscape`, a refused regex, and the order of the binds across several rows.

Other limits:

- No file goes past 1,000 lines. `filters.rs` goes from 185 to about 400, and every engine file gets
  shorter.
- No comment written by this task cites a roadmap task number (`check-task-citations.mjs`).
- Nothing the window sends or shows changes, so there is no CHANGELOG line.

## D5 — commits

One branch, `t209j-one-build-where`, and one pull request. Each commit leaves the build and the tests
green:

1. The tests from D4, against the five copies as they are.
2. `WhereSyntax`, `Placeholders` and the shared `build_where` in `filters.rs`, with their own tests.
   The five engines switch to it, and their copies go.
3. The phase file ticks T209j, and this spec becomes `implemented`. `docs/architecture/desktop/
   backend.md`'s row for `filters.rs` says it builds the `WHERE` for the SQL engines.

## MixLab

The grid's filter bar on a MySQL, SQLite, PostgreSQL, SQL Server or ClickHouse connection runs
through the code this task changes, and sends the same SQL as before. Nothing a user sees changes.
This is inside MixLab's `db` module. MixEngine and its daemon have no part in it, and no API method
is added.

## Not in scope

- Making the engines agree where they differ today. SQLite writes no `ESCAPE` after `like`, MySQL
  and PostgreSQL rely on the backslash being their default escape, and ClickHouse refuses `regexp`
  although it has `match()`. Each is a behaviour change, and gets its own task if it is wanted.
- MongoDB's `build_filter`, which builds a document, not SQL.
- `table_data` around `build_where`. T209c counted it with this part, but what is left there after
  the move is each engine's own query for the page, its count and its foreign keys.
- Editing rows, which is T209k.

## Is there an ADR here? No

The code moves into `drivers/filters.rs`, which already holds what the engines' filters share. No
boundary between crates or modules changes.
