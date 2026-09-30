//! SQL Server, over TDS rather than through sqlx — see
//! `docs/specs/2026-09-05-mssql-support-design.md` (D1..D15).
//!
//! Closest to `postgres.rs` in shape, since both put a schema between the database and its tables
//! and so name a table `schema.table`; closest to `mysql.rs` in connection model, since both reach
//! every database of the server over one pool rather than dialing again per database.

use super::filters::split_list;
use crate::error::AppError;
use crate::modules::db::models::ServerInfo;
use deadpool::managed::{
    Manager as ManagerTrait, Metrics, Pool as DeadPool, RecycleError, RecycleResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use tiberius::numeric::Decimal;
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, FromSql, Row};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

/// The schema whose tables are named without a prefix, here and in the sidebar. `dbo` is the
/// default schema every new user is given, so it plays the part `public` plays on PostgreSQL: its
/// tables are also what an unqualified name in a query means.
pub const DEFAULT_SCHEMA: &str = "dbo";

/// Brackets an identifier for interpolation into SQL text, doubling an embedded closing bracket —
/// SQL Server's own escaping rule.
///
/// The one engine here whose quote is not a single symmetric character: `[` opens and `]` closes,
/// and only `]` is escaped. An opening bracket inside a name means nothing, because the name is
/// already open by the time it is read.
pub(super) fn quote_ident(ident: &str) -> String {
    format!("[{}]", ident.replace(']', "]]"))
}

/// How a table is named everywhere outside this module: in the sidebar, in the frontend's state,
/// and back down in every command that takes a table.
///
/// A table in `dbo` is named by itself, since that is the schema an unqualified name already
/// resolves to — so a database that only uses `dbo`, which is most of them, reads exactly as a
/// MySQL one does. Anything else carries its schema.
///
/// The bracketing is what makes this reversible by [`resolve`]: a name holding a `.` or a `]`
/// would otherwise render to something that reads as a different pair.
pub fn qualify(schema: &str, table: &str) -> String {
    let table = if needs_quoting(table) {
        quote_ident(table)
    } else {
        table.to_string()
    };
    if schema == DEFAULT_SCHEMA {
        return table;
    }
    let schema = if needs_quoting(schema) {
        quote_ident(schema)
    } else {
        schema.to_string()
    };
    format!("{schema}.{table}")
}

/// Whether a name would be misread if it were written plainly — the characters [`qualify`] builds
/// its result out of.
fn needs_quoting(name: &str) -> bool {
    name.contains('.') || name.contains(']') || name.contains('[')
}

/// The schema and table a [`qualify`]ed name stands for. An unqualified name is a table of
/// [`DEFAULT_SCHEMA`], which is what `qualify` leaves the prefix off for.
///
/// Nothing calls this yet: the commands that take a table name — reading a page, reading a
/// structure — arrive with the reads in Plan 2. It is written and tested here rather than there
/// because it is the half of [`qualify`] that proves `qualify` reversible, and a round-trip test
/// needs both halves in one place.
#[allow(dead_code)]
pub fn resolve(qualified: &str) -> (String, String) {
    let (schema, table) = split_qualified(qualified);
    (unquote_ident(schema), unquote_ident(table))
}

/// Splits at the dot that separates the two halves, leaving both still bracketed as they were. A
/// bracketed first half is scanned to its closing bracket first, so a dot inside it is not the
/// split.
///
/// Unlike PostgreSQL's, this cannot toggle a flag on one character: `[` and `]` are different, and
/// a doubled `]]` inside a name is an escaped bracket rather than a close followed by an open.
fn split_qualified(qualified: &str) -> (&str, &str) {
    let bytes = qualified.as_bytes();
    let mut i = 0;
    let mut bracketed = false;
    while i < bytes.len() {
        match bytes[i] {
            b'[' if !bracketed => bracketed = true,
            b']' if bracketed => {
                // A doubled `]` is an escaped bracket, still inside the name.
                if bytes.get(i + 1) == Some(&b']') {
                    i += 2;
                    continue;
                }
                bracketed = false;
            }
            b'.' if !bracketed => return (&qualified[..i], &qualified[i + 1..]),
            _ => {}
        }
        i += 1;
    }
    (DEFAULT_SCHEMA, qualified)
}

/// Takes the brackets off an identifier that carries them, undoubling what is inside.
fn unquote_ident(ident: &str) -> String {
    let trimmed = ident.trim();
    match trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        Some(inner) => inner.replace("]]", "]"),
        None => trimmed.to_string(),
    }
}

/// One TDS connection, as the rest of this file uses it.
///
/// `Compat` because tiberius reads and writes through the `futures` traits while tokio has its
/// own: the wrapper is the whole of the adaptation, and `tokio-util`'s `compat` feature is carried
/// for it.
pub type Connection = Client<Compat<TcpStream>>;

/// What a whole SQL Server connection is.
///
/// One pool for the server, not one per database — a TDS session moves between databases with
/// `USE` or a three-part name, the way MySQL can and PostgreSQL cannot. So `database` in every
/// command below names a database to reach into, never a pool to pick (D2).
pub type Pool = DeadPool<Manager>;

/// What the pool opens a connection with. Holds the whole `Config` rather than the parts, since a
/// tiberius connection is built from one.
pub struct Manager {
    config: Config,
}

impl ManagerTrait for Manager {
    type Type = Connection;
    type Error = AppError;

    async fn create(&self) -> Result<Connection, AppError> {
        dial(&self.config).await
    }

    /// A pooled connection is handed back only if it still answers. `SELECT 1` is the cheapest
    /// thing that proves a TDS session is alive; one that fails is dropped and a fresh connection
    /// dialed, which is what makes a tunnel that went down and came back usable again without
    /// reconnecting the whole thing by hand.
    async fn recycle(&self, client: &mut Connection, _: &Metrics) -> RecycleResult<AppError> {
        client
            .simple_query("SELECT 1")
            .await
            .map_err(|e| RecycleError::Backend(map_error(e)))?;
        Ok(())
    }
}

/// Opens one connection and settles the session-wide setting every read below depends on.
async fn dial(config: &Config) -> Result<Connection, AppError> {
    let tcp = TcpStream::connect(config.get_addr())
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    // Nagle off: TDS is request/response, and holding a small request back behind a 40ms timer is
    // the wrong trade for every query this app sends.
    tcp.set_nodelay(true).ok();
    let mut client = Client::connect(config.clone(), tcp.compat_write())
        .await
        .map_err(map_error)?;

    /* Waiting forever on a lock is the one way SQL Server can hang this app that the other three
    engines cannot: its default READ COMMITTED takes locks rather than reading a snapshot, so a
    table someone else is mid-transaction on blocks a plain SELECT with no error and no timeout
    — a spinner that never stops. Five seconds is long enough for a short transaction to pass
    and short enough that the user reads it as a failure rather than a freeze. See D13. */
    client
        .simple_query("SET LOCK_TIMEOUT 5000")
        .await
        .map_err(map_error)?;
    Ok(client)
}

/// Turns a driver error into one the frontend can show, keeping the server's own words — the
/// counterpart of `postgres::map_error`.
pub(super) fn map_error(e: tiberius::error::Error) -> AppError {
    err!("error.mssql", message = e)
}

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

/// Dials the server and hands back a pool that reaches every database on it.
///
/// `database` is the one a session starts on, and unlike PostgreSQL that is all it is: a command
/// naming another database reaches it over the same pool. Left empty, the server's own default for
/// the login stands, which is what every other SQL Server client does too.
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: Option<&str>,
    use_ssl: Option<bool>,
) -> Result<Pool, AppError> {
    let mut config = Config::new();
    config.host(host);
    config.port(port);
    config.authentication(AuthMethod::sql_server(username, password));
    if let Some(database) = database.map(str::trim).filter(|d| !d.is_empty()) {
        config.database(database);
    }

    /* Read tiberius' own words for these two rather than the names: `Off` is *"only use encryption
    for the login procedure"*, not "no encryption at all" — that one is `NotSupported`. Which is
    exactly the shape SQL Server has and the other two engines do not: the login packet is
    encrypted whatever the server's TLS setting says, so "try TLS, else plaintext" is not the
    clean binary `PgSslMode::Prefer` is. So the box off means `Off` — the login still protected,
    nothing beyond it — and the box on means `Required`. See D6. */
    config.encryption(if use_ssl == Some(false) {
        EncryptionLevel::Off
    } else {
        EncryptionLevel::Required
    });
    /* A self-signed certificate is what a self-installed SQL Server has, and refusing it would
    make the app unable to reach the servers it is most often pointed at. So the TLS box here
    means "encrypt", not "encrypt and verify the chain" — worth saying, because it is a real
    difference from what the same box means on MySQL and PostgreSQL (D6). */
    config.trust_cert();

    let pool = Pool::builder(Manager { config })
        .max_size(8)
        .build()
        .map_err(|e| err!("error.mssql", message = e))?;
    // Dial once here rather than on the first command: a wrong host or password should fail the
    // Connect button, not the first table the user opens. The connection goes straight back to the
    // pool — it is the dialing that was the point, not the object.
    drop(
        pool.get()
            .await
            .map_err(|e| err!("error.mssql", message = e))?,
    );
    Ok(pool)
}

/// The server's version and the machine under it, for the header.
///
/// `SERVERPROPERTY` rather than cutting the version out of `@@VERSION`, which is the
/// obvious-looking way and the wrong one: `@@VERSION` is localised to the language the server was
/// installed in, so looking for English words in it gives an empty header on any server that is
/// not English. `SERVERPROPERTY` answers in numbers whatever the language.
///
/// `@@VERSION` is still read, for the operating system alone — its tail, after " on ", is the one
/// part `SERVERPROPERTY` has no equivalent for. It failing leaves `os` empty rather than failing
/// the command: a header without the machine is still a header.
pub async fn server_info(pool: &Pool) -> Result<ServerInfo, AppError> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;

    let row = client
        .query(
            "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128)),
                    CAST(SERVERPROPERTY('Edition') AS nvarchar(128))",
            &[],
        )
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?
        .ok_or_else(|| err!("error.mssql", message = "the server reported no version"))?;

    let product: &str = row.get(0).unwrap_or_default();
    let edition: &str = row.get(1).unwrap_or_default();
    let version = if edition.is_empty() {
        product.to_string()
    } else {
        format!("{product} ({edition})")
    };

    let os = match client.query("SELECT @@VERSION", &[]).await {
        Ok(stream) => stream
            .into_row()
            .await
            .ok()
            .flatten()
            .and_then(|row| row.get::<&str, _>(0).map(str::to_string))
            .and_then(|banner| {
                banner
                    .split(" on ")
                    .nth(1)
                    .map(|tail| tail.trim().to_string())
            })
            .unwrap_or_default(),
        Err(_) => String::new(),
    };

    Ok(ServerInfo { version, os })
}

/// Every database on the server this login can actually open, the server's own left out.
///
/// Three conditions, and the third is the one that is easy to leave off and impossible to notice
/// while testing as `sa`:
///
/// * `database_id > 4` drops `master`, `tempdb`, `model` and `msdb`, which the server rebuilds
///   from its own files and which nobody browsing their data wants to see.
/// * `state = 0` drops one that is offline, restoring, or otherwise not readable.
/// * `HAS_DBACCESS(name) = 1` drops the ones this login has no rights to. Without it an ordinary
///   login sees a database in the sidebar and is told "The server principal is not able to access
///   the database" the moment it clicks — `sa` sees everything, so the bug hides during testing.
pub async fn list_databases(pool: &Pool) -> Result<Vec<String>, AppError> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(
            "SELECT name FROM sys.databases
             WHERE database_id > 4 AND state = 0 AND HAS_DBACCESS(name) = 1
             ORDER BY name",
            &[],
        )
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| row.get::<&str, _>(0).map(str::to_string))
        .collect())
}

/// Every table and view of `database`, across every schema, named as [`qualify`] names them.
///
/// Views as well as tables, because `postgres::list_tables` lists both and the sidebar is one list
/// whichever server filled it. That is why this reads `sys.objects` filtered to `U` and `V` rather
/// than `sys.tables`, which holds no views at all.
///
/// `dbo` first and the other schemas after it, alphabetically — so the tables of the schema that
/// needs no prefix are together at the top, the way they are on PostgreSQL. No system-schema
/// filter is needed: `sys.objects` in a user database holds that user's own objects.
///
/// The database is written into the query as a three-part name rather than reached with a `USE`,
/// since the pooled session may be sitting on any database at all (D2) and a `USE` would leave it
/// somewhere else for the next borrower.
pub async fn list_tables(pool: &Pool, database: &str) -> Result<Vec<String>, AppError> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let db = quote_ident(database);
    let sql = format!(
        "SELECT s.name, o.name
         FROM {db}.sys.objects o
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         WHERE o.type IN ('U', 'V')
         ORDER BY CASE WHEN s.name = '{DEFAULT_SCHEMA}' THEN 0 ELSE 1 END, s.name, o.name"
    );
    let rows = client
        .query(sql, &[])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| {
            let schema: &str = row.get(0)?;
            let table: &str = row.get(1)?;
            Some(qualify(schema, table))
        })
        .collect())
}

/// The types whose length the catalogue reports in bytes but which are counted in characters.
const UNICODE_TYPES: [&str; 3] = ["nchar", "nvarchar", "ntext"];
/// The types declared with a precision and a scale.
const DECIMAL_TYPES: [&str; 2] = ["decimal", "numeric"];
/// The types declared with a fractional-seconds scale alone.
const SCALED_TIME_TYPES: [&str; 3] = ["datetime2", "time", "datetimeoffset"];
/// The types declared with a length.
const SIZED_TYPES: [&str; 6] = [
    "char",
    "varchar",
    "nchar",
    "nvarchar",
    "binary",
    "varbinary",
];

/// A column's type the way it would be written in a `CREATE TABLE`: `nvarchar(255)`,
/// `decimal(10,2)`, `int`.
///
/// Two conventions of `sys.columns` make the obvious reading wrong, and both are silent:
///
/// * `max_length` is in **bytes**. An `nvarchar(255)` reads 510 there, two bytes to the character,
///   so the Unicode types are halved. `INFORMATION_SCHEMA.COLUMNS` counts characters instead — one
///   source or the other, never a mix, and this file is on `sys.columns` throughout.
/// * `-1` means **`MAX`**, not a negative width. It appears for `varchar(max)`, `nvarchar(max)` and
///   `varbinary(max)`, and reads as a length only if nobody looks.
///
/// Everything else takes no argument, precision and scale being reported for the fixed-width
/// numbers as well: `int` has a precision of 10 in the catalogue, and `int(10)` is not a type.
pub(super) fn display_type(name: &str, max_length: i16, precision: u8, scale: u8) -> String {
    let lower = name.to_ascii_lowercase();
    if DECIMAL_TYPES.contains(&lower.as_str()) {
        return format!("{name}({precision},{scale})");
    }
    if SCALED_TIME_TYPES.contains(&lower.as_str()) {
        return format!("{name}({scale})");
    }
    if SIZED_TYPES.contains(&lower.as_str()) {
        if max_length == -1 {
            return format!("{name}(max)");
        }
        let length = if UNICODE_TYPES.contains(&lower.as_str()) {
            max_length / 2
        } else {
            max_length
        };
        return format!("{name}({length})");
    }
    name.to_string()
}

/// A default constraint's definition without the parentheses SQL Server wraps it in.
///
/// The catalogue stores `((0))` for the literal 0 and `(getdate())` for the call, so showing the
/// definition as it is stored puts a pair of brackets on every default in the grid. Only pairs that
/// wrap the **whole** expression come off — `(a)+(b)` opens and closes twice and keeps both.
///
/// Writing DDL puts them back: SQL Server accepts a `DEFAULT` either way, and re-wrapping is what
/// makes what MixLab writes read the way what SSMS writes does. That is Plan 6's problem, not this
/// one's.
pub(super) fn strip_default_parens(definition: &str) -> String {
    let mut current = definition.trim();
    loop {
        let Some(inner) = current.strip_prefix('(').and_then(|s| s.strip_suffix(')')) else {
            return current.to_string();
        };
        // The leading `(` has to be the one the trailing `)` closes. It is not, in `(a)+(b)`:
        // scanning what is between them, the depth goes negative at the first `)`, which is where
        // the leading bracket was already closed.
        let mut depth = 0i32;
        for ch in inner.chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return current.to_string();
            }
        }
        current = inner.trim();
    }
}

/// One column of one table, as this file reads it out of the catalogue.
pub(super) struct ColumnRow {
    pub name: String,
    /// Already through [`display_type`]: `nvarchar(255)`, not `nvarchar` and a byte count.
    pub data_type: String,
    pub nullable: bool,
    /// The default constraint's definition, already through [`strip_default_parens`].
    pub default_value: Option<String>,
    pub is_identity: bool,
    pub is_computed: bool,
    /// A `rowversion` (spelled `timestamp` by older schemas): a value the server stamps on every
    /// write and that nothing may ever name in an INSERT (D7).
    pub is_rowversion: bool,
}

/// The tokens `ColumnMeta::extra` carries for one column — what the server fills in itself.
///
/// Read back by `src/modules/db/mssql/columns.ts`, and those two files are the only ones that need
/// to agree on the spelling. Three `bool`s rather than a [`ColumnRow`] because the two callers read
/// them out of two different catalogue queries.
pub(super) fn extra_tokens(is_identity: bool, is_computed: bool, is_rowversion: bool) -> String {
    let mut tokens: Vec<&str> = Vec::new();
    if is_identity {
        tokens.push("identity");
    }
    if is_computed {
        tokens.push("generated");
    }
    if is_rowversion {
        tokens.push("rowversion");
    }
    tokens.join(" ")
}

/// Whether a type is a `rowversion` under either of its two names.
///
/// `timestamp` is what older schemas call it, and it has nothing to do with a date — it is eight
/// bytes the server stamps on every write.
pub(super) fn is_rowversion_type(type_name: &str) -> bool {
    let lower = type_name.to_ascii_lowercase();
    lower == "rowversion" || lower == "timestamp"
}

/// A table named the way a pooled session can reach it whatever database it is sitting on.
pub(super) fn three_part(database: &str, schema: &str, table: &str) -> String {
    format!(
        "{}.{}.{}",
        quote_ident(database),
        quote_ident(schema),
        quote_ident(table)
    )
}

/// A read, wrapped so it does not queue behind someone else's uncommitted write.
///
/// SQL Server's default READ COMMITTED takes locks rather than reading a snapshot, so a plain
/// SELECT on a table another transaction is holding **waits** — the reason [`dial`] sets
/// `LOCK_TIMEOUT` at all (D13). Browsing data to look at it is not worth blocking on, and reading a
/// row someone is about to commit is not worth failing over, so every read here runs at READ
/// UNCOMMITTED. The write paths keep the default: a dirty row that is displayed is a different
/// thing from a dirty row that is written back.
///
/// The level is put back explicitly rather than left to the RPC. tiberius sends a parameterised
/// query through `sp_executesql`, and SQL Server restores the isolation level a stored procedure
/// changed when it returns — but this connection goes back into a pool that write paths will
/// borrow, and that is not a guarantee worth depending on being right about.
pub(super) fn read_uncommitted(sql: &str) -> String {
    format!(
        "SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED;\n{sql};\n\
         SET TRANSACTION ISOLATION LEVEL READ COMMITTED;"
    )
}

/// One condition on the rows a page is cut out of, as the grid's filter bar sends it. The same
/// shape and the same operator ids as MySQL's and PostgreSQL's — only what they become differs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// The wildcards out of a value going into a `LIKE`, T-SQL's set of them.
///
/// Not `filters::escape_like`, which is right for the two engines it serves and wrong here twice
/// over: SQL Server takes **no** escape character in a `LIKE` unless the pattern names one (so
/// every clause below writes `ESCAPE '\'`), and `[` opens a character set — `[0-9]` is a range —
/// which neither MySQL nor PostgreSQL does. A `contains` filter for `a[0]` must find the text
/// `a[0]`; without the bracket escaped it silently finds `a0` instead. See D12.
pub(super) fn escape_like_mssql(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('[', "\\[")
}

/// Turns the filter rows into a WHERE clause and the values to bind into it, in order.
///
/// Every value reaches the server as a bound parameter — tiberius numbers them `@P1`, `@P2`, …, the
/// way PostgreSQL numbers `$1`, so they cannot simply be counted off the bind list at the end. The
/// column name is the one part that has to be interpolated, which is why it is checked against the
/// table's own columns first.
///
/// Two differences from the PostgreSQL version this is shaped after:
///
/// * **No regex.** SQL Server has no counterpart to `~` or `REGEXP` before its 2025 release, so
///   there is no arm for `regexp`/`notRegexp` and `mssqlDialect.regexpFilter` is false — the
///   operator is gone from the dropdown rather than offered and failing (D12).
/// * **Case is the column's business.** There is no `ILIKE` here, and no `LOWER()` wrapped around
///   anything: whether a comparison is case-sensitive is decided by the column's collation, and
///   most installations default to a `_CI_` one — so in practice these filters ignore case. Forcing
///   it either way with `LOWER()` would only make every one of them unable to use an index.
///
/// Text comparisons cast the column to `nvarchar(max)` so a value bound as text compares as typed.
/// The ordering operators are the exception, and cast nothing: `CAST([id] AS nvarchar(max)) > '9'`
/// would sort 10 before 9. There the column stands and SQL Server converts the bound text to the
/// column's own type, which is what it does implicitly in a comparison — and what it refuses,
/// loudly, when the text is not a value of that type after all. That refusal is the right answer:
/// the user typed something this column could not hold.
fn build_where(
    filters: &[Filter],
    columns: &BTreeMap<String, String>,
) -> Result<(String, Vec<String>), AppError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    let mut next = 1usize;
    let mut placeholder = |binds: &mut Vec<String>, value: String| {
        binds.push(value);
        let p = format!("@P{next}");
        next += 1;
        p
    };

    for filter in filters {
        if !columns.contains_key(&filter.column) {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        }
        let raw = quote_ident(&filter.column);
        let col = format!("CAST({raw} AS nvarchar(max))");
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();
        // Every pattern this builds escapes with a backslash, and SQL Server only knows that if the
        // pattern says so.
        let like = |negated: bool, p: String| {
            let not = if negated { "NOT " } else { "" };
            format!("{col} {not}LIKE {p} ESCAPE '\\'")
        };

        let clause = match operator {
            "eq" => format!("{col} = {}", placeholder(&mut binds, value.to_string())),
            "ne" => format!("{col} <> {}", placeholder(&mut binds, value.to_string())),
            "gt" => format!("{raw} > {}", placeholder(&mut binds, value.to_string())),
            "gte" => format!("{raw} >= {}", placeholder(&mut binds, value.to_string())),
            "lt" => format!("{raw} < {}", placeholder(&mut binds, value.to_string())),
            "lte" => format!("{raw} <= {}", placeholder(&mut binds, value.to_string())),
            "contains" => like(
                false,
                placeholder(&mut binds, format!("%{}%", escape_like_mssql(value))),
            ),
            "notContains" => like(
                true,
                placeholder(&mut binds, format!("%{}%", escape_like_mssql(value))),
            ),
            "startsWith" => like(
                false,
                placeholder(&mut binds, format!("{}%", escape_like_mssql(value))),
            ),
            "endsWith" => like(
                false,
                placeholder(&mut binds, format!("%{}", escape_like_mssql(value))),
            ),
            // `like`/`notLike` hand the pattern over as the user wrote it — that is the point of
            // the operator — so the value is not escaped. The `ESCAPE` clause still stands, which
            // is how a pattern of their own can escape a wildcard too.
            "like" => like(false, placeholder(&mut binds, value.to_string())),
            "notLike" => like(true, placeholder(&mut binds, value.to_string())),
            "in" | "notIn" => {
                let items = split_list(value);
                if items.is_empty() {
                    continue;
                }
                let placeholders = items
                    .into_iter()
                    .map(|item| placeholder(&mut binds, item))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql_op = if operator == "in" { "IN" } else { "NOT IN" };
                format!("{col} {sql_op} ({placeholders})")
            }
            "between" | "notBetween" => {
                let items = split_list(value);
                // Two bounds or nothing — one of them alone says nothing about a range.
                if items.len() < 2 {
                    continue;
                }
                let low = placeholder(&mut binds, items[0].clone());
                let high = placeholder(&mut binds, items[1].clone());
                let sql_op = if operator == "between" {
                    "BETWEEN"
                } else {
                    "NOT BETWEEN"
                };
                format!("{raw} {sql_op} {low} AND {high}")
            }
            "isNull" => format!("{raw} IS NULL"),
            "isNotNull" => format!("{raw} IS NOT NULL"),
            "isEmpty" => format!("{col} = ''"),
            "isNotEmpty" => format!("{col} <> ''"),
            other => return Err(err!("error.unknownFilterOperator", operator = other)),
        };

        clauses.push(clause);
    }

    if clauses.is_empty() {
        return Ok((String::new(), binds));
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), binds))
}

/// The types read as text rather than decoded, and why each one is on the list.
///
/// `xml` and `sql_variant` have no shape the grid could draw. The three CLR types are read with
/// `.ToString()` instead, having no `CAST` to text at all. `money` is text too but through
/// [`MONEY_TYPES`], which needs a conversion style the others do not.
const TEXT_CAST_TYPES: [&str; 2] = ["xml", "sql_variant"];
const CLR_TYPES: [&str; 3] = ["geography", "geometry", "hierarchyid"];
/// The fixed-point currency types, which lose digits twice over if read the obvious way.
///
/// Through tiberius they arrive as an f64 — four decimal places of a fixed-point type put through a
/// binary float, which is the same reason `Numeric` is rendered as text in [`column_value`]. And a
/// plain `CAST(m AS nvarchar(max))` is no better: its default style rounds to **two** decimal
/// places, so a column holding 9.9999 reads 10.00. `CONVERT`'s style 2 is the one that writes all
/// four with no thousands separators, and it is the only spelling of this that keeps what is
/// stored.
const MONEY_TYPES: [&str; 2] = ["money", "smallmoney"];

/// How one column is named in the SELECT list.
///
/// Named one by one rather than `SELECT *`, so a column of a type with no useful decoding can be
/// asked for as text — the same trade `postgres::table_data` makes with its `is_decodable`. The
/// alias keeps the name the cast would otherwise replace.
pub(super) fn select_expr(column: &str, data_type: &str) -> String {
    let base = data_type
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if CLR_TYPES.contains(&base.as_str()) {
        return format!("{column}.ToString() AS {column}");
    }
    if MONEY_TYPES.contains(&base.as_str()) {
        return format!("CONVERT(nvarchar(max), {column}, 2) AS {column}");
    }
    if TEXT_CAST_TYPES.contains(&base.as_str()) {
        return format!("CAST({column} AS nvarchar(max)) AS {column}");
    }
    column.to_string()
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

/// The ORDER BY a page is cut out of.
///
/// Always present, because `OFFSET ... ROWS FETCH NEXT ... ROWS ONLY` is grammatically part of
/// `ORDER BY` and SQL Server refuses the one without the other. `(SELECT NULL)` is what "no order
/// was asked for" is spelled as: it satisfies the grammar and asks the server to sort nothing,
/// which is the semantics `postgres::table_data` gets by leaving the clause out entirely. Ordering
/// by the first column instead would sort the whole table on every page turn.
pub(super) fn order_by_clause(
    sort_column: Option<&str>,
    sort_desc: bool,
    columns: &[String],
) -> String {
    sort_column
        .filter(|c| columns.iter().any(|existing| existing == c))
        .map(|c| {
            format!(
                " ORDER BY {} {}",
                quote_ident(c),
                if sort_desc { "DESC" } else { "ASC" }
            )
        })
        .unwrap_or_else(|| " ORDER BY (SELECT NULL)".to_string())
}

/// Which page of a table is wanted, and what it is cut out of.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    pub page: i64,
    pub page_size: i64,
    /// Ignored unless it names a real column of this table.
    #[serde(default)]
    pub sort_column: Option<String>,
    #[serde(default)]
    pub sort_desc: bool,
    /// ANDed together; `total` counts what is left after them.
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    pub columns: Vec<String>,
    pub column_meta: BTreeMap<String, ColumnMeta>,
    pub primary_key: Vec<String>,
    /// The IDENTITY column, if the table has one — the only kind with a counter worth offering to
    /// reset after a delete.
    pub auto_increment_column: Option<String>,
    pub rows: Vec<Map<String, Value>>,
    pub total: i64,
}

/// The row a foreign key column points at.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKey {
    /// [`qualify`]ed, so a key into another schema names a table the sidebar can open.
    pub table: String,
    pub column: String,
}

/// What is known about one column beyond its name — everything a new row would have to respect.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    /// The declared type as a `CREATE TABLE` would write it: `nvarchar(255)`, `decimal(10,2)`.
    pub data_type: String,
    pub nullable: bool,
    /// The column's DEFAULT, without the parentheses the catalogue wraps it in.
    pub default_value: Option<String>,
    /// `identity`, `generated`, `rowversion` — see [`extra_tokens`].
    pub extra: String,
    pub foreign_key: Option<ForeignKey>,
}

/// The columns of one table, in table order, with what each one is declared as.
///
/// `sys.columns` rather than `INFORMATION_SCHEMA.COLUMNS`: the identity and computed flags are
/// there as proper bits, and the length conventions [`display_type`] knows about are that view's.
/// `sys.default_constraints` carries the default, whose name the server usually generated
/// (`DF__t__col__1A2B3C4D`) and which is therefore looked up by column rather than by name.
///
/// Read from the catalogue rather than from a result set, so a table with no rows still describes
/// itself.
pub(super) async fn table_columns(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnRow>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT c.name, t.name AS type_name, c.max_length, c.precision, c.scale,
                c.is_nullable, c.is_identity, c.is_computed, d.definition
         FROM {db}.sys.columns c
         JOIN {db}.sys.objects o ON o.object_id = c.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.types t ON t.user_type_id = c.user_type_id
         LEFT JOIN {db}.sys.default_constraints d ON d.object_id = c.default_object_id
         WHERE s.name = @P1 AND o.name = @P2
         ORDER BY c.column_id"
    ));

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[&schema, &table])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| {
            let name: &str = row.get("name")?;
            let type_name: &str = row.get("type_name")?;
            Some(ColumnRow {
                name: name.to_string(),
                data_type: display_type(
                    type_name,
                    row.get("max_length").unwrap_or(0),
                    row.get("precision").unwrap_or(0),
                    row.get("scale").unwrap_or(0),
                ),
                nullable: row.get("is_nullable").unwrap_or(true),
                default_value: row.get::<&str, _>("definition").map(strip_default_parens),
                is_identity: row.get("is_identity").unwrap_or(false),
                is_computed: row.get("is_computed").unwrap_or(false),
                is_rowversion: is_rowversion_type(type_name),
            })
        })
        .collect())
}

/// The primary key's columns, in the order the key declares them — not the table's order, and the
/// order a composite key has to be written in.
pub(super) async fn primary_key(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT c.name
         FROM {db}.sys.indexes i
         JOIN {db}.sys.objects o ON o.object_id = i.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.index_columns ic
             ON ic.object_id = i.object_id AND ic.index_id = i.index_id
         JOIN {db}.sys.columns c
             ON c.object_id = ic.object_id AND c.column_id = ic.column_id
         WHERE i.is_primary_key = 1 AND s.name = @P1 AND o.name = @P2
         ORDER BY ic.key_ordinal"
    ));

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[&schema, &table])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| row.get::<&str, _>(0).map(str::to_string))
        .collect())
}

/// Which columns of the table are foreign keys, and what each one points at.
///
/// A failure is swallowed by the caller: the markers are decoration on the grid, and losing them
/// must not cost the rows.
async fn foreign_keys(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<BTreeMap<String, ForeignKey>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT c.name AS column_name,
                rs.name AS ref_schema, ro.name AS ref_table, rc.name AS ref_column
         FROM {db}.sys.foreign_key_columns fk
         JOIN {db}.sys.objects o ON o.object_id = fk.parent_object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.columns c
             ON c.object_id = fk.parent_object_id AND c.column_id = fk.parent_column_id
         JOIN {db}.sys.objects ro ON ro.object_id = fk.referenced_object_id
         JOIN {db}.sys.schemas rs ON rs.schema_id = ro.schema_id
         JOIN {db}.sys.columns rc
             ON rc.object_id = fk.referenced_object_id
             AND rc.column_id = fk.referenced_column_id
         WHERE s.name = @P1 AND o.name = @P2
         ORDER BY fk.constraint_object_id, fk.constraint_column_id"
    ));

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[&schema, &table])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    let mut keys = BTreeMap::new();
    for row in &rows {
        let (Some(column), Some(ref_schema), Some(ref_table), Some(ref_column)) = (
            row.get::<&str, _>("column_name"),
            row.get::<&str, _>("ref_schema"),
            row.get::<&str, _>("ref_table"),
            row.get::<&str, _>("ref_column"),
        ) else {
            continue;
        };
        // The first key a column takes part in is the one shown; a column in two keys is rare and
        // the grid has one marker to give it.
        keys.entry(column.to_string())
            .or_insert_with(|| ForeignKey {
                table: qualify(ref_schema, ref_table),
                column: ref_column.to_string(),
            });
    }
    Ok(keys)
}

/// Reads one page of a table, and how many rows the filters leave to page through.
///
/// `table` arrives as the sidebar names it — bare for a table of `dbo`, `schema.table` otherwise —
/// and [`resolve`] turns it back into the two parts. `database` is named in the SQL rather than
/// switched to with `USE`: the pooled session may be sitting on any database at all, and a `USE`
/// would leave it somewhere else for whoever borrows the connection next (D2).
pub async fn table_data(
    pool: &Pool,
    database: &str,
    table: &str,
    query: &PageQuery,
) -> Result<TablePage, AppError> {
    let (schema, name) = resolve(table);
    let qualified = three_part(database, &schema, &name);

    let column_rows = table_columns(pool, database, &schema, &name).await?;
    let columns: Vec<String> = column_rows.iter().map(|c| c.name.clone()).collect();
    let mut keys = foreign_keys(pool, database, &schema, &name)
        .await
        .unwrap_or_default();

    let column_meta: BTreeMap<String, ColumnMeta> = column_rows
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                ColumnMeta {
                    data_type: c.data_type.clone(),
                    nullable: c.nullable,
                    default_value: c.default_value.clone(),
                    extra: extra_tokens(c.is_identity, c.is_computed, c.is_rowversion),
                    foreign_key: keys.remove(&c.name),
                },
            )
        })
        .collect();

    let primary_key = primary_key(pool, database, &schema, &name)
        .await
        .unwrap_or_default();
    let auto_increment_column = column_rows
        .iter()
        .find(|c| c.is_identity)
        .map(|c| c.name.clone());

    // `build_where` only needs to know a column exists; unlike PostgreSQL, SQL Server converts a
    // bound string to the column's type in a comparison itself, so nothing here casts the value.
    let column_types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();
    let (where_clause, binds) = build_where(&query.filters, &column_types)?;

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;

    // `COUNT_BIG` rather than `COUNT`, which answers as an `int` and overflows — with an error from
    // the server, not a wrong number — somewhere past two billion rows.
    let count_sql = read_uncommitted(&format!(
        "SELECT COUNT_BIG(*) FROM {qualified}{where_clause}"
    ));
    let mut count_query = tiberius::Query::new(count_sql);
    for value in &binds {
        count_query.bind(value.as_str());
    }
    let total: i64 = count_query
        .query(&mut client)
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?
        .and_then(|row| row.get::<i64, _>(0))
        .unwrap_or(0);

    // The ceiling is the largest page size the grid offers; see `mysql::table_data`.
    let page_size = query.page_size.clamp(1, 5000);
    let offset = query.page.max(0).saturating_mul(page_size);
    let order_by = order_by_clause(query.sort_column.as_deref(), query.sort_desc, &columns);
    let select_list = column_rows
        .iter()
        .map(|c| select_expr(&quote_ident(&c.name), &c.data_type))
        .collect::<Vec<_>>()
        .join(", ");
    let data_sql = read_uncommitted(&format!(
        "SELECT {select_list} FROM {qualified}{where_clause}{order_by} \
         OFFSET {offset} ROWS FETCH NEXT {page_size} ROWS ONLY"
    ));
    let mut data_query = tiberius::Query::new(data_sql);
    for value in &binds {
        data_query.bind(value.as_str());
    }
    let rows = data_query
        .query(&mut client)
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(TablePage {
        columns,
        column_meta,
        primary_key,
        auto_increment_column,
        rows: rows.iter().map(row_to_json).collect(),
        total,
    })
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

/// One row as the grid reads it: every column by name, whatever the column holds.
pub(super) fn row_to_json(row: &Row) -> Map<String, Value> {
    let mut obj = Map::new();
    for (column, data) in row.cells() {
        obj.insert(column.name().to_string(), column_value(data));
    }
    obj
}

/// One value, as the closest JSON the frontend can show.
///
/// A `match` rather than the chain of `try_get`s `postgres::column_value` is, because tiberius has
/// already decided what the column is: `ColumnData` is typed off the column's own metadata, so
/// there is nothing to guess and no order for a wrong branch to claim a value in.
///
/// Three arms are worth their reasons:
///
/// * `Numeric` becomes **text**. `DECIMAL(19,4)` does not fit an f64, and the precision is the
///   reason someone chose the type; `rust_decimal` re-renders it exactly, which is the same answer
///   PostgreSQL's `numeric` gives here.
/// * A float that JSON cannot write — NaN, ±infinity — becomes null. `serde_json` refuses those,
///   and a silently dropped column would read in the grid as a row that holds nothing.
/// * `Binary` becomes base64, bytes having no representation of their own in JSON. The frontend's
///   `mssqlDialect.isBinary` is what tells the grid a column arrives that way.
///
/// `money`/`smallmoney`, `xml`, `sql_variant`, `geography`, `geometry` and `hierarchyid` do not
/// reach the arms one would expect: [`select_expr`] asks the server for them as text first, which
/// is what keeps `money`'s four decimal places from going through tiberius' f64 decoding.
pub(super) fn column_value(data: &ColumnData<'static>) -> Value {
    match data {
        ColumnData::U8(v) => v.map(Value::from).unwrap_or(Value::Null),
        ColumnData::I16(v) => v.map(Value::from).unwrap_or(Value::Null),
        ColumnData::I32(v) => v.map(Value::from).unwrap_or(Value::Null),
        ColumnData::I64(v) => v.map(Value::from).unwrap_or(Value::Null),
        ColumnData::F32(v) => v
            .and_then(|n| serde_json::Number::from_f64(n as f64).map(Value::Number))
            .unwrap_or(Value::Null),
        ColumnData::F64(v) => v
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ColumnData::Bit(v) => v.map(Value::from).unwrap_or(Value::Null),
        ColumnData::String(v) => v
            .as_ref()
            .map(|s| Value::String(s.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::Guid(v) => v
            .map(|u| Value::String(u.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::Binary(v) => {
            use base64::Engine;
            v.as_ref()
                .map(|bytes| Value::String(base64::engine::general_purpose::STANDARD.encode(bytes)))
                .unwrap_or(Value::Null)
        }
        ColumnData::Numeric(v) => v
            .and_then(|_| {
                Decimal::from_sql(data)
                    .ok()
                    .flatten()
                    .map(|d| Value::String(d.to_string()))
            })
            .unwrap_or(Value::Null),
        ColumnData::Xml(v) => v
            .as_ref()
            .map(|x| Value::String(x.to_string()))
            .unwrap_or(Value::Null),
        // The four date/time shapes all go through tiberius' own chrono conversions rather than
        // through their day and increment counts by hand: those counts have three different epochs
        // between them, and getting one wrong is a value that is merely plausible.
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            chrono::NaiveDateTime::from_sql(data)
                .ok()
                .flatten()
                .map(|d| Value::String(d.to_string()))
                .unwrap_or(Value::Null)
        }
        ColumnData::Date(_) => chrono::NaiveDate::from_sql(data)
            .ok()
            .flatten()
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::Time(_) => chrono::NaiveTime::from_sql(data)
            .ok()
            .flatten()
            .map(|t| Value::String(t.to_string()))
            .unwrap_or(Value::Null),
        // The only one that carries a zone. Kept as RFC 3339 rather than flattened to UTC: the
        // offset is part of what was stored.
        ColumnData::DateTimeOffset(_) => chrono::DateTime::<chrono::FixedOffset>::from_sql(data)
            .ok()
            .flatten()
            .map(|d| Value::String(d.to_rfc3339()))
            .unwrap_or(Value::Null),
    }
}

/// The catalogue spellings of "raw bytes" that `select_expr` does not already redirect elsewhere.
const BINARY_TYPES: [&str; 3] = ["binary", "varbinary", "image"];

/// Whether a column's value crosses the wire as base64 rather than as itself.
///
/// The write-side twin of `src/modules/db/mssql/columns.ts`'s `isBinary`, and the two files are the
/// only ones that need to agree: a column that one calls binary is a column this one has to decode
/// before binding, or SQL Server refuses the statement outright — `nvarchar` (what `tiberius`
/// always sends a Rust string as) has no implicit conversion to `varbinary`, in an assignment or in
/// a comparison, unlike every other type this file writes.
pub(super) fn is_binary_type(data_type: &str) -> bool {
    let base = data_type
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    BINARY_TYPES.contains(&base.as_str()) || is_rowversion_type(&base)
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
    use super::{
        build_where, column_value, decode_binary, display_type, escape_like_mssql, extra_tokens,
        full_object_name, is_binary_type, is_money_type, key_predicate, order_by_clause, qualify,
        quote_ident, reject_money_thousands_separator, resolve, select_expr, strip_default_parens,
        three_part, Filter,
    };
    use serde_json::{Map, Value};
    use std::borrow::Cow;
    use std::collections::BTreeMap;
    use tiberius::numeric::Numeric;
    use tiberius::time::{Date, DateTime, DateTime2, Time};
    use tiberius::ColumnData;

    /// Every integer width lands on a JSON number, and a NULL of any of them lands on JSON null —
    /// the grid tells "no value" from "the empty string" by exactly this.
    #[test]
    fn integers_become_numbers_and_nulls_become_null() {
        assert_eq!(column_value(&ColumnData::U8(Some(7))), Value::from(7));
        assert_eq!(column_value(&ColumnData::I16(Some(-3))), Value::from(-3));
        assert_eq!(
            column_value(&ColumnData::I32(Some(1_000))),
            Value::from(1_000)
        );
        assert_eq!(column_value(&ColumnData::I64(Some(-9))), Value::from(-9));
        assert_eq!(column_value(&ColumnData::I32(None)), Value::Null);
    }

    /// A float JSON cannot write — NaN, either infinity — reads as null rather than as a number
    /// that is not the one stored, the same choice `postgres::column_value` makes.
    #[test]
    fn a_float_json_cannot_write_becomes_null() {
        assert_eq!(column_value(&ColumnData::F64(Some(1.5))), Value::from(1.5));
        assert_eq!(column_value(&ColumnData::F32(Some(f32::NAN))), Value::Null);
        assert_eq!(
            column_value(&ColumnData::F64(Some(f64::INFINITY))),
            Value::Null
        );
    }

    /// DECIMAL/NUMERIC arrives as text, not as an f64: the precision is the whole point of the
    /// type, and an f64 would round it away before the grid ever saw it.
    #[test]
    fn a_decimal_keeps_its_digits_by_arriving_as_text() {
        let value = column_value(&ColumnData::Numeric(Some(Numeric::new_with_scale(
            12345, 2,
        ))));
        assert_eq!(value, Value::String("123.45".into()));
    }

    /// Bytes have no JSON of their own, so they arrive base64-encoded — which is what
    /// `mssqlDialect.isBinary` then tells the grid to expect.
    #[test]
    fn binary_arrives_base64_encoded() {
        let value = column_value(&ColumnData::Binary(Some(Cow::Borrowed(&[1, 2, 3]))));
        assert_eq!(value, Value::String("AQID".into()));
    }

    /// Dates and times are formatted the way `postgres::column_value` formats them, since one grid
    /// draws either: `2020-01-02 03:04:05`, never a driver's own Debug spelling.
    #[test]
    fn dates_and_times_read_the_way_postgres_writes_them() {
        // 1900-01-01 plus 0 days, at 300 seconds-fragments past midnight — 1/300 of a second each.
        assert_eq!(
            column_value(&ColumnData::DateTime(Some(DateTime::new(0, 300)))),
            Value::String("1900-01-01 00:00:01".into())
        );
        // DateTime2 counts days from year 1 and time in 10^-scale second increments.
        let dt2 = DateTime2::new(Date::new(730_119), Time::new(0, 0));
        assert_eq!(
            column_value(&ColumnData::DateTime2(Some(dt2))),
            Value::String("2000-01-01 00:00:00".into())
        );
        assert_eq!(
            column_value(&ColumnData::Date(Some(Date::new(730_119)))),
            Value::String("2000-01-01".into())
        );
        assert_eq!(
            column_value(&ColumnData::Time(Some(Time::new(3_600, 0)))),
            Value::String("01:00:00".into())
        );
    }

    /// Text is text, and a NULL string is null rather than "".
    #[test]
    fn strings_pass_through_and_a_null_string_is_null() {
        assert_eq!(
            column_value(&ColumnData::String(Some(Cow::Borrowed("hello")))),
            Value::String("hello".into())
        );
        assert_eq!(column_value(&ColumnData::String(None)), Value::Null);
    }

    fn columns() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("id".to_string(), "int".to_string()),
            ("name".to_string(), "nvarchar(255)".to_string()),
        ])
    }

    fn filter(column: &str, operator: &str, value: Option<&str>) -> Filter {
        Filter {
            column: column.to_string(),
            operator: operator.to_string(),
            value: value.map(str::to_string),
        }
    }

    /// T-SQL's `LIKE` reads `[` as the start of a character set — `[0-9]` is a range, not three
    /// characters — which no other engine here does. A `contains` filter for `a[0]` must find the
    /// text `a[0]`, so the bracket is escaped along with the wildcards.
    #[test]
    fn like_escaping_covers_the_bracket_t_sql_treats_as_a_wildcard() {
        assert_eq!(escape_like_mssql("a[0]"), "a\\[0]");
        assert_eq!(escape_like_mssql("50%"), "50\\%");
        assert_eq!(escape_like_mssql("a_b"), "a\\_b");
        // The backslash first, or it would escape the escapes added after it.
        assert_eq!(escape_like_mssql("a\\b"), "a\\\\b");
    }

    /// SQL Server has no default escape character in `LIKE`, so every pattern this builds has to
    /// name one — without `ESCAPE '\'` the backslashes above would be matched literally.
    #[test]
    fn every_like_names_its_escape_character() {
        let (clause, binds) = build_where(&[filter("name", "contains", Some("50%"))], &columns())
            .expect("contains is a known operator");
        assert_eq!(
            clause,
            " WHERE CAST([name] AS nvarchar(max)) LIKE @P1 ESCAPE '\\'"
        );
        assert_eq!(binds, ["%50\\%%"]);
    }

    /// The ordering operators compare the column as it is, so `10 > 9` stays true: casting the
    /// column to text would sort them the other way round.
    #[test]
    fn ordering_compares_the_column_rather_than_its_text() {
        let (clause, binds) = build_where(&[filter("id", "gt", Some("9"))], &columns())
            .expect("gt is a known operator");
        assert_eq!(clause, " WHERE [id] > @P1");
        assert_eq!(binds, ["9"]);
    }

    /// Placeholders are numbered, not counted off at the end: `@P1` and `@P2` have to line up with
    /// the order the values are bound in, and an `IN` list makes as many as it has items.
    #[test]
    fn placeholders_are_numbered_in_bind_order() {
        let filters = [
            filter("name", "eq", Some("a")),
            filter("id", "in", Some("1,2")),
        ];
        let (clause, binds) = build_where(&filters, &columns()).expect("both are known");
        assert_eq!(
            clause,
            " WHERE CAST([name] AS nvarchar(max)) = @P1 \
             AND CAST([id] AS nvarchar(max)) IN (@P2, @P3)"
        );
        assert_eq!(binds, ["a", "1", "2"]);
    }

    /// No regex arm at all — see D12. The dialect closes the operator in the dropdown, and this is
    /// the other half of that: a filter that reached here anyway is an error, not a silent pass.
    #[test]
    fn regexp_is_not_an_operator_this_engine_has() {
        let error = build_where(&[filter("name", "regexp", Some("^a"))], &columns())
            .expect_err("SQL Server has no regex operator");
        assert_eq!(error.code, "error.unknownFilterOperator");
    }

    /// A column not on the table is refused rather than interpolated — the column name is the one
    /// part of a filter that cannot be bound.
    #[test]
    fn a_column_the_table_does_not_have_is_refused() {
        let error = build_where(&[filter("nope", "eq", Some("x"))], &columns())
            .expect_err("unknown column");
        assert_eq!(error.code, "error.unknownFilterColumn");
    }

    /// No filters is no clause — not `WHERE 1=1`, which would be a scan the planner has to see
    /// through on every page.
    #[test]
    fn no_filters_is_no_clause() {
        let (clause, binds) = build_where(&[], &columns()).expect("nothing to fail");
        assert_eq!(clause, "");
        assert!(binds.is_empty());
    }

    /// `sys.columns.max_length` counts **bytes**, so an `nvarchar(255)` reads 510 there — two bytes
    /// to the character. Halved for the Unicode types and left alone for the rest; mixing the two
    /// is how a column ends up declared twice the width it has.
    #[test]
    fn a_unicode_length_is_halved_because_the_catalog_counts_bytes() {
        assert_eq!(display_type("nvarchar", 510, 0, 0), "nvarchar(255)");
        assert_eq!(display_type("nchar", 20, 0, 0), "nchar(10)");
        assert_eq!(display_type("varchar", 255, 0, 0), "varchar(255)");
        assert_eq!(display_type("binary", 8, 0, 0), "binary(8)");
    }

    /// -1 is `MAX`, not a negative width. `varchar(-1)` is not a type anything could declare.
    #[test]
    fn minus_one_is_the_word_max() {
        assert_eq!(display_type("varchar", -1, 0, 0), "varchar(max)");
        assert_eq!(display_type("nvarchar", -1, 0, 0), "nvarchar(max)");
        assert_eq!(display_type("varbinary", -1, 0, 0), "varbinary(max)");
    }

    /// Precision and scale belong to the numeric types, and to them only — an `int` has a precision
    /// in the catalogue too, and writing `int(10)` would be a type SQL Server rejects.
    #[test]
    fn only_the_types_that_take_arguments_get_them() {
        assert_eq!(display_type("decimal", 9, 10, 2), "decimal(10,2)");
        assert_eq!(display_type("numeric", 9, 18, 0), "numeric(18,0)");
        assert_eq!(display_type("int", 4, 10, 0), "int");
        assert_eq!(display_type("bit", 1, 1, 0), "bit");
        assert_eq!(display_type("datetime2", 8, 27, 7), "datetime2(7)");
        assert_eq!(display_type("time", 5, 16, 3), "time(3)");
    }

    /// SQL Server stores a default wrapped in its own parentheses — `((0))`, `(getdate())` — and
    /// showing them would put a pair of brackets on every default in the grid. Only the wrapping
    /// pairs come off: an expression whose own parentheses reach the ends keeps them.
    #[test]
    fn a_default_loses_the_parentheses_the_catalog_added() {
        assert_eq!(strip_default_parens("((0))"), "0");
        assert_eq!(strip_default_parens("(getdate())"), "getdate()");
        assert_eq!(strip_default_parens("('new')"), "'new'");
        // Not one wrapping pair: the first `(` closes before the end.
        assert_eq!(strip_default_parens("(a)+(b)"), "(a)+(b)");
    }

    /// The tokens `ColumnMeta::extra` carries, which `src/modules/db/mssql/columns.ts` reads back.
    /// `rowversion` is on the list because an INSERT must not name such a column either — see D7.
    #[test]
    fn extra_names_every_kind_of_column_the_server_fills_in() {
        assert_eq!(extra_tokens(true, false, false), "identity");
        assert_eq!(extra_tokens(false, true, false), "generated");
        assert_eq!(extra_tokens(false, false, true), "rowversion");
        assert_eq!(extra_tokens(true, false, true), "identity rowversion");
        assert_eq!(extra_tokens(false, false, false), "");
    }

    /// A pooled session may be sitting on any database at all (D2), so every read names its own —
    /// three parts, each bracketed.
    #[test]
    fn a_read_names_its_database_rather_than_switching_to_it() {
        assert_eq!(three_part("shop", "dbo", "users"), "[shop].[dbo].[users]");
        assert_eq!(three_part("a b", "sales", "Order"), "[a b].[sales].[Order]");
    }

    /// Most columns are asked for as they are. The ones that are not are the ones tiberius would
    /// decode into something lossy or into nothing at all — `money` through an f64 loses its fourth
    /// decimal place, and the three CLR types have no `ColumnData` of their own.
    #[test]
    fn a_lossy_column_is_asked_for_as_text_instead() {
        assert_eq!(select_expr("[id]", "int"), "[id]");
        assert_eq!(select_expr("[name]", "nvarchar(255)"), "[name]");
        // Style 2, not a plain CAST: the default style rounds a `money` to two decimal places, so
        // a column holding 9.9999 would arrive as 10.00 — found on the live server, not in review.
        assert_eq!(
            select_expr("[price]", "money"),
            "CONVERT(nvarchar(max), [price], 2) AS [price]"
        );
        assert_eq!(
            select_expr("[doc]", "xml"),
            "CAST([doc] AS nvarchar(max)) AS [doc]"
        );
        // A CLR type has no CAST to text at all - the method is how it is read.
        assert_eq!(
            select_expr("[area]", "geography"),
            "[area].ToString() AS [area]"
        );
    }

    /// `OFFSET ... FETCH` is a clause of `ORDER BY`, so SQL Server refuses a page without one —
    /// unlike `LIMIT`, which stands alone on MySQL and PostgreSQL. With no column chosen the order
    /// asked for is `(SELECT NULL)`: it satisfies the syntax without making the server sort
    /// anything, where ordering by "the first column" would sort the whole table on every page turn
    /// of an unindexed column.
    #[test]
    fn a_page_with_no_sort_column_still_names_an_order() {
        let columns = ["id".to_string(), "name".to_string()];
        assert_eq!(
            order_by_clause(None, false, &columns),
            " ORDER BY (SELECT NULL)"
        );
        assert_eq!(
            order_by_clause(Some("name"), true, &columns),
            " ORDER BY [name] DESC"
        );
        // A sort column that is not a column of this table is ignored rather than interpolated.
        assert_eq!(
            order_by_clause(Some("; DROP TABLE users"), false, &columns),
            " ORDER BY (SELECT NULL)"
        );
    }

    /// SQL Server brackets an identifier rather than quoting it, and the character doubled inside
    /// is the *closing* bracket — the one asymmetry no other engine here has.
    #[test]
    fn an_identifier_is_bracketed_and_its_closing_bracket_doubled() {
        assert_eq!(quote_ident("users"), "[users]");
        assert_eq!(quote_ident("Order Details"), "[Order Details]");
        assert_eq!(quote_ident("a]b"), "[a]]b]");
        // An opening bracket needs no escape: it only opens inside a name that is already open.
        assert_eq!(quote_ident("a[b"), "[a[b]");
    }

    /// A table of `dbo` reads bare, the way a table of `public` does on PostgreSQL — that is what
    /// keeps a database using only the default schema looking the same as a MySQL one.
    #[test]
    fn a_dbo_table_is_named_without_its_schema() {
        assert_eq!(qualify("dbo", "users"), "users");
        assert_eq!(qualify("sales", "orders"), "sales.orders");
    }

    /// Only a name that would be misread gets brackets, so the common case stays readable.
    #[test]
    fn only_a_name_that_would_be_misread_is_bracketed() {
        assert_eq!(qualify("dbo", "a.b"), "[a.b]");
        assert_eq!(qualify("dbo", "a]b"), "[a]]b]");
        assert_eq!(qualify("sa les", "orders"), "sa les.orders");
    }

    /// `resolve` is `qualify` backwards: whatever the sidebar shows has to name the same table
    /// again when it comes back down.
    #[test]
    fn resolve_undoes_qualify() {
        for (schema, table) in [
            ("dbo", "users"),
            ("sales", "orders"),
            ("dbo", "a.b"),
            ("dbo", "a]b"),
            ("we ird", "a.b"),
        ] {
            let round_tripped = resolve(&qualify(schema, table));
            assert_eq!(round_tripped, (schema.to_string(), table.to_string()));
        }
    }

    /// The write-side counterpart of `src/modules/db/mssql/columns.ts`'s `isBinary` — the two must
    /// agree, because this is what decides whether a value gets base64-decoded before it is bound.
    /// `rowversion`/`timestamp` is on the list for the same reason it is on that one: it is eight
    /// raw bytes, base64-encoded on the way out by `column_value`'s `Binary` arm, even though it is
    /// also server-assigned and never itself the target of a write.
    #[test]
    fn binary_types_match_the_frontend_list() {
        assert!(is_binary_type("varbinary(max)"));
        assert!(is_binary_type("binary(8)"));
        assert!(is_binary_type("image"));
        assert!(is_binary_type("rowversion"));
        assert!(is_binary_type("timestamp"));
        assert!(!is_binary_type("nvarchar(255)"));
        assert!(!is_binary_type("int"));
    }

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
