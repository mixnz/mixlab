---
status: approved
date: 2026-10-20
task: T209i
---

# One statement splitter for the SQL engines (design)

The Query tab sends its whole script to the backend, and the backend cuts it into statements and
runs them one at a time. The window cuts the same script itself, to highlight the statement under
the caret and to run that one alone. The two must cut in the same places: the statement the editor
highlights has to be the one the server runs.

The window already does this once. `src/modules/db/sql/statements.ts` has one `splitStatements(sql,
syntax)`, and what differs between engines travels in a `SqlSyntax` from `syntax.ts`. The backend
does it four times. `split_statements` in `mysql/`, `postgres/`, `sqlite/` and `mssql/` `script.rs`
under `src-tauri/src/modules/db/drivers/` are four hand-written copies of one loop, kept in step with
the window and with each other by tests alone. SQL Server's copy does not even agree with the
window: it cuts `GO` batches line by line before it looks at strings or comments.

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209i, found by
T209c's measurement.

## What was measured

T209c ran `jscpd` 4.3.0 over `drivers/` on `master` at `8733802d`. Between engines, `script.rs`
repeats about 185 lines in the splitter (`split_statements` and its inner `push`, in mssql, mysql,
postgres and sqlite) and about 200 in `run` and `validate`. Read by hand, the four splitters differ
only in these rules, which are exactly the fields of the window's `SqlSyntax`:

| rule | MySQL | PostgreSQL | SQLite | SQL Server |
| --- | --- | --- | --- | --- |
| `#` opens a line comment | yes | no | no | no |
| `--` needs whitespace after it | yes | no | no | no |
| `/* */` nests | no | yes | no | no |
| identifier quote beside `"` | `` ` `` | none | `` ` `` | `[` … `]` |
| backslash escapes in a string | yes, in `'` and `"` | only in `E'…'` | no | no |
| `$tag$ … $tag$` | no | yes | no | no |
| `GO` line ends a batch | no | no | no | yes, cut before splitting |

Everything else is the same in all four: a doubled close quote is an escaped one, a semicolon
outside all of the above ends a statement, the first word is the verb, a chunk without a verb is
not a statement, and comments stay in the statement's text.

The `run` loops are less alike than the splitters. PostgreSQL's and SQLite's are the same loop:
one `StatementResult` per statement, rows capped at `MAX_ROWS`, the server's message on a failure,
stop at the first failure. They differ only in `last_insert_id`, which SQLite reads from the rowid
after an `INSERT` or `REPLACE` and PostgreSQL never sets. MySQL's loop is a different loop: a stored
procedure returns several result sets and one closing acknowledgement, and MySQL reports each
result set as a result of its own. SQL Server runs on tiberius rather than sqlx, and ClickHouse
sends each statement as an HTTP request.

## D1 — `SqlSyntax` in Rust

`drivers/sql/syntax.rs` holds a `SqlSyntax` struct and one constant per engine: `MYSQL`,
`POSTGRES`, `SQLITE`, `MSSQL`. The fields are the window's fields in snake case, with the same
meaning and the same values for these four engines:

- `hash_comments`
- `dash_comment_needs_space`
- `nested_block_comments`
- `identifier_quote: Option<(char, char)>`, the open and close character
- `backslash_escapes`
- `escape_string_prefix`
- `dollar_quoting`
- `batch_separator`

`doubleQuoteIsIdentifier` is not carried over. It matters to the window's tokenizer, which tells a
name from a string. The splitter treats both the same way, so the backend has no use for it.

There is no `CLICKHOUSE` constant, see D5.

## D2 — one `split_statements`

`drivers/sql/statements.rs` holds `Statement { text, verb }` and `split_statements(sql: &str,
syntax: &SqlSyntax) -> Vec<Statement>`. It is the window's `splitStatements` in Rust, rule for rule,
and replaces the four copies. PostgreSQL's helpers `opens_escape_string` and `dollar_tag` move with
it, and run only when the syntax asks for them.

Each engine's `script.rs` keeps its own tests. Their assertions do not change. Only their helper,
which calls the splitter, now passes the engine's constant. Every test the four files hold today
passes after the move: mssql 13, mysql 7, postgres 10 and sqlite 8.

**My call:** the verb is upper-cased with `char::to_uppercase`, as MySQL and SQL Server do today
and as the window's `toUpperCase` does. PostgreSQL and SQLite use `to_ascii_uppercase`. The two
differ only on a statement whose first word starts with a non-ASCII letter, and no verb the backend
compares against has one.

## D3 — `GO` is read where the splitter already knows strings and comments

Today `mssql/script.rs` cuts the script into batches with `split_batches` before it splits
anything. `split_batches` reads line by line and does not know whether a line sits inside a string
or a comment. So a line that holds only `GO` inside a multi-line string or comment is cut as a
batch separator, and the window does not cut it there. The file's own comment records this as a
known gap from the window. `split_batches` also rejoins the lines with `\n`, so a `\r\n` script
reaches the server with its carriage returns gone.

**Decided with the user:** fix it in this task rather than carry it over. With `batch_separator`
set, the shared splitter does what `statements.ts` does. In plain code, on a `g` or `G`, it checks
for a `GO` line, using `matchBatchSeparator`'s rule:

- Only whitespace sits between the previous newline and the `G`. This is read from the original
  text, not from the statement built so far, so `SELECT 1; GO` is not a separator.
- Then comes `GO`, case-insensitive, with a word boundary after it, optional spaces or tabs, an
  optional run of digits with spaces after it, an optional `\r`, and a newline or the end.

On a match it ends the current statement and continues after the line. `is_go_line`,
`split_batches` and `split_script` go away.

This is a behaviour change on purpose, and it reaches two places:

- **The Query tab** on a SQL Server connection, where a `GO` line inside a string or a block
  comment no longer splits the script.
- **Restoring a SQL Server dump**, since `mssql::dump::restore` replays a file through
  `script::run`. A file this app wrote has no `GO` lines, so its restore is unchanged. A file
  written by hand has its `GO` lines read the way the Query tab reads them.

It also takes the window's stricter rule for the line itself. `GO3` and `GO 3 4` stop being
separators. Today they are, because `is_go_line` accepts any mix of digits and spaces after `GO`.

New tests in `mssql/script.rs`:

- a `GO` line inside a string and one inside a block comment do not split
- `GO3` is not a separator
- a `\r\n` script keeps its text

The CHANGELOG gets one line under `### Fixed`.

## D4 — one `run` loop for PostgreSQL and SQLite

`drivers/sql/statements.rs` gets the loop the two share. The builder may put it in a file of its own
beside it. It is a generic function over `sqlx::Database`, not a macro:

- It takes the connection the engine has already acquired and prepared, the statements, and what
  the engine supplies: its `column_value`, its `is_write_verb`, and how to read `last_insert_id`
  from a finished statement's result.
- It returns the `Vec<StatementResult>`.

Acquiring the connection, `close_on_drop`, PostgreSQL's `announce(backend_pid)` and the "nothing to
run" error stay in each engine's `run`, in the order they are in now.

`MAX_ROWS` (10,000, the same value in all five engines) moves to `drivers/sql/` once, and MySQL, SQL
Server and ClickHouse read it from there.

MySQL keeps its own loop, and so do SQL Server and ClickHouse, for the reasons under *What was
measured*. They use the shared splitter (MySQL, SQL Server) and the shared `MAX_ROWS`, and nothing
else from this task.

If a generic `fetch_many` over `DB::Connection` cannot be written without bounds that leak into
every caller, the loop stays in the two engines and the reason is recorded in the phase file. The
splitter is the bigger half of this task, and does not depend on the loop.

## D5 — ClickHouse stays as it is

`clickhouse/script.rs` has a `Scanner` that reads one character at a time and can resume. The
restore reader in `clickhouse/dump.rs` feeds it a file in chunks, and a chunk can end in the middle
of a string. The shared splitter takes the whole script at once, so ClickHouse would need the
`Scanner` anyway.

ClickHouse also escapes a backtick- or double-quoted name with a backslash, which no other engine
does and which the window's `SqlSyntax` cannot express (`backslashEscapes` never applies inside an
identifier there). Bringing it in would mean a field for that and a resumable shape for every
engine, to serve one. The phase file records ClickHouse as checked and left out, with this reason.

## D6 — what the change must not alter

- Apart from D2's verb casing and D3's `GO`, the statements are the same text and verb as before.
  The four engines' existing splitter tests prove it.
- A PostgreSQL or SQLite script returns the same `StatementResult`s as before: the same `kind`, the
  same rows, the same `last_insert_id` and the same stop at the first failure. No test runs a
  script today. So commit 3 first adds tests in `sqlite/script.rs` against an in-memory database,
  written and passing against the current loop before it moves. They cover each `kind`,
  `last_insert_id` after an `INSERT` and after an `UPDATE`, rows past `MAX_ROWS`, and the stop at
  a failure. PostgreSQL has no server in the unit tests. It runs the same generic code, and is
  checked by hand in the running window: a script with a `SELECT`, an `UPDATE` and a failing
  statement.
- `src/modules/db/sql/` keeps its code. Its header comments in `statements.ts` and `syntax.ts`
  still point at `src-tauri/src/db/mysql_script.rs` and `postgres_script.rs`, paths that no longer
  exist, and say SQL Server has no Rust splitter. They are corrected to name
  `drivers/sql/statements.rs`, since that is the file they must now agree with.
- No comment written by this task cites a roadmap task number, per `check-task-citations.mjs`.
  Citations in lines the task deletes go with them, and `--shrink` records it.
- Every file stays under 1,000 lines. The shared file starts near 250, and every `script.rs` gets
  shorter.

## D7 — commits

One branch, `t209i-one-statement-splitter`, and one pull request. Each commit leaves the build and
the tests green:

1. `drivers/sql/syntax.rs` and `drivers/sql/statements.rs`, with tests of their own for each rule
   in the table above. MySQL, PostgreSQL and SQLite switch to it, and their copies go.
2. SQL Server switches, with D3's `GO` handling, its new tests, the CHANGELOG line and the corrected
   comments in `statements.ts` and `syntax.ts`.
3. The SQLite script tests from D6 against the current loop, then the shared `run` loop for
   PostgreSQL and SQLite, and the shared `MAX_ROWS`.
4. The phase file ticks T209i, records ClickHouse as left out with D5's reason, and this spec
   becomes `implemented`.

## MixLab

The Query tab of a MySQL, PostgreSQL, SQLite or SQL Server connection, and restoring a SQL Server or
SQLite dump, run through the code this task changes. On SQL Server, a `GO` line inside a string or
a comment stops splitting the script, as the editor already showed it. Nothing else a user sees
changes. This is all inside MixLab's `db` module. MixEngine and its daemon have no part in it, and
no API method is added.

## Not in scope

- ClickHouse's `Scanner`, see D5.
- MySQL's and SQL Server's `run` loops, and every engine's `validate`. Each asks its server in its
  own way, through its own protocol.
- Making the window and the backend share one splitter. They are two languages, and the tests on
  each side stay the contract.
- MySQL's `DELIMITER`, which neither splitter supports.

## Is there an ADR here? No

The code moves into `drivers/sql/`, which ADR 0062 already made the home of what the SQL engines
share. No boundary between crates or modules changes.

## Documentation, when it lands

The phase file and this spec's header, as in D7. `docs/architecture/desktop/backend.md` lists
what `drivers/sql/` holds. It gets a row for `drivers/sql/syntax.rs` and
`drivers/sql/statements.rs`, and its row for `drivers/mysql/script.rs` says the splitting is
shared.
