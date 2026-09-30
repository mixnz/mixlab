//! Running whatever SQL the user typed. Where the rest of the MySQL code builds statements from a
//! form, this module takes them as text: the Query tab's editor sends its whole contents, and what
//! comes back is one result per statement — a result set, a count of rows changed, or plain
//! confirmation that the statement ran.

use super::mysql::{column_value, map_error, quote_ident};
use crate::error::AppError;
use crate::modules::db::models::{SqlProblem, StatementResult};
use futures_util::StreamExt;
use serde_json::Value;
use sqlx::{Column, Either, MySqlPool, Row};
use std::time::Instant;

/// How many rows of one result set are read back. A query without a LIMIT can name more rows than
/// there is memory for, so the client stops here and says that it did.
///
/// Ten thousand rather than the thousand this was: the results grid holds only the rows on screen
/// now, so the cost of a large set is what it takes to decode and hand over — not what it takes to
/// draw. This is still a ceiling and not a promise; the auto-LIMIT in the Query tab is what most
/// scripts actually stop at.
const MAX_ROWS: usize = 10_000;

/// One statement carved out of the editor's text.
struct Statement {
    text: String,
    /// The keyword it opens with, upper-cased. Empty for a run of nothing but comments, which is
    /// how such a run is recognised as not being a statement at all.
    verb: String,
}

/// Splits a script into the statements that are to be sent one at a time.
///
/// Only a semicolon outside a string, a quoted identifier and a comment separates statements;
/// comments themselves are kept in the text, since a `/*+ hint */` or a `/*!50000 ... */` version
/// comment is part of what MySQL is being asked to run.
///
/// The client-side `DELIMITER` directive is not supported: a routine body whose `BEGIN ... END`
/// holds semicolons of its own has to be run as the single statement it is, not pasted in with the
/// `DELIMITER $$` wrapper a command-line client would want.
fn split_statements(sql: &str) -> Vec<Statement> {
    let chars: Vec<char> = sql.chars().collect();
    let mut statements: Vec<Statement> = Vec::new();
    let mut current = String::new();
    let mut verb = String::new();
    // Set once the opening word has ended, so that the words after it can't overwrite it.
    let mut verb_done = false;
    let mut i = 0;

    fn push(
        statements: &mut Vec<Statement>,
        current: &mut String,
        verb: &mut String,
        verb_done: &mut bool,
    ) {
        let text = current.trim().to_string();
        current.clear();
        *verb_done = false;
        let verb = std::mem::take(verb);
        // No opening keyword means there was no statement here — only whitespace, or a comment
        // sitting between two semicolons.
        if verb.is_empty() {
            return;
        }
        statements.push(Statement { text, verb });
    }

    while i < chars.len() {
        let c = chars[i];

        // `--` opens a comment only when whitespace (or the end of the text) follows it: `5--3`
        // is arithmetic, not a comment.
        if c == '-'
            && chars.get(i + 1) == Some(&'-')
            && matches!(
                chars.get(i + 2),
                None | Some(' ') | Some('\t') | Some('\n') | Some('\r')
            )
        {
            while i < chars.len() && chars[i] != '\n' {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            current.push('/');
            current.push('*');
            i += 2;
            while i < chars.len() {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    current.push('*');
                    current.push('/');
                    i += 2;
                    break;
                }
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if c == '\'' || c == '"' || c == '`' {
            current.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                current.push(ch);
                i += 1;
                // A backslash escapes the next character inside a string literal. Inside a
                // backtick-quoted identifier it does not — there, doubling is the only escape.
                if ch == '\\' && c != '`' {
                    if let Some(&next) = chars.get(i) {
                        current.push(next);
                        i += 1;
                    }
                    continue;
                }
                if ch == c {
                    // Two of the quote in a row are an escaped quote, not the end of the literal.
                    if chars.get(i) == Some(&c) {
                        current.push(c);
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }

        if c == ';' {
            push(&mut statements, &mut current, &mut verb, &mut verb_done);
            i += 1;
            continue;
        }

        // Plain code: the first word of it is the statement's keyword.
        if !verb_done {
            if c.is_alphanumeric() || c == '_' {
                verb.extend(c.to_uppercase());
            } else if !verb.is_empty() {
                verb_done = true;
            }
        }
        current.push(c);
        i += 1;
    }

    push(&mut statements, &mut current, &mut verb, &mut verb_done);
    statements
}

/// The statement text could not be parsed. The one thing a checker can be sure is wrong.
const ER_PARSE_ERROR: u16 = 1064;
/// "This statement kind cannot be prepared" — `USE`, `SHOW BINLOG EVENTS`, and a long tail of
/// others. It says nothing about whether the statement is valid.
const ER_UNSUPPORTED_PS: u16 = 1295;
/// The user may not do this. Also says nothing about the text: the statement is well-formed, this
/// login simply may not run it, and running it is not what was asked for.
const ACCESS_DENIED: [u16; 6] = [1044, 1045, 1142, 1143, 1227, 1370];

/// The line MySQL named, out of `... near 'x' at line 3`. Anchoring a diagnostic anywhere else
/// would put the squiggle under text the server never complained about.
fn error_line(message: &str) -> Option<u32> {
    let at = message.rfind("at line ")?;
    message[at + "at line ".len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// Turns a failed `PREPARE` into something worth showing, or into nothing.
fn problem(error: &sqlx::Error) -> Option<SqlProblem> {
    let db = error.as_database_error()?;
    let number = db
        .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
        .map_or(0, |e| e.number());
    if number == ER_UNSUPPORTED_PS || ACCESS_DENIED.contains(&number) {
        return None;
    }
    let message = db.message().to_string();
    Some(SqlProblem {
        line: error_line(&message),
        severity: if number == ER_PARSE_ERROR {
            "error"
        } else {
            "warning"
        }
        .to_string(),
        message,
        number,
    })
}

/// Asks MySQL what it makes of one statement, **without running it**.
///
/// `PREPARE` parses and plans; `DEALLOCATE` throws the plan away. Nothing in between executes, so
/// this is safe to fire at a half-typed `DELETE`. `PREPARE` will not take a placeholder for the
/// text it prepares — its argument has to be a string literal or a user variable — so the text goes
/// into a user variable first, which *can* be bound, and no user text is ever interpolated into
/// SQL.
///
/// This runs on a pooled connection rather than on the session the script runs on, and that is the
/// whole reason most of what comes back is a warning rather than an error. A temporary table, a
/// `USE`, a `SET` from earlier in the script — none of it is visible from here, so "table doesn't
/// exist" may well mean "not yet". Only the server refusing to parse the text at all is certain.
///
/// Unlike {@link run} this does keep the connection: it fires on a debounce while someone types,
/// and a handshake per pause would cost more than it saves. What it leaves on the session is
/// bounded and known — the `USE`, and `@mixlab_check` holding the last statement's text — and no
/// query in the app reads either: everything else names its database in full or binds it as a
/// parameter to `information_schema`.
pub async fn validate(
    pool: &MySqlPool,
    sql: &str,
    database: Option<&str>,
) -> Result<Option<SqlProblem>, AppError> {
    if sql.trim().is_empty() {
        return Ok(None);
    }

    let mut conn = pool.acquire().await.map_err(map_error)?;
    if let Some(db) = database.filter(|d| !d.is_empty()) {
        // Sent as text, since `USE` is one of the statements the prepared protocol refuses. A
        // database that cannot be entered ends the check rather than failing it: whatever is wrong
        // is wrong with the header, not with the statement being asked about.
        if sqlx::raw_sql(sqlx::AssertSqlSafe(format!("USE {}", quote_ident(db))))
            .execute(&mut *conn)
            .await
            .is_err()
        {
            return Ok(None);
        }
    }

    sqlx::query("SET @mixlab_check = ?")
        .bind(sql)
        .execute(&mut *conn)
        .await
        .map_err(map_error)?;

    match sqlx::raw_sql("PREPARE mixlab_check FROM @mixlab_check")
        .execute(&mut *conn)
        .await
    {
        Ok(_) => {
            // Ignored on purpose: the plan is gone when the connection goes back to the pool
            // either way, and a failure to tidy up is not something to report as a problem with
            // the user's statement.
            let _ = sqlx::raw_sql("DEALLOCATE PREPARE mixlab_check")
                .execute(&mut *conn)
                .await;
            Ok(None)
        }
        Err(e) => Ok(problem(&e)),
    }
}

/// The statements whose point is the number of rows they changed, so that a count of zero is still
/// worth reporting as a count — `UPDATE ... 0 rows` says something, `SET @x = 1 ... 0 rows` does
/// not.
fn is_write_verb(verb: &str) -> bool {
    matches!(
        verb,
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "LOAD" | "TRUNCATE"
    )
}

/// Runs the editor's text against `database`, statement by statement, and reports each one's
/// outcome.
///
/// Everything runs on one connection, so a `USE`, a `SET`, a temporary table or a transaction
/// opened by one statement is still in force for the next — a script reads the way it would in a
/// command-line client. That connection is the script's own and is closed when the script ends, so
/// nothing it left behind is ever seen by the queries the rest of the app runs; the other side of
/// that is that one Run press starts a session and the next one starts another.
///
/// A statement that fails stops the script: its own result carries the error, and the results
/// before it are still returned rather than being lost with it.
/// `announce` is handed the session's thread id once, before the first statement runs. That is
/// what makes the script interruptible: the id is the only handle another connection has on it,
/// and `mysql::kill_query` needs it to stop a statement that is still going.
pub async fn run(
    pool: &MySqlPool,
    sql: &str,
    database: Option<&str>,
    announce: impl FnOnce(u64),
) -> Result<Vec<StatementResult>, AppError> {
    let statements = split_statements(sql);
    if statements.is_empty() {
        return Err(err!("error.nothingToRun"));
    }

    let mut conn = pool.acquire().await.map_err(map_error)?;
    // The script gets this connection to itself, and it is closed when the script is done rather
    // than going back to the pool carrying whatever the script left on it: an open `BEGIN`, a `USE`
    // of another database, `SET autocommit = 0`, `LOCK TABLES`, a temporary table. sqlx only pings
    // a returned connection and every one of those survives a ping — the next `list_tables` to
    // borrow it would read the wrong database, and an uncommitted `UPDATE` would hold its row locks
    // on an idle connection until some later `pool.begin()` implicitly committed it. MySQL has no
    // statement that resets a session, so the only way to be sure is not to hand the session on.
    //
    // On drop rather than at the end: an error on the way in, or the whole run being dropped
    // because the tab closed, has to close it too. The pool's permit is held until it does, so
    // this never opens a sixth connection. The cost is one handshake per Run, which is a keypress.
    conn.close_on_drop();
    announce(super::mysql::thread_id(&mut conn).await?);
    if let Some(db) = database.filter(|d| !d.is_empty()) {
        // Sent as text, not prepared: MySQL refuses `USE` in the prepared statement protocol
        // (error 1295), and the whole script would fail before its first statement ran.
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("USE {}", quote_ident(db))))
            .execute(&mut *conn)
            .await
            .map_err(map_error)?;
    }

    let mut results: Vec<StatementResult> = Vec::new();

    for statement in statements {
        let started = Instant::now();
        let mut columns: Vec<String> = Vec::new();
        let mut rows: Vec<Vec<Value>> = Vec::new();
        let mut truncated = false;
        // How many results this one statement has already produced — a stored procedure can
        // return several result sets, and then one closing acknowledgement of its own.
        let mut produced = 0usize;
        let mut failure: Option<String> = None;

        // Scoped so the stream lets go of the connection before the next statement takes it.
        {
            let mut stream =
                sqlx::raw_sql(sqlx::AssertSqlSafe(statement.text.clone())).fetch_many(&mut *conn);
            while let Some(item) = stream.next().await {
                match item {
                    Ok(Either::Right(row)) => {
                        if columns.is_empty() {
                            columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                        }
                        if rows.len() < MAX_ROWS {
                            rows.push(
                                (0..row.columns().len())
                                    .map(|i| column_value(&row, i))
                                    .collect(),
                            );
                        } else {
                            // The rest of the set is still read off the wire — the connection has
                            // to be left where the next statement can use it — just not decoded.
                            truncated = true;
                        }
                    }
                    Ok(Either::Left(done)) => {
                        let has_result_set = !columns.is_empty() || !rows.is_empty();
                        if !has_result_set && produced > 0 && done.rows_affected() == 0 {
                            // The acknowledgement a procedure sends after its last result set. It
                            // says nothing the results already collected don't.
                            continue;
                        }
                        let last_insert_id = done.last_insert_id();
                        results.push(StatementResult {
                            statement: statement.text.clone(),
                            verb: statement.verb.clone(),
                            kind: if has_result_set {
                                "rows"
                            } else if is_write_verb(&statement.verb) || done.rows_affected() > 0 {
                                "affected"
                            } else {
                                "ok"
                            }
                            .to_string(),
                            columns: std::mem::take(&mut columns),
                            rows: std::mem::take(&mut rows),
                            truncated: std::mem::take(&mut truncated),
                            rows_affected: done.rows_affected(),
                            // Zero is MySQL's way of saying "this statement generated none".
                            last_insert_id: (last_insert_id != 0).then_some(last_insert_id),
                            duration_ms: started.elapsed().as_millis() as u64,
                            error: None,
                        });
                        produced += 1;
                    }
                    Err(e) => {
                        failure = Some(e.to_string());
                        break;
                    }
                }
            }
        }

        if let Some(error) = failure {
            results.push(StatementResult {
                statement: statement.text,
                verb: statement.verb,
                kind: "error".to_string(),
                columns: Vec::new(),
                rows: Vec::new(),
                truncated: false,
                rows_affected: 0,
                last_insert_id: None,
                duration_ms: started.elapsed().as_millis() as u64,
                error: Some(error),
            });
            // Nothing after a failed statement runs: the statements that follow it were written
            // expecting it to have happened.
            break;
        }
    }

    Ok(results)
}

/// What the splitter has to get right is where one statement ends: a semicolon inside a string, a
/// quoted identifier or a comment is text, not a separator, and sending the halves of a statement
/// separately is a syntax error at best and half an operation at worst.
#[cfg(test)]
mod tests {
    use super::{error_line, split_statements};

    /// Where the diagnostic gets anchored. MySQL puts the line at the end of the sentence, and a
    /// statement whose text happens to contain those words must not be read as the server's.
    #[test]
    fn the_line_is_read_off_the_end_of_the_server_message() {
        assert_eq!(
            error_line("You have an error in your SQL syntax; ... near 'x' at line 3"),
            Some(3)
        );
        // The last one wins: the quoted fragment can hold the phrase too.
        assert_eq!(error_line("... near 'at line 9' at line 2"), Some(2));
        assert_eq!(error_line("Table 'db.t' doesn't exist"), None);
        assert_eq!(error_line("at line "), None);
    }

    fn texts(sql: &str) -> Vec<String> {
        split_statements(sql).into_iter().map(|s| s.text).collect()
    }

    fn verbs(sql: &str) -> Vec<String> {
        split_statements(sql).into_iter().map(|s| s.verb).collect()
    }

    #[test]
    fn splits_on_semicolons_and_trims_each_statement() {
        assert_eq!(texts("SELECT 1;\n  SELECT 2 ;"), ["SELECT 1", "SELECT 2"]);
        // A script needs no trailing semicolon, and an empty one adds no statement.
        assert_eq!(texts("SELECT 1"), ["SELECT 1"]);
        assert_eq!(texts(";;\n;"), Vec::<String>::new());
    }

    #[test]
    fn the_verb_is_the_opening_keyword_upper_cased() {
        assert_eq!(verbs("select 1; delete from t"), ["SELECT", "DELETE"]);
        // Leading whitespace and a comment before the keyword don't become part of it.
        assert_eq!(verbs("  -- a note\n  insert into t values ()"), ["INSERT"]);
    }

    #[test]
    fn a_semicolon_inside_a_string_is_not_a_separator() {
        assert_eq!(
            texts("INSERT INTO t VALUES ('a;b'); SELECT 1"),
            ["INSERT INTO t VALUES ('a;b')", "SELECT 1"]
        );
        assert_eq!(texts(r#"SELECT "a;b""#), [r#"SELECT "a;b""#]);
        assert_eq!(texts("SELECT `we;ird`"), ["SELECT `we;ird`"]);
    }

    /// Both of MySQL's escapes inside a string literal: a backslash, and the quote doubled.
    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        assert_eq!(texts(r"SELECT 'a\'; b'"), [r"SELECT 'a\'; b'"]);
        assert_eq!(texts("SELECT 'a''; b'"), ["SELECT 'a''; b'"]);
    }

    /// Comments are kept in the text — a `/*! ... */` version comment or a `/*+ hint */` is part
    /// of what MySQL is being asked to run — but a semicolon inside one still separates nothing.
    #[test]
    fn comments_are_carried_along_and_hide_their_semicolons() {
        assert_eq!(
            texts("SELECT 1 -- one; two\n; SELECT 2"),
            ["SELECT 1 -- one; two", "SELECT 2"]
        );
        assert_eq!(texts("SELECT 1 # one; two"), ["SELECT 1 # one; two"]);
        assert_eq!(
            texts("/*!40101 SET x=1; */ SELECT 1"),
            ["/*!40101 SET x=1; */ SELECT 1"]
        );
        // Nothing but a comment is not a statement at all.
        assert_eq!(texts("-- just a note\n"), Vec::<String>::new());
    }

    /// `--` opens a comment only when whitespace follows it; `5--3` is arithmetic.
    #[test]
    fn two_dashes_without_whitespace_are_an_operator() {
        assert_eq!(texts("SELECT 5--3; SELECT 2"), ["SELECT 5--3", "SELECT 2"]);
    }
}
