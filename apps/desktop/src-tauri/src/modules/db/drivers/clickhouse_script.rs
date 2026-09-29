//! Running whatever SQL the Query tab sends, against ClickHouse — the counterpart of
//! `mysql_script.rs`/`postgres_script.rs`/`sqlite_script.rs`, and two of their three jobs: split a
//! script, and run it statement by statement. There is no third: ClickHouse's `KILL QUERY` needs a
//! `query_id` tracked per request, which nothing here does yet, so the Cancel button is closed —
//! see `cancellable` on the dialect.
//!
//! What the splitter has to know matches `CLICKHOUSE_SYNTAX` in `src/modules/db/sql/syntax.ts`,
//! checked against a running server rather than only documented: `#` opens a comment
//! (`SELECT 1 # x;\n...`), `--` needs no trailing space (`SELECT 5--3` is `5`, not `2`), `/* */`
//! nests, and a single-quoted string takes both a doubled quote and a backslash as an escape
//! (`'it''s'` and `'it\'s'` both read back as `it's`). Backtick and double quote are both
//! identifier quotes, backslash-escaped rather than doubled the way `quote_ident` writes one.
//!
//! Every statement is sent to the server as its own HTTP request — ClickHouse's HTTP interface
//! refuses a body holding more than one — which is the reason this module exists at all rather
//! than handing `run_script`'s whole text to `query` in one call.

use super::clickhouse::{
    execute_check, execute_with_written_rows, query_in_database, run_mutation_and_wait, Connection,
    QueryResult,
};
use crate::error::AppError;
use crate::modules::db::models::{SqlProblem, StatementResult};
use serde_json::Value;
use std::time::Instant;

/// How many rows of one result set are read back — the same ceiling the other three engines hold
/// a script's result set to.
const MAX_ROWS: usize = 10_000;

/// One statement carved out of the editor's text.
struct Statement {
    text: String,
    /// The keyword it opens with, upper-cased. Empty for a run of nothing but comments.
    verb: String,
}

/// Where the scanner is inside a statement, resumable across calls to [`Scanner::feed`] so that a
/// chunk boundary landing mid-comment or mid-string does not read as the end of one — the property
/// the streaming restore reader in `clickhouse_dump.rs` needs (see that module's doc).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    /// Not inside a comment or a quoted string. `pending` is the previous character when it might
    /// be the first half of `--` or `/*` and the next character decides which.
    Body {
        pending: Option<char>,
    },
    LineComment,
    /// `pending` is `Some('*')` right after a `*` that a following `/` would close, or `Some('/')`
    /// right after a `/` that a following `*` would open one level deeper.
    BlockComment {
        depth: u32,
        pending: Option<char>,
    },
    /// Inside `` ` ``, `"` or `'` — `quote` says which. `escaped` when the previous character was
    /// an unconsumed `\`.
    Quoted {
        quote: char,
        escaped: bool,
    },
    /// Just closed a `'`; one more `'` reopens the string (SQL's doubled-quote escape) rather than
    /// ending it for good.
    MaybeDoubledSingleQuote,
}

pub(super) enum Fed {
    More,
    /// This `;` ends the statement — the caller's own buffer, not including this character, is the
    /// whole statement text; [`Scanner::verb`] is its opening keyword.
    End,
}

/// A statement-boundary scanner, fed one character at a time and resumable across calls — the core
/// both `split_statements` below (the whole script at once) and `clickhouse_dump.rs`'s restore
/// reader (a file read in chunks) drive, so the two can never disagree about where a statement ends.
/// Encodes the same rules `split_statements`' own doc lists (checked against the test server): `#`
/// and `--` open a line comment, `/* */` nests, backtick/double-quote identifiers are backslash-
/// escaped, single-quoted strings take both a doubled quote and a backslash as an escape.
pub(super) struct Scanner {
    region: Region,
    verb: String,
    verb_done: bool,
}

impl Scanner {
    pub(super) fn new() -> Self {
        Self {
            region: Region::Body { pending: None },
            verb: String::new(),
            verb_done: false,
        }
    }

    /// The statement's opening keyword, upper-cased — empty for a run of nothing but comments.
    pub(super) fn verb(&self) -> &str {
        &self.verb
    }

    /// Starts the next statement. Only ever called right after [`Fed::End`], where `region` is
    /// already back to `Body { pending: None }` — nothing there needs resetting, only the verb.
    pub(super) fn reset(&mut self) {
        self.verb.clear();
        self.verb_done = false;
    }

    pub(super) fn feed(&mut self, c: char) -> Fed {
        match self.region {
            Region::LineComment => {
                if c == '\n' {
                    self.region = Region::Body { pending: None };
                    return self.body_from(None, c);
                }
                Fed::More
            }
            Region::BlockComment { depth, pending } => {
                match (pending, c) {
                    (Some('/'), '*') => {
                        self.region = Region::BlockComment {
                            depth: depth + 1,
                            pending: None,
                        };
                    }
                    (Some('*'), '/') => {
                        self.region = if depth <= 1 {
                            Region::Body { pending: None }
                        } else {
                            Region::BlockComment {
                                depth: depth - 1,
                                pending: None,
                            }
                        };
                    }
                    _ => {
                        self.region = Region::BlockComment {
                            depth,
                            pending: (c == '/' || c == '*').then_some(c),
                        };
                    }
                }
                Fed::More
            }
            Region::Quoted { quote, escaped } => {
                if escaped {
                    self.region = Region::Quoted {
                        quote,
                        escaped: false,
                    };
                } else if c == '\\' {
                    self.region = Region::Quoted {
                        quote,
                        escaped: true,
                    };
                } else if c == quote {
                    self.region = if quote == '\'' {
                        Region::MaybeDoubledSingleQuote
                    } else {
                        Region::Body { pending: None }
                    };
                }
                Fed::More
            }
            Region::MaybeDoubledSingleQuote => {
                if c == '\'' {
                    self.region = Region::Quoted {
                        quote: '\'',
                        escaped: false,
                    };
                    Fed::More
                } else {
                    self.region = Region::Body { pending: None };
                    self.body_from(None, c)
                }
            }
            Region::Body { pending } => self.body_from(pending, c),
        }
    }

    fn body_from(&mut self, pending: Option<char>, c: char) -> Fed {
        match (pending, c) {
            (Some('-'), '-') => {
                self.region = Region::LineComment;
                Fed::More
            }
            (Some('/'), '*') => {
                self.region = Region::BlockComment {
                    depth: 1,
                    pending: None,
                };
                Fed::More
            }
            // The pending character was not the first half of anything — resolve it as an
            // ordinary character (verb tracking included) before handling `c` fresh.
            (Some(prev), _) => {
                self.plain(prev);
                self.body_from(None, c)
            }
            (None, '#') => {
                self.region = Region::LineComment;
                Fed::More
            }
            (None, '-') | (None, '/') => {
                self.region = Region::Body { pending: Some(c) };
                Fed::More
            }
            (None, '`') | (None, '"') => {
                self.region = Region::Quoted {
                    quote: c,
                    escaped: false,
                };
                Fed::More
            }
            (None, '\'') => {
                self.region = Region::Quoted {
                    quote: '\'',
                    escaped: false,
                };
                Fed::More
            }
            (None, ';') => {
                self.region = Region::Body { pending: None };
                Fed::End
            }
            (None, _) => {
                self.plain(c);
                Fed::More
            }
        }
    }

    /// Verb tracking for one character known to be ordinary body text — the counterpart of the
    /// original splitter's bottom `if !verb_done { ... }` branch.
    fn plain(&mut self, c: char) {
        if !self.verb_done {
            if c.is_alphanumeric() || c == '_' {
                self.verb.push(c.to_ascii_uppercase());
            } else if !self.verb.is_empty() {
                self.verb_done = true;
            }
        }
    }
}

/// Splits a script into the statements that are to be sent one at a time.
///
/// Ported from `src/modules/db/sql/statements.ts`, which the editor splits with so that the
/// statement it highlights is the one that ends up running — **a change to either splitter
/// belongs in the same commit as the other**, and in both sets of tests. Comments are kept in the
/// text: they may carry a hint (`SETTINGS`, say), and dropping them would change what is run.
fn split_statements(sql: &str) -> Vec<Statement> {
    let mut statements: Vec<Statement> = Vec::new();
    let mut current = String::new();
    let mut scanner = Scanner::new();

    for c in sql.chars() {
        match scanner.feed(c) {
            Fed::More => current.push(c),
            Fed::End => {
                let text = current.trim().to_string();
                let verb = scanner.verb().to_string();
                current.clear();
                if !verb.is_empty() {
                    statements.push(Statement { text, verb });
                }
                scanner.reset();
            }
        }
    }
    let text = current.trim().to_string();
    let verb = scanner.verb().to_string();
    if !verb.is_empty() {
        statements.push(Statement { text, verb });
    }
    statements
}

/// The server's own words out of a failed call, rather than `AppError`'s debug form
/// (`"error.clickhouse message=…"`) its `Display` would give — this module reports server errors
/// as text meant for the Query tab to show beside the statement, not as a translation code.
fn server_message(error: &AppError) -> String {
    error
        .params
        .get("message")
        .cloned()
        .unwrap_or_else(|| error.to_string())
}

/// One value, as the closest JSON the frontend can show — the counterpart of `column_value` on
/// the other engines. `FORMAT JSON` has already turned it into JSON, so this is only about the
/// shape a table cell wants: a `Vec<Value>` in column order rather than a `Map`.
fn row_to_columns(row: &serde_json::Map<String, Value>, columns: &[String]) -> Vec<Value> {
    columns
        .iter()
        .map(|c| row.get(c).cloned().unwrap_or(Value::Null))
        .collect()
}

/// One token of a statement's own text that D3/D4 of
/// `docs/specs/2026-09-04-clickhouse-query-dml-design.md` need — nothing more. Walked
/// fresh here rather than reusing `split_statements`'s loop: that function only tracks a
/// statement's own boundaries and its opening verb, never what comes after, and its quoting rules
/// (backtick/double-quote identifiers, both escape styles, nesting block comments, `#`/`--` line
/// comments) are exactly what this needs too — see that function's own doc for where each rule was
/// checked against the test server.
enum Word {
    /// An unquoted run of letters/digits/underscore — a keyword or a bare identifier. Original
    /// case kept: SQL keywords compare case-insensitively, but a bare table name's case is part of
    /// its name.
    Bare(String),
    /// Backtick- or double-quoted, quoting undone (backslash-escaped, the same convention
    /// `quote_ident` writes one in).
    Quoted(String),
    Comma,
    Dot,
}

impl Word {
    fn is_keyword(&self, kw: &str) -> bool {
        matches!(self, Word::Bare(s) if s.eq_ignore_ascii_case(kw))
    }

    fn name(&self) -> Option<&str> {
        match self {
            Word::Bare(s) | Word::Quoted(s) => Some(s),
            _ => None,
        }
    }
}

/// Splits `sql` into the [`Word`]s above, dropping whitespace, comments and string literals, and
/// everything inside a bracketed group — `ALTER TABLE <name> UPDATE`/`DELETE FROM <name> WHERE`
/// never need to look inside one, the name always sits at the top level.
fn dml_words(sql: &str) -> Vec<Word> {
    let chars: Vec<char> = sql.chars().collect();
    let mut words = Vec::new();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '#' || (c == '-' && chars.get(i + 1) == Some(&'-')) {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            let mut nesting = 1u32;
            while i < chars.len() && nesting > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    i += 2;
                    nesting += 1;
                    continue;
                }
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    i += 2;
                    nesting -= 1;
                    continue;
                }
                i += 1;
            }
            continue;
        }
        if c == '`' || c == '"' {
            let quote = c;
            i += 1;
            let mut value = String::new();
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() {
                    value.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                i += 1;
                if ch == quote {
                    break;
                }
                value.push(ch);
            }
            if depth == 0 {
                words.push(Word::Quoted(value));
            }
            continue;
        }
        if c == '\'' {
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() {
                    i += 2;
                    continue;
                }
                i += 1;
                if ch == '\'' {
                    if chars.get(i) == Some(&'\'') {
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        if c == '(' {
            depth += 1;
            i += 1;
            continue;
        }
        if c == ')' {
            depth -= 1;
            i += 1;
            continue;
        }
        if depth == 0 && c == ',' {
            words.push(Word::Comma);
            i += 1;
            continue;
        }
        if depth == 0 && c == '.' {
            words.push(Word::Dot);
            i += 1;
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            if depth == 0 {
                words.push(Word::Bare(chars[start..i].iter().collect()));
            }
            continue;
        }
        i += 1;
    }
    words
}

/// The `[database.]table` reference starting at `words[at]` — a bare/quoted name, optionally
/// followed by `.` and a second bare/quoted name. `None` when `words[at]` is not a name.
fn read_table_ref(words: &[Word], at: usize) -> Option<(Option<String>, String, usize)> {
    let first = words.get(at)?.name()?.to_string();
    if matches!(words.get(at + 1), Some(Word::Dot)) {
        let second = words.get(at + 2)?.name()?.to_string();
        Some((Some(first), second, at + 3))
    } else {
        Some((None, first, at + 1))
    }
}

/// `Some((database, table))` when `sql` is `ALTER TABLE [db.]name UPDATE ...` and nothing else —
/// one `AlterCommand`, no comma anywhere at the top level. A comma means more than one command is
/// packed into the `ALTER` (ClickHouse allows `ALTER TABLE t DROP COLUMN x, UPDATE y = 1 WHERE
/// ...`) — left unrecognised on purpose (D3 of the design doc): the safe default is to fall through
/// to the existing, still-blocked DDL path rather than guess.
///
/// `sql` is one statement's own text; the caller already knows its opening verb is `ALTER` from
/// `Statement::verb` before calling this.
fn alter_table_update_target(sql: &str) -> Option<(Option<String>, String)> {
    let words = dml_words(sql);
    if words.iter().any(|w| matches!(w, Word::Comma)) {
        return None;
    }
    if !words.first()?.is_keyword("ALTER") {
        return None;
    }
    if !words.get(1)?.is_keyword("TABLE") {
        return None;
    }
    let (database, table, next) = read_table_ref(&words, 2)?;
    if !words.get(next)?.is_keyword("UPDATE") {
        return None;
    }
    Some((database, table))
}

/// `Some((database, table))` when `sql` is `DELETE FROM [db.]name ...` — ClickHouse's lightweight
/// delete. `sql` is one statement's own text; the caller already knows its opening verb is
/// `DELETE`.
fn delete_from_target(sql: &str) -> Option<(Option<String>, String)> {
    let words = dml_words(sql);
    if !words.first()?.is_keyword("DELETE") {
        return None;
    }
    if !words.get(1)?.is_keyword("FROM") {
        return None;
    }
    let (database, table, _next) = read_table_ref(&words, 2)?;
    Some((database, table))
}

/// What sending one statement to ClickHouse turned into — the result [`dispatch_statement`] hands
/// back to `run()`'s loop, which is the one place `duration_ms` and the statement's own text get
/// folded in to build a `StatementResult`.
enum DispatchOutcome {
    /// A result set from the unchanged `query_in_database` path.
    Rows(QueryResult),
    /// `INSERT`, synchronous — ClickHouse's own count of what it wrote.
    Affected(u64),
    /// `TRUNCATE`, or a mutation (`ALTER TABLE ... UPDATE`/`DELETE FROM ... WHERE`) that finished.
    /// ClickHouse gives no rows-affected count for either (see the design doc's Non-goals), so
    /// there is nothing more than "it worked" to report.
    Ok,
    Err(AppError),
}

/// Resolves which database a mutation's table sits in — the statement's own qualifier if it named
/// one, the Query tab's active database otherwise — then runs it through the same
/// `run_mutation_and_wait` the grid's `update_row`/`delete_rows` already use (D1 of the design:
/// dialect-agnostic, does not care whether `command_sql` is an `UPDATE` or a `DELETE` mutation).
async fn run_as_mutation(
    conn: &Connection,
    qualifier: Option<String>,
    table: &str,
    database: Option<&str>,
    command_sql: &str,
) -> DispatchOutcome {
    let resolved = qualifier
        .filter(|d| !d.is_empty())
        .or_else(|| database.filter(|d| !d.is_empty()).map(str::to_string));
    let Some(db) = resolved else {
        return DispatchOutcome::Err(err!("error.clickhouseMutationTargetUnknown"));
    };
    match run_mutation_and_wait(conn, &db, table, command_sql).await {
        Ok(()) => DispatchOutcome::Ok,
        Err(e) => DispatchOutcome::Err(e),
    }
}

/// Sends one statement the way its own shape asks for — D1 of
/// `docs/specs/2026-09-04-clickhouse-query-dml-design.md`. `database` is the Query
/// tab's own active database, used when the statement itself does not qualify the table.
async fn dispatch_statement(
    conn: &Connection,
    statement: &Statement,
    database: Option<&str>,
) -> DispatchOutcome {
    match statement.verb.as_str() {
        "INSERT" => match execute_with_written_rows(conn, &statement.text, database).await {
            Ok(written) => DispatchOutcome::Affected(written),
            Err(e) => DispatchOutcome::Err(e),
        },
        "TRUNCATE" => match execute_check(conn, &statement.text, database).await {
            Ok(()) => DispatchOutcome::Ok,
            Err(e) => DispatchOutcome::Err(e),
        },
        "DELETE" => match delete_from_target(&statement.text) {
            Some((qualifier, table)) => {
                run_as_mutation(conn, qualifier, &table, database, &statement.text).await
            }
            None => match query_in_database(conn, &statement.text, database).await {
                Ok(result) => DispatchOutcome::Rows(result),
                Err(e) => DispatchOutcome::Err(e),
            },
        },
        "ALTER" => match alter_table_update_target(&statement.text) {
            Some((qualifier, table)) => {
                run_as_mutation(conn, qualifier, &table, database, &statement.text).await
            }
            None => match query_in_database(conn, &statement.text, database).await {
                Ok(result) => DispatchOutcome::Rows(result),
                Err(e) => DispatchOutcome::Err(e),
            },
        },
        _ => match query_in_database(conn, &statement.text, database).await {
            Ok(result) => DispatchOutcome::Rows(result),
            Err(e) => DispatchOutcome::Err(e),
        },
    }
}

/// Runs a script, statement by statement, each on its own request — see the module doc for why.
///
/// `INSERT`, `TRUNCATE`, `ALTER TABLE ... UPDATE ... WHERE` and `DELETE FROM ... WHERE` are sent
/// through [`dispatch_statement`] rather than `query_in_database` — see
/// `docs/specs/2026-09-04-clickhouse-query-dml-design.md`'s D1. Everything else keeps
/// the original `"rows"`/`"ok"` shape. A failed statement stops the script, the way it would in
/// `clickhouse-client`.
pub async fn run(
    conn: &Connection,
    sql: &str,
    database: Option<&str>,
) -> Result<Vec<StatementResult>, AppError> {
    let statements = split_statements(sql);
    if statements.is_empty() {
        return Err(err!("error.nothingToRun"));
    }

    let mut results: Vec<StatementResult> = Vec::new();

    for statement in statements {
        let started = Instant::now();
        let outcome = dispatch_statement(conn, &statement, database).await;

        let (columns, rows, truncated, rows_affected, kind, failure) = match outcome {
            DispatchOutcome::Rows(result) => {
                let columns: Vec<String> = result
                    .data
                    .first()
                    .map(|row| row.keys().cloned().collect())
                    .unwrap_or_default();
                let truncated = result.data.len() > MAX_ROWS;
                let rows: Vec<Vec<Value>> = result
                    .data
                    .iter()
                    .take(MAX_ROWS)
                    .map(|row| row_to_columns(row, &columns))
                    .collect();
                let kind = if columns.is_empty() { "ok" } else { "rows" };
                (columns, rows, truncated, 0u64, kind, None)
            }
            DispatchOutcome::Affected(written) => {
                (Vec::new(), Vec::new(), false, written, "affected", None)
            }
            DispatchOutcome::Ok => (Vec::new(), Vec::new(), false, 0u64, "ok", None),
            // `message` rather than `e.to_string()`: the latter is `AppError`'s own debug form
            // ("error.clickhouse message=…"), meant for a log line — this field is what the Query
            // tab shows beside the statement, and what belongs there is the server's own words.
            DispatchOutcome::Err(e) => (
                Vec::new(),
                Vec::new(),
                false,
                0u64,
                "error",
                Some(server_message(&e)),
            ),
        };

        let failed = failure.is_some();
        results.push(StatementResult {
            statement: statement.text,
            verb: statement.verb,
            kind: kind.to_string(),
            columns,
            rows,
            truncated,
            rows_affected,
            last_insert_id: None,
            duration_ms: started.elapsed().as_millis() as u64,
            error: failure,
        });
        if failed {
            break;
        }
    }

    Ok(results)
}

/// Asks ClickHouse what it makes of one statement, without running it — `EXPLAIN AST` parses the
/// statement and throws the result away without executing anything past that, which is what makes
/// this safe to fire at a half-typed statement.
///
/// Only a parse failure is reported: `EXPLAIN AST` has nothing to say about a name that does not
/// exist, since resolving names is not part of what it does. That leaves nothing to report as a
/// `"warning"` the way the other three engines can — everything this returns is `"error"`, or
/// `None`.
///
/// Goes through [`execute_check`] rather than [`query_in_database`], and has to: `EXPLAIN AST`'s
/// own output is a plain-text tree, and `FORMAT JSON` appended after the statement is parsed as
/// part of *what is being explained* rather than as a format for the explanation — see that
/// function's own doc for what that looks like when it goes wrong.
pub async fn validate(
    conn: &Connection,
    sql: &str,
    database: Option<&str>,
) -> Result<Option<SqlProblem>, AppError> {
    if sql.trim().is_empty() {
        return Ok(None);
    }
    match execute_check(conn, &format!("EXPLAIN AST {sql}"), database).await {
        Ok(()) => Ok(None),
        Err(e) => Ok(Some(SqlProblem {
            message: server_message(&e),
            number: 0,
            line: None,
            severity: "error".to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(sql: &str) -> Vec<(String, String)> {
        split_statements(sql)
            .into_iter()
            .map(|s| (s.verb, s.text))
            .collect()
    }

    /// Runs `sql` through `Scanner`, split into `chunk_size`-character pieces fed one chunk's worth
    /// of `feed()` calls at a time — the same shape the restore reader in `clickhouse_dump.rs` drives
    /// it in, minus the file I/O. Returns the statement texts it produced.
    fn split_in_chunks(sql: &str, chunk_size: usize) -> Vec<String> {
        let mut scanner = Scanner::new();
        let mut current = String::new();
        let mut out = Vec::new();
        for chunk in sql.chars().collect::<Vec<_>>().chunks(chunk_size.max(1)) {
            for &c in chunk {
                match scanner.feed(c) {
                    Fed::More => current.push(c),
                    Fed::End => {
                        let text = current.trim().to_string();
                        current.clear();
                        if !scanner.verb().is_empty() {
                            out.push(text);
                        }
                        scanner.reset();
                    }
                }
            }
        }
        let tail = current.trim().to_string();
        if !scanner.verb().is_empty() {
            out.push(tail);
        }
        out
    }

    #[test]
    fn chunking_never_changes_the_statements_split_produces() {
        let sql = "SELECT 1 -- a comment with ; in it\n; \
                    SELECT 'it''s a semicolon: ;' FROM t; \
                    SELECT \"a`b\" /* nested /* comment ; */ still comment */ FROM u; \
                    SELECT 'backslash \\' then quote''s here';";
        let whole: Vec<String> = split(sql).into_iter().map(|(_, text)| text).collect();
        for chunk_size in [1, 2, 3, 5, 7, 16, 64] {
            assert_eq!(
                split_in_chunks(sql, chunk_size),
                whole,
                "chunk_size={chunk_size}"
            );
        }
    }

    #[test]
    fn a_chunk_boundary_inside_a_doubled_single_quote_still_resolves() {
        // `''` is one escaped quote inside the string — a boundary landing exactly between the two
        // `'` characters is the one-character-lookahead case `MaybeDoubledSingleQuote` exists for.
        let sql = "SELECT 'it''s fine';";
        let whole: Vec<String> = split(sql).into_iter().map(|(_, text)| text).collect();
        assert_eq!(split_in_chunks(sql, 9), whole); // "SELECT 'i" | "t''s fine';" — splits inside `''`
    }

    #[test]
    fn a_chunk_boundary_inside_a_backslash_escape_still_resolves() {
        let sql = r"SELECT 'a\'b';";
        let whole: Vec<String> = split(sql).into_iter().map(|(_, text)| text).collect();
        assert_eq!(split_in_chunks(sql, 10), whole); // splits right after the `\`
    }

    #[test]
    fn alter_table_update_target_reads_an_unqualified_table() {
        assert_eq!(
            alter_table_update_target("ALTER TABLE t UPDATE x = 1 WHERE id = 2"),
            Some((None, "t".to_string()))
        );
    }

    #[test]
    fn alter_table_update_target_reads_a_qualified_table() {
        assert_eq!(
            alter_table_update_target("ALTER TABLE mydb.t UPDATE x = 1"),
            Some((Some("mydb".to_string()), "t".to_string()))
        );
    }

    #[test]
    fn alter_table_update_target_understands_backtick_and_double_quote() {
        assert_eq!(
            alter_table_update_target("ALTER TABLE `my db`.`my table` UPDATE x = 1"),
            Some((Some("my db".to_string()), "my table".to_string()))
        );
        assert_eq!(
            alter_table_update_target(r#"ALTER TABLE "t" UPDATE x = 1"#),
            Some((None, "t".to_string()))
        );
    }

    #[test]
    fn alter_table_update_target_refuses_more_than_one_command() {
        assert_eq!(
            alter_table_update_target("ALTER TABLE t DROP COLUMN x, UPDATE y = 1 WHERE z = 2"),
            None
        );
    }

    #[test]
    fn alter_table_update_target_refuses_a_plain_drop_column() {
        assert_eq!(
            alter_table_update_target("ALTER TABLE t DROP COLUMN x"),
            None
        );
    }

    #[test]
    fn alter_table_update_target_refuses_alter_delete() {
        // D2 of the design: only DELETE FROM...WHERE is supported, not this older mutation spelling.
        assert_eq!(
            alter_table_update_target("ALTER TABLE t DELETE WHERE id = 1"),
            None
        );
    }

    #[test]
    fn alter_table_update_target_skips_comments() {
        assert_eq!(
            alter_table_update_target(
                "ALTER TABLE /* c */ t UPDATE x = 1 -- trailing\nWHERE id = 1"
            ),
            Some((None, "t".to_string()))
        );
    }

    #[test]
    fn delete_from_target_reads_the_table() {
        assert_eq!(
            delete_from_target("DELETE FROM t WHERE id = 1"),
            Some((None, "t".to_string()))
        );
        assert_eq!(
            delete_from_target("DELETE FROM mydb.t WHERE id = 1"),
            Some((Some("mydb".to_string()), "t".to_string()))
        );
    }

    #[test]
    fn delete_from_target_refuses_a_shape_with_no_from() {
        assert_eq!(delete_from_target("DELETE"), None);
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_split() {
        let statements = split("select 'a;b'; select 2");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].1, "select 'a;b'");
    }

    #[test]
    fn a_doubled_quote_and_a_backslash_both_escape() {
        assert_eq!(split("select 'it''s; here'; select 2").len(), 2);
        assert_eq!(split(r"select 'it\'s; here'; select 2").len(), 2);
    }

    #[test]
    fn both_identifier_quotes_are_understood() {
        assert_eq!(split(r#"select "a;b" from t; select 2"#).len(), 2);
        assert_eq!(split("select `a;b` from t; select 2").len(), 2);
    }

    /// Checked against the server: `SELECT 5--3` is `5`, not `2` — no space is needed to open the
    /// comment, unlike MySQL.
    #[test]
    fn a_dash_comment_needs_no_space_after_it() {
        let statements = split("select 1 --3;\nselect 2");
        assert_eq!(statements.len(), 1);
    }

    /// Checked against the server: a comment inside a comment does not end the outer one early.
    #[test]
    fn a_block_comment_nests() {
        let statements = split("select /* a /* b */ 1 */ ; select 2");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].1, "select /* a /* b */ 1 */");
    }

    #[test]
    fn a_hash_comment_runs_to_the_end_of_the_line() {
        let statements = split("select 1 # a;b\n; select 2");
        assert_eq!(statements.len(), 2);
    }

    #[test]
    fn comments_are_kept_and_a_run_of_them_alone_is_not_a_statement() {
        assert!(split("-- nothing here\n").is_empty());
        assert!(split("  ;  ; ").is_empty());
        let statements = split("/* keep me */ select 1");
        assert_eq!(statements[0].1, "/* keep me */ select 1");
    }

    #[test]
    fn the_verb_is_the_opening_word() {
        let statements = split("  select 1 from t");
        assert_eq!(statements[0].0, "SELECT");
    }

    /// The Query tab shows this beside the statement — it has to be the server's own sentence, not
    /// `AppError`'s `code`+`params` debug form.
    #[test]
    fn server_message_reads_the_driver_s_words_not_the_error_code() {
        let error = err!("error.clickhouse", message = "Code: 62. Syntax error");
        assert_eq!(server_message(&error), "Code: 62. Syntax error");
    }

    /// A code carrying no `message` param falls back to its own `Display` rather than panicking or
    /// silently losing what went wrong.
    #[test]
    fn server_message_falls_back_when_there_is_no_message_param() {
        let error = err!("error.nothingToRun");
        assert_eq!(server_message(&error), "error.nothingToRun");
    }
}
