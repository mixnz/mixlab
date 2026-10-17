//! SQL Server, over TDS rather than through sqlx — see
//! `docs/specs/2026-09-05-mssql-support-design.md` (D1..D15).
//!
//! Closest to `postgres.rs` in shape, since both put a schema between the database and its tables
//! and so name a table `schema.table`; closest to `mysql.rs` in connection model, since both reach
//! every database of the server over one pool rather than dialing again per database.

pub mod ddl;
pub mod dump;
pub mod editing;
pub mod engine;
pub mod script;
pub mod structure;

use super::filters::split_list;
use crate::error::AppError;
pub use crate::modules::db::drivers::sql::types::{
    ColumnMeta, Filter, ForeignKey, PageQuery, TablePage,
};
use crate::modules::db::models::ServerInfo;
use deadpool::managed::{
    Manager as ManagerTrait, Metrics, Pool as DeadPool, RecycleError, RecycleResult,
};
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
pub(super) const MONEY_TYPES: [&str; 2] = ["money", "smallmoney"];

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

#[cfg(test)]
mod tests;
