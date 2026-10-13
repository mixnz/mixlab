//! Writing to a SQL Server table: updating, inserting and deleting rows, each in a
//! transaction of its own, and the binding and key matching those writes share. The
//! frontend's half is `src/modules/db/mssql/editing.ts`.

use super::mssql::{
    is_binary_type, map_error, quote_ident, resolve, table_columns, three_part, Connection, Pool,
    MONEY_TYPES,
};
use crate::error::AppError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Ends the transaction every write function below opens: commits on success, or rolls back and
/// keeps the original error on failure.
///
/// `tiberius` tracks a transaction only by what SQL was sent on this exact connection — there is no
/// value here whose `Drop` rolls back the way `sqlx::Transaction`'s does for a caller that forgets.
/// Funnelling every write's fallible body through here, rather than trusting a bare `?` between
/// `BEGIN` and `COMMIT` to leave the connection how it found it, is what stands in for that: a
/// connection handed back to the pool mid-transaction would hold whatever locks that transaction
/// holds until something eventually closes it.
pub(super) async fn end_transaction<T>(
    client: &mut Connection,
    result: Result<T, AppError>,
) -> Result<T, AppError> {
    match result {
        Ok(value) => {
            client
                .simple_query("COMMIT TRANSACTION")
                .await
                .map_err(map_error)?;
            Ok(value)
        }
        Err(e) => {
            // Best-effort: if the rollback itself fails, the connection is unhealthy either way,
            // and `Manager::recycle`'s `SELECT 1` catches that the next time it is checked out.
            let _ = client.simple_query("ROLLBACK TRANSACTION").await;
            Err(e)
        }
    }
}

/// Binds one edited value at its next placeholder, given the declared type of the column it is
/// going into — decoding it first when [`is_binary_type`] says the column is one (D7), and refusing
/// it outright when [`is_money_type`] says it is one and the text is ambiguous (found live: see
/// [`reject_money_thousands_separator`]).
///
/// Every other value is bound as the text it arrived as and left for SQL Server to convert on its
/// own — it does that implicitly for every type here except a binary one, which is the one
/// conversion `nvarchar` (what `tiberius` always sends a Rust string as) is not allowed to make,
/// whether the value is being compared or assigned.
fn bind_write<'a>(
    query: &mut tiberius::Query<'a>,
    value: &'a Value,
    data_type: &str,
) -> Result<(), AppError> {
    let binary = is_binary_type(data_type);
    if binary {
        match value {
            Value::String(s) => {
                query.bind(Some(std::borrow::Cow::<[u8]>::Owned(decode_binary(s)?)));
            }
            _ => query.bind(Option::<std::borrow::Cow<[u8]>>::None),
        }
        return Ok(());
    }

    if is_money_type(data_type) {
        if let Value::String(s) = value {
            reject_money_thousands_separator(s)?;
        }
    }

    match value {
        Value::String(s) => query.bind(Some(s.as_str())),
        _ => query.bind(Option::<&str>::None),
    }
    Ok(())
}

/// Whether `data_type`'s base name is one of the fixed-point currency types.
fn is_money_type(data_type: &str) -> bool {
    let base = data_type
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    MONEY_TYPES.contains(&base.as_str())
}

/// Refuses a money value that reads a comma, rather than let SQL Server guess what it meant.
///
/// Found live, not in review: `CAST('9,9999' AS money)` does not fail and does not read 9.9999 with
/// a mistyped decimal separator — SQL Server's own string-to-money conversion treats a comma as an
/// optional thousands separator and drops it, so the text becomes the integer `99999`, written out
/// with money's fixed four decimal places as `99999.0000`. No other type here does this: `decimal`,
/// `int` and the rest reject a comma outright with a clear conversion error. Money is silent about
/// it instead, off by two orders of magnitude, which is worse than an error a user would notice —
/// so this catches it before the value ever reaches the server.
fn reject_money_thousands_separator(value: &str) -> Result<(), AppError> {
    if value.contains(',') {
        return Err(err!("error.mssqlAmbiguousMoney"));
    }
    Ok(())
}

/// Updates exactly one row, identified by `key` (the primary key's columns, or — when a table has
/// none — every column, the same fallback MySQL uses). Runs inside a transaction that first counts
/// what the key matches, so the no-primary-key fallback cannot silently clobber a row's duplicate.
///
/// The count runs at the connection's default isolation, not `READ UNCOMMITTED` the way browsing a
/// page of data does (D13): reading how many rows a key matches right before writing one of them is
/// exactly the "dirty read then write" D13 keeps off the write path, unlike reading a row only to
/// display it.
pub async fn update_row(
    pool: &Pool,
    database: &str,
    table: &str,
    updates: &Map<String, Value>,
    key: &Map<String, Value>,
) -> Result<(), AppError> {
    if updates.is_empty() {
        return Ok(());
    }
    if key.is_empty() {
        return Err(err!("error.updateWithoutKey"));
    }

    let (schema, name) = resolve(table);
    let qualified = three_part(database, &schema, &name);
    let column_rows = table_columns(pool, database, &schema, &name).await?;
    let types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    client
        .simple_query("BEGIN TRANSACTION")
        .await
        .map_err(map_error)?;

    let result = update_row_body(&mut client, &qualified, &types, updates, key).await;
    end_transaction(&mut client, result).await
}

/// The body of [`update_row`] once inside its transaction — split out so [`end_transaction`] can
/// commit or roll back around whatever it returns, rather than the function returning early through
/// several `?`s that would each have to remember to roll back by hand.
async fn update_row_body(
    client: &mut Connection,
    qualified: &str,
    types: &BTreeMap<String, String>,
    updates: &Map<String, Value>,
    key: &Map<String, Value>,
) -> Result<(), AppError> {
    let data_type = |column: &str| types.get(column).map(String::as_str).unwrap_or("");

    let count_sql = format!(
        "SELECT COUNT_BIG(*) FROM {qualified} WHERE {}",
        key_predicate(key, 1)
    );
    let mut count_query = tiberius::Query::new(count_sql);
    for (column, value) in key {
        bind_write(&mut count_query, value, data_type(column))?;
    }
    let matched: i64 = count_query
        .query(client)
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?
        .and_then(|row| row.get::<i64, _>(0))
        .unwrap_or(0);
    if matched != 1 {
        return Err(err!("error.rowsMatched", matched = matched));
    }

    let set_clause = updates
        .keys()
        .enumerate()
        .map(|(i, c)| format!("{} = @P{}", quote_ident(c), i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    // The key's placeholders carry on where the SET clause's left off: one numbering runs through
    // the whole statement, unlike MySQL's positional `?`.
    let update_sql = format!(
        "UPDATE {qualified} SET {set_clause} WHERE {}",
        key_predicate(key, updates.len() + 1)
    );
    let mut update_query = tiberius::Query::new(update_sql);
    for (column, value) in updates {
        bind_write(&mut update_query, value, data_type(column))?;
    }
    for (column, value) in key {
        bind_write(&mut update_query, value, data_type(column))?;
    }
    update_query.execute(client).await.map_err(map_error)?;

    Ok(())
}

/// Inserts `rows`, one map per new row, all in a single transaction: if any one of them is
/// rejected, none of them land.
///
/// A row only carries the columns it has something to say about — a column left out of the map is
/// left out of that row's INSERT too, so the table's own DEFAULT (or IDENTITY, or a computed
/// expression) is what fills it. That is also why each row is its own statement rather than one
/// multi-VALUES INSERT: rows may fill in different sets of columns, and the error a rejected row
/// produces can then say which row it was.
pub async fn insert_rows(
    pool: &Pool,
    database: &str,
    table: &str,
    rows: &[Map<String, Value>],
) -> Result<(), AppError> {
    if rows.is_empty() {
        return Ok(());
    }

    let (schema, name) = resolve(table);
    let qualified = three_part(database, &schema, &name);
    let column_rows = table_columns(pool, database, &schema, &name).await?;
    let types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    client
        .simple_query("BEGIN TRANSACTION")
        .await
        .map_err(map_error)?;

    let result = insert_rows_body(&mut client, &qualified, &types, rows).await;
    end_transaction(&mut client, result).await
}

async fn insert_rows_body(
    client: &mut Connection,
    qualified: &str,
    types: &BTreeMap<String, String>,
    rows: &[Map<String, Value>],
) -> Result<(), AppError> {
    for (i, row) in rows.iter().enumerate() {
        // `DEFAULT VALUES` is the same keyword PostgreSQL uses for "a row that is nothing but
        // defaults" — T-SQL has it too, unlike MySQL's `() VALUES ()`.
        let sql = if row.is_empty() {
            format!("INSERT INTO {qualified} DEFAULT VALUES")
        } else {
            let columns = row
                .keys()
                .map(|c| quote_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            let placeholders = (1..=row.len())
                .map(|n| format!("@P{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("INSERT INTO {qualified} ({columns}) VALUES ({placeholders})")
        };

        let mut query = tiberius::Query::new(sql);
        let mut bind_err = None;
        for (column, value) in row {
            let data_type = types.get(column).map(String::as_str).unwrap_or("");
            if let Err(e) = bind_write(&mut query, value, data_type) {
                bind_err = Some(e);
                break;
            }
        }
        if let Some(e) = bind_err {
            return Err(err!("error.rowFailed", index = i + 1).caused_by(e));
        }

        query
            .execute(client)
            .await
            .map_err(|e| err!("error.rowFailed", index = i + 1).caused_by(map_error(e)))?;
    }

    Ok(())
}

/// Deletes the rows `keys` names — each map is one row's primary key columns, or every column when
/// the table has none — or every row when `all` is set. The deletes run in one transaction: if any
/// of them fails, none of them land.
///
/// SQL Server has no `(tableoid, ctid)` the way PostgreSQL does for a table with no primary key, and
/// no need of one: a heap or a clustered index both support `DELETE TOP (n)`, T-SQL's spelling of
/// the cap MySQL's `LIMIT 1` puts on the same fallback — one row's predicate deletes at most one
/// row, never its duplicate, when every column is the key rather than a real one.
///
/// `reset_auto_increment` keeps the name the frontend already uses for the checkbox; what it resets
/// here is the table's IDENTITY seed.
pub async fn delete_rows(
    pool: &Pool,
    database: &str,
    table: &str,
    keys: &[Map<String, Value>],
    all: bool,
    reset_auto_increment: bool,
) -> Result<(), AppError> {
    if !all && keys.is_empty() {
        return Ok(());
    }

    let (schema, name) = resolve(table);
    let qualified = three_part(database, &schema, &name);
    let column_rows = table_columns(pool, database, &schema, &name).await?;
    let types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    client
        .simple_query("BEGIN TRANSACTION")
        .await
        .map_err(map_error)?;

    let result = delete_rows_body(&mut client, &qualified, &types, keys, all).await;
    end_transaction(&mut client, result).await?;

    if reset_auto_increment {
        reset_identity(pool, database, &schema, &name).await?;
    }
    Ok(())
}

async fn delete_rows_body(
    client: &mut Connection,
    qualified: &str,
    types: &BTreeMap<String, String>,
    keys: &[Map<String, Value>],
    all: bool,
) -> Result<(), AppError> {
    if all {
        client
            .simple_query(format!("DELETE FROM {qualified}"))
            .await
            .map_err(map_error)?;
        return Ok(());
    }

    for key in keys {
        if key.is_empty() {
            return Err(err!("error.deleteWithoutKey"));
        }
        let sql = format!(
            "DELETE TOP (1) FROM {qualified} WHERE {}",
            key_predicate(key, 1)
        );
        let mut query = tiberius::Query::new(sql);
        for (column, value) in key {
            let data_type = types.get(column).map(String::as_str).unwrap_or("");
            bind_write(&mut query, value, data_type)?;
        }
        query.execute(client).await.map_err(map_error)?;
    }

    Ok(())
}

/// Puts the table's IDENTITY seed back to 0, so the next insert numbers from 1 again — SQL Server's
/// counterpart to PostgreSQL's `ALTER SEQUENCE ... RESTART` and MySQL's `AUTO_INCREMENT = 1`.
///
/// `DBCC CHECKIDENT` errors outright on a table with no IDENTITY column, unlike MySQL's
/// unconditional `AUTO_INCREMENT = 1` — so this checks `sys.identity_columns` first and does
/// nothing on a table that has none, since `delete_rows(all, true)` is called on any table
/// regardless of whether it has a counter (D7).
async fn reset_identity(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<(), AppError> {
    let db = quote_ident(database);
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;

    let sql = format!(
        "SELECT TOP (1) 1 FROM {db}.sys.identity_columns c
         JOIN {db}.sys.objects o ON o.object_id = c.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         WHERE s.name = @P1 AND o.name = @P2"
    );
    let has_identity = client
        .query(sql, &[&schema, &table])
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?
        .is_some();
    if !has_identity {
        return Ok(());
    }

    // A string argument, not SQL: DBCC CHECKIDENT takes the table's name as a quoted literal, which
    // is why it needs its own escaping (doubling `'`) rather than `quote_ident`'s bracket rule, and
    // why it needs its schema spelled out explicitly rather than left to resolve on its own (D7).
    let object = full_object_name(schema, table).replace('\'', "''");
    client
        .simple_query(format!("DBCC CHECKIDENT ('{object}', RESEED, 0)"))
        .await
        .map_err(map_error)?;
    Ok(())
}

/// The bytes a binary cell's text stands for — the base64 [`column_value`]'s `Binary` arm encoded
/// them as, undone.
///
/// Refused rather than passed through some other way when it does not decode: text that is not
/// valid base64 did not come out of this app's own grid, and guessing at what the user meant would
/// silently write something other than what they typed.
pub(super) fn decode_binary(value: &str) -> Result<Vec<u8>, AppError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|e| err!("error.mssqlInvalidBinary", message = e))
}

/// `[schema].[table]`, always both bracketed and always both present — unlike [`qualify`], whose
/// output is a *display* string the sidebar can leave a plain-looking name in. This one is embedded
/// in real SQL text (inside the string literal `DBCC CHECKIDENT` takes), so a name with a space
/// left unbracketed would break the multi-part name parsing DBCC does on that string, and dropping
/// `dbo` would leave the server to resolve the unqualified half against whichever schema happens to
/// be the caller's own default rather than the one this table is actually in (D7).
pub(super) fn full_object_name(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}

/// A predicate matching every column `key` names, its placeholders numbered from `first`.
///
/// `(col = @Pn OR (col IS NULL AND @Pn IS NULL))` rather than `=`, so a key column that is itself
/// NULL still matches — the same problem PostgreSQL's `IS NOT DISTINCT FROM` and MySQL's `<=>`
/// solve, spelled out by hand because SQL Server has no equivalent before its 2022 release and
/// nobody has confirmed either test server, or a user's, is running one that new.
fn key_predicate(key: &Map<String, Value>, first: usize) -> String {
    key.keys()
        .enumerate()
        .map(|(i, column)| {
            let col = quote_ident(column);
            let p = format!("@P{}", first + i);
            format!("({col} = {p} OR ({col} IS NULL AND {p} IS NULL))")
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The base64 `column_value` encoded a binary column's bytes as, undone. A round trip through
    /// both is what a copy-then-paste-back of a binary cell actually does.
    #[test]
    fn base64_decodes_back_to_the_original_bytes() {
        assert_eq!(decode_binary("AQID").unwrap(), vec![1, 2, 3]);
    }

    /// Text that never came out of this app's own grid is refused rather than sent to the server as
    /// something it is not — the byte string it would decode to (if it decoded at all) has nothing
    /// to do with what the user typed.
    #[test]
    fn text_that_is_not_base64_is_refused() {
        assert!(decode_binary("not base64!!").is_err());
    }

    /// Unlike `qualify`, both halves are always bracketed and `dbo` is never dropped: this name is
    /// embedded in the string literal `DBCC CHECKIDENT` takes, not shown to a person, so there is no
    /// plain-looking form to prefer — a space left unbracketed would break the multi-part name
    /// DBCC parses out of that string, and an unqualified table would resolve against whichever
    /// schema happens to be the caller's own default rather than the one it is actually in (D7).
    #[test]
    fn full_object_name_always_carries_its_schema() {
        assert_eq!(full_object_name("dbo", "users"), "[dbo].[users]");
        assert_eq!(
            full_object_name("sales", "Order Details"),
            "[sales].[Order Details]"
        );
        assert_eq!(full_object_name("dbo", "a.b"), "[dbo].[a.b]");
    }

    /// `col = @Pn` alone never matches a key column that is itself NULL — `NULL = NULL` is NULL,
    /// not true — so a row a filter or an edit left NULL in its key could never be found again.
    /// SQL Server has no `IS NOT DISTINCT FROM` before its 2022 release, so this spells the same
    /// comparison out rather than betting on a version nobody has confirmed either test server or a
    /// user's is running.
    #[test]
    fn a_key_column_that_is_null_still_matches_itself() {
        let mut key = Map::new();
        key.insert("id".to_string(), Value::from(9));
        assert_eq!(
            key_predicate(&key, 1),
            "([id] = @P1 OR ([id] IS NULL AND @P1 IS NULL))"
        );
    }

    /// Placeholders start at `first`, not at 1 — the SET clause of an UPDATE claims `@P1..@Pn`
    /// first, and the key predicate's own placeholders have to continue from there.
    #[test]
    fn key_predicate_numbers_from_the_given_offset() {
        let mut key = Map::new();
        key.insert("id".to_string(), Value::from(1));
        key.insert("region".to_string(), Value::from("us"));
        assert_eq!(
            key_predicate(&key, 4),
            "([id] = @P4 OR ([id] IS NULL AND @P4 IS NULL)) AND \
             ([region] = @P5 OR ([region] IS NULL AND @P5 IS NULL))"
        );
    }

    #[test]
    fn money_types_are_recognized_by_base_name() {
        assert!(is_money_type("money"));
        assert!(is_money_type("smallmoney"));
        assert!(!is_money_type("decimal(19,4)"));
        assert!(!is_money_type("int"));
    }

    /// Found live: a user typed `9,9999` meaning `9.9999` (comma as the decimal point, a common
    /// habit outside en-US locales) and SQL Server's money conversion read it as the integer
    /// `99999` instead — the comma is an optional thousands separator to it, silently dropped,
    /// leaving no decimal point at all in what remained. No error, no warning, a value two orders
    /// of magnitude off. This is the guard that catches it before the value ever reaches the server.
    #[test]
    fn a_comma_in_a_money_value_is_refused() {
        let error = reject_money_thousands_separator("9,9999").expect_err("a comma is ambiguous");
        assert_eq!(error.code, "error.mssqlAmbiguousMoney");
    }

    #[test]
    fn a_money_value_with_only_a_decimal_point_is_accepted() {
        assert!(reject_money_thousands_separator("9.9999").is_ok());
        assert!(reject_money_thousands_separator("-42").is_ok());
    }
}
