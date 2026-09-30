//! Dumping and restoring a ClickHouse database over its HTTP interface — no `clickhouse-client`,
//! no child process. See `docs/specs/2026-09-04-clickhouse-dump-restore-design.md` for
//! the decisions this implements (referenced as D1..D9 below).

use super::clickhouse::{self, Connection};
use super::clickhouse_script::{Fed, Scanner};
use super::dump::{self, Tracker};
use crate::error::AppError;

/// Removes every `database.` qualifier from `sql` that stands immediately before an identifier —
/// the leading name of the table/view being created, and every reference to the same database
/// inside a `VIEW`/`MATERIALIZED VIEW`'s `AS SELECT`/`TO` clause (D4). Leaves every other
/// occurrence of `database` untouched: inside a single-quoted string, and specifically the database
/// argument of `ENGINE = Distributed('cluster', 'database', 'table')` — that one is a data value,
/// not a scoped reference, and rewriting it would point the proxy somewhere it never meant to go
/// (D4's documented exception).
///
/// Whole-string rather than built on [`Scanner`]: this runs once per table at dump time on a single
/// `SHOW CREATE TABLE` result already held in memory in full — there is no file to read in chunks,
/// so `Scanner`'s resumability buys nothing here. It uses the same quote/comment rules by hand
/// instead (checked against `split_statements`' own doc for where each was verified against the
/// test server).
pub(super) fn strip_database_qualifiers(sql: &str, database: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '#' || (c == '-' && chars.get(i + 1) == Some(&'-')) {
            while i < chars.len() && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            out.push('/');
            out.push('*');
            i += 2;
            let mut depth = 1u32;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    out.push('/');
                    out.push('*');
                    i += 2;
                    depth += 1;
                    continue;
                }
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    out.push('*');
                    out.push('/');
                    i += 2;
                    depth -= 1;
                    continue;
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // A single-quoted string is copied verbatim — this is the position `Distributed(...)` puts
        // the database name in, and D4 says it must not be touched.
        if c == '\'' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                out.push(ch);
                i += 1;
                if ch == '\\' && i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                    continue;
                }
                if ch == '\'' {
                    if chars.get(i) == Some(&'\'') {
                        out.push('\'');
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }

        // A backtick- or double-quoted identifier: dropped along with a following `.` if it names
        // `database` exactly, copied through untouched otherwise.
        if c == '`' || c == '"' {
            let start = i;
            let quote = c;
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() {
                    i += 2;
                    continue;
                }
                i += 1;
                if ch == quote {
                    break;
                }
            }
            let raw: String = chars[start..i].iter().collect();
            let inner = &raw[1..raw.len().saturating_sub(1)];
            if inner == database && chars.get(i) == Some(&'.') {
                i += 1;
            } else {
                out.push_str(&raw);
            }
            continue;
        }

        // A bare (unquoted) identifier — the same check without quotes. Checked against the test
        // server: `SHOW CREATE TABLE` only backtick-quotes a *column* name, not the table/database
        // name itself when it needs no quoting — `CREATE TABLE mixlab_agent_test.events (...)`, no
        // backticks around either half. This branch is the common case, not a defensive fallback.
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if word == database && chars.get(i) == Some(&'.') {
                i += 1;
            } else {
                out.push_str(&word);
            }
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}

/// Table engines whose data a dump never exports (D9): either they carry no storage of their own
/// (`View`, `MaterializedView` — its rows live in a hidden `.inner_id` table this app does not
/// reach), or exporting through them has a side effect or is meaningless as a backup
/// (`Kafka`/`RabbitMQ`/`NATS`/`FileLog` consume a queue by reading it; `Distributed`/`MySQL`/
/// `PostgreSQL`/`S3`/`URL`/`HDFS`/`ODBC`/`JDBC`/`ExternalDistributed` proxy data that lives
/// somewhere else entirely). Structure is still dumped for all of these — see `dump_structure`.
///
/// An exclude list, not a whitelist: an engine not named here is still tried, so a valid storage
/// engine this list has not caught up with is not silently skipped.
const DATA_DUMP_EXCLUDED_ENGINES: &[&str] = &[
    "View",
    "MaterializedView",
    "Distributed",
    "Kafka",
    "RabbitMQ",
    "NATS",
    "FileLog",
    "MySQL",
    "PostgreSQL",
    "S3",
    "URL",
    "HDFS",
    "ODBC",
    "JDBC",
    "ExternalDistributed",
];

pub(super) fn excluded_from_data_dump(engine: &str) -> bool {
    DATA_DUMP_EXCLUDED_ENGINES.contains(&engine)
}

/// One table or view of a database, as `system.tables` reports it — the shared read `dump_structure`
/// and `dump_data` both start from.
struct TableInfo {
    name: String,
    engine: String,
    total_bytes: u64,
}

async fn list_tables_with_engine(
    conn: &Connection,
    database: &str,
) -> Result<Vec<TableInfo>, AppError> {
    let result = clickhouse::query_with_params(
        conn,
        "SELECT name, engine, total_bytes FROM system.tables \
         WHERE database = {database:String} ORDER BY name",
        &[("database".to_string(), database.to_string())],
    )
    .await?;
    Ok(result
        .data
        .iter()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?.to_string();
            let engine = row.get("engine")?.as_str()?.to_string();
            let total_bytes = clickhouse::as_u64(row.get("total_bytes")).unwrap_or(0);
            Some(TableInfo {
                name,
                engine,
                total_bytes,
            })
        })
        .collect())
}

/// Tables before views/materialized views — an index or a trigger replayed before its table fails
/// on the other engines, and the same reasoning applies here: a `MATERIALIZED VIEW`'s `TO` table
/// (or a plain `VIEW`'s `AS SELECT`) should exist before the view naming it is created. Stable sort:
/// ties (both within the "table" group and within the "view" group) keep the `ORDER BY name` order
/// the query already gave.
fn ordered_for_structure(mut tables: Vec<TableInfo>) -> Vec<TableInfo> {
    tables.sort_by_key(|t| matches!(t.engine.as_str(), "View" | "MaterializedView"));
    tables
}

async fn show_create(conn: &Connection, database: &str, table: &str) -> Result<String, AppError> {
    let sql = format!(
        "SHOW CREATE TABLE {}",
        clickhouse::qualified(database, table)
    );
    let result = clickhouse::query_in_database(conn, &sql, Some(database)).await?;
    result
        .data
        .first()
        .and_then(|row| row.values().next())
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            err!(
                "error.clickhouse",
                message = format!("SHOW CREATE TABLE returned nothing for {table}")
            )
        })
}

/// Writes every table/view's `DROP TABLE IF EXISTS` + (database-qualifier-stripped, D4)
/// `SHOW CREATE TABLE` to `path`, tables before views (see [`ordered_for_structure`]). Creates the
/// file fresh — a caller doing a `data`-only dump never calls this, and an `all` dump's
/// [`dump_data`] appends after it.
pub async fn dump_structure(
    conn: &Connection,
    database: &str,
    path: &str,
    watch: &dump::Watch<'_>,
) -> Result<(), AppError> {
    use std::io::Write;

    let tables = ordered_for_structure(list_tables_with_engine(conn, database).await?);
    let weights: Vec<(String, u64)> = tables.iter().map(|t| (t.name.clone(), 1)).collect();
    let mut tracker = Tracker::new(&weights, path, false);

    let mut file = std::fs::File::create(path)
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
    write!(file, "-- MixLab structure dump\n\n")
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;

    for table in &tables {
        if (watch.cancel)() {
            return Err(err!("error.transferCancelled", tool = "ClickHouse dump"));
        }
        let create_sql = show_create(conn, database, &table.name).await?;
        let stripped = strip_database_qualifiers(&create_sql, database);
        write!(
            file,
            "DROP TABLE IF EXISTS {};\n{stripped};\n\n",
            clickhouse::quote_ident(&table.name)
        )
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;

        tracker.reached(&table.name);
        (watch.report)(tracker.progress());
    }
    Ok(())
}

/// Streams `SELECT * FROM t FORMAT SQLInsert` for every table not in [`excluded_from_data_dump`]
/// (D9) straight into `path`, never buffering a whole response in memory (D5) — `FORMAT SQLInsert`
/// is ClickHouse's own SQL-generating output format, so what arrives on the wire is already valid
/// `INSERT` text.
///
/// `append`: `true` when this follows [`dump_structure`] into the same file (an `all`-mode dump),
/// `false` for a `data`-only dump, which owns the file from scratch.
pub async fn dump_data(
    conn: &Connection,
    database: &str,
    path: &str,
    append: bool,
    watch: &dump::Watch<'_>,
) -> Result<(), AppError> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let tables: Vec<TableInfo> = list_tables_with_engine(conn, database)
        .await?
        .into_iter()
        .filter(|t| !excluded_from_data_dump(&t.engine))
        .collect();
    let weights: Vec<(String, u64)> = tables
        .iter()
        .map(|t| (t.name.clone(), t.total_bytes.max(1)))
        .collect();
    let mut tracker = Tracker::new(&weights, path, true);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .await
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;

    for table in &tables {
        tracker.reached(&table.name);
        // `output_format_sql_insert_table_name` has to be set explicitly — left at its default,
        // `FORMAT SQLInsert` writes the literal placeholder `INSERT INTO table (...)` rather than
        // the table's real name (checked against the test server). The bare, unquoted-by-`quote_
        // ident` name matches what `strip_database_qualifiers` leaves the structure dump's own
        // `CREATE TABLE` statements with, so both halves of an `all`-mode dump agree on how a table
        // is named once the database qualifier is gone.
        let sql = format!(
            "SELECT * FROM {} FORMAT SQLInsert SETTINGS output_format_sql_insert_table_name = {}",
            clickhouse::qualified(database, &table.name),
            clickhouse::quote_literal(&table.name)
        );
        let response = clickhouse::query_streaming(conn, &sql, Some(database)).await?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if (watch.cancel)() {
                return Err(err!("error.transferCancelled", tool = "ClickHouse dump"));
            }
            let bytes = chunk.map_err(|e| err!("error.clickhouse", message = e))?;
            file.write_all(&bytes)
                .await
                .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
            (watch.report)(tracker.progress());
        }
    }
    // `write_all` on a tokio file returns once the bytes are handed to a blocking thread, so the
    // last write is still in flight here — and its error unseen — until it is flushed.
    file.flush()
        .await
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))
}

/// Replays a dump file into `database`, one statement at a time, without holding more of the file
/// in memory than the largest single statement (D7) — typically one `FORMAT SQLInsert` batch, a few
/// MB at most, not the whole file. Reads the file in 64KB chunks, decodes as much valid UTF-8 as
/// each growing buffer holds (keeping only a genuinely incomplete multi-byte tail for the next
/// read), and feeds the decoded characters to a [`Scanner`] — the same one `split_statements`
/// drives, so a chunk boundary landing inside a quote or a comment cannot be mistaken for the end of
/// a statement.
///
/// Every statement runs through `execute_check`, scoped to `database` via the same `?database=`
/// mechanism the dump's stripped-qualifier statements rely on (D4) — stops at the first one that
/// fails, reporting that statement and the server's own words, like `sqlite_dump::restore`.
pub async fn restore(
    conn: &Connection,
    database: &str,
    path: &str,
    watch: &dump::Watch<'_>,
) -> Result<(), AppError> {
    use std::io::Read;

    let file = std::fs::File::open(path)
        .map_err(|e| err!("error.cannotReadFile", path = path, message = e))?;
    let total = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut reader = std::io::BufReader::new(file);

    let mut buf: Vec<u8> = Vec::new();
    let mut read_buf = [0u8; 64 * 1024];
    let mut scanner = Scanner::new();
    let mut current = String::new();
    let mut sent: u64 = 0;
    let mut eof = false;

    loop {
        if (watch.cancel)() {
            return Err(err!("error.transferCancelled", tool = "ClickHouse restore"));
        }

        let valid_up_to = match std::str::from_utf8(&buf) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        let text = std::str::from_utf8(&buf[..valid_up_to])
            .expect("valid_up_to always points at a char boundary")
            .to_string();
        for c in text.chars() {
            sent += c.len_utf8() as u64;
            match scanner.feed(c) {
                Fed::More => current.push(c),
                Fed::End => {
                    let statement = current.trim().to_string();
                    current.clear();
                    scanner.reset();
                    if !statement.is_empty() {
                        clickhouse::execute_check(conn, &statement, Some(database)).await?;
                    }
                    (watch.report)(dump::Progress {
                        percent: (total > 0)
                            .then(|| ((sent as f64 / total as f64) * 100.0).min(99.0) as u8),
                        ..dump::Progress::default()
                    });
                }
            }
        }
        buf.drain(..valid_up_to);

        if eof && buf.is_empty() {
            break;
        }
        if eof {
            return Err(err!(
                "error.cannotReadFile",
                path = path,
                message = "invalid UTF-8"
            ));
        }

        let read = reader
            .read(&mut read_buf)
            .map_err(|e| err!("error.cannotReadFile", path = path, message = e))?;
        if read == 0 {
            eof = true;
        } else {
            buf.extend_from_slice(&read_buf[..read]);
        }
    }

    let tail = current.trim().to_string();
    if !tail.is_empty() {
        clickhouse::execute_check(conn, &tail, Some(database)).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_every_named_engine() {
        for engine in DATA_DUMP_EXCLUDED_ENGINES {
            assert!(excluded_from_data_dump(engine), "{engine}");
        }
    }

    #[test]
    fn does_not_exclude_an_unlisted_engine() {
        assert!(!excluded_from_data_dump("MergeTree"));
        assert!(!excluded_from_data_dump("SomeFutureEngine"));
    }

    #[test]
    fn strips_the_leading_qualifier_of_a_table() {
        assert_eq!(
            strip_database_qualifiers("CREATE TABLE `db`.`t` (`a` Int32) ENGINE = MergeTree", "db"),
            "CREATE TABLE `t` (`a` Int32) ENGINE = MergeTree"
        );
    }

    #[test]
    fn strips_every_reference_in_a_view() {
        assert_eq!(
            strip_database_qualifiers("CREATE VIEW `db`.`v` AS SELECT * FROM `db`.`t`", "db"),
            "CREATE VIEW `v` AS SELECT * FROM `t`"
        );
    }

    #[test]
    fn strips_every_reference_in_a_materialized_view_including_to() {
        assert_eq!(
            strip_database_qualifiers(
                "CREATE MATERIALIZED VIEW `db`.`mv` TO `db`.`target` AS SELECT * FROM `db`.`source`",
                "db"
            ),
            "CREATE MATERIALIZED VIEW `mv` TO `target` AS SELECT * FROM `source`"
        );
    }

    #[test]
    fn leaves_the_database_name_alone_inside_a_string_literal() {
        assert_eq!(
            strip_database_qualifiers(
                "CREATE TABLE `db`.`t` (`a` String DEFAULT 'db') ENGINE = X",
                "db"
            ),
            "CREATE TABLE `t` (`a` String DEFAULT 'db') ENGINE = X"
        );
    }

    #[test]
    fn leaves_a_distributed_engines_database_argument_alone() {
        let sql = "CREATE TABLE `db`.`d` AS `db`.`local` \
                    ENGINE = Distributed('cluster', 'db', 'local')";
        assert_eq!(
            strip_database_qualifiers(sql, "db"),
            "CREATE TABLE `d` AS `local` ENGINE = Distributed('cluster', 'db', 'local')"
        );
    }

    #[test]
    fn a_different_database_named_the_same_word_as_a_column_value_is_not_touched() {
        // Sanity check on the string-literal test above: an unrelated database name is never a
        // false-positive match target either.
        assert_eq!(
            strip_database_qualifiers("CREATE TABLE `other`.`t` (`a` Int32)", "db"),
            "CREATE TABLE `other`.`t` (`a` Int32)"
        );
    }
}
