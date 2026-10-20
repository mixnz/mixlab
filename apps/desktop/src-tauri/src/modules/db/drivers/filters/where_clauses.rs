//! The exact `WHERE` each SQL engine writes for every operator of the filter bar, and the values it
//! binds. These are the SQL the engines sent before their five copies became one function, written
//! out in full, so that any change to what reaches a server shows here as a failing line.

use crate::modules::db::drivers::sql::types::Filter;
use crate::modules::db::drivers::{clickhouse, mssql, mysql, postgres, sqlite};
use std::collections::BTreeMap;

/// What a call gave: the clause behind ` WHERE ` and the binds, or the error's code.
type Outcome = Result<(String, Vec<String>), &'static str>;

/// One engine's `build_where` over the columns `id` and `name`, with ClickHouse's named
/// parameters written `name=value` so that every engine's binds compare as text.
type Engine = fn(&[Filter]) -> Outcome;

/// Every operator, one filter at a time: the operator, the column and the value.
const CASES: [(&str, &str, Option<&str>); 22] = [
    ("eq", "name", Some("x")),
    ("ne", "name", Some("x")),
    ("gt", "id", Some("5")),
    ("gte", "id", Some("5")),
    ("lt", "id", Some("5")),
    ("lte", "id", Some("5")),
    ("contains", "name", Some("50%_[")),
    ("notContains", "name", Some("50%_[")),
    ("startsWith", "name", Some("50%_[")),
    ("endsWith", "name", Some("50%_[")),
    ("like", "name", Some("a%")),
    ("notLike", "name", Some("a%")),
    ("regexp", "name", Some("a%")),
    ("notRegexp", "name", Some("a%")),
    ("in", "name", Some("x, 'y,z'")),
    ("notIn", "name", Some("x, 'y,z'")),
    // Three items, of which a range takes the first two.
    ("between", "id", Some("1,9,20")),
    ("notBetween", "id", Some("1,9,20")),
    ("isNull", "id", None),
    ("isNotNull", "id", None),
    ("isEmpty", "name", None),
    ("isNotEmpty", "name", None),
];

/// What a case must give: the clause without its leading ` WHERE `, and the binds. A clause that
/// starts with `!` is the code of the error the call must fail with.
type Expected = [(&'static str, &'static [&'static str]); 22];

const REFUSED: &str = "!error.unknownFilterOperator";

fn filter(column: &str, operator: &str, value: Option<&str>) -> Filter {
    Filter {
        column: column.to_string(),
        operator: operator.to_string(),
        value: value.map(str::to_string),
    }
}

fn names() -> Vec<String> {
    vec!["id".to_string(), "name".to_string()]
}

fn typed(id: &str, name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("id".to_string(), id.to_string()),
        ("name".to_string(), name.to_string()),
    ])
}

fn mysql(filters: &[Filter]) -> Outcome {
    mysql::build_where(filters, &names()).map_err(|e| e.code)
}

fn sqlite(filters: &[Filter]) -> Outcome {
    sqlite::build_where(filters, &names()).map_err(|e| e.code)
}

fn postgres(filters: &[Filter]) -> Outcome {
    postgres::page::build_where(filters, &typed("integer", "text")).map_err(|e| e.code)
}

fn mssql(filters: &[Filter]) -> Outcome {
    mssql::page::build_where(filters, &typed("int", "nvarchar")).map_err(|e| e.code)
}

fn clickhouse(filters: &[Filter]) -> Outcome {
    clickhouse::page::build_where(filters, &typed("Nullable(UInt64)", "String"))
        .map(|(clause, params)| {
            let binds = params
                .into_iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            (clause, binds)
        })
        .map_err(|e| e.code)
}

fn assert_every_operator(engine: Engine, expected: &Expected) {
    for ((operator, column, value), (clause, binds)) in CASES.iter().zip(expected) {
        let outcome = engine(&[filter(column, operator, *value)]);
        let wanted: Outcome = match clause.strip_prefix('!') {
            Some(code) => Err(code),
            None => Ok((
                format!(" WHERE {clause}"),
                binds.iter().map(|b| b.to_string()).collect(),
            )),
        };
        assert_eq!(outcome, wanted, "operator `{operator}`");
    }
}

/// The cases every engine shares beyond one operator at a time: how placeholders are numbered
/// across rows, that a dropped row takes no number, what is refused first, and the empty value.
struct Rows {
    /// `eq name x` and then `between id 1,9`.
    two_rows: (&'static str, &'static [&'static str]),
    /// An empty `in`, a one-item `between`, then `eq name x`: only the last says anything.
    after_dropped_rows: (&'static str, &'static [&'static str]),
    /// `eq name` with no value at all.
    no_value: (&'static str, &'static [&'static str]),
}

fn assert_rows(engine: Engine, rows: &Rows) {
    let ok = |(clause, binds): (&str, &[&str])| -> Outcome {
        Ok((
            format!(" WHERE {clause}"),
            binds.iter().map(|b| b.to_string()).collect(),
        ))
    };

    let two_rows = engine(&[
        filter("name", "eq", Some("x")),
        filter("id", "between", Some("1,9")),
    ]);
    assert_eq!(
        two_rows,
        ok(rows.two_rows),
        "placeholders run on across rows"
    );

    let after_dropped_rows = engine(&[
        filter("name", "in", Some("")),
        filter("id", "between", Some("1")),
        filter("name", "eq", Some("x")),
    ]);
    assert_eq!(
        after_dropped_rows,
        ok(rows.after_dropped_rows),
        "a dropped row takes no placeholder"
    );

    assert_eq!(
        engine(&[filter("name", "eq", None)]),
        ok(rows.no_value),
        "a row without a value binds the empty string"
    );

    assert_eq!(
        engine(&[filter("nope", "sqli", Some("1"))]),
        Err("error.unknownFilterColumn"),
        "an unknown column is refused before its operator is read"
    );

    assert_eq!(
        engine(&[]),
        Ok((String::new(), Vec::new())),
        "no filters is no clause"
    );
}

#[test]
fn mysql_writes_every_operator_as_it_did() {
    assert_every_operator(
        mysql,
        &[
            ("`name` = ?", &["x"]),
            ("`name` <> ?", &["x"]),
            ("`id` > ?", &["5"]),
            ("`id` >= ?", &["5"]),
            ("`id` < ?", &["5"]),
            ("`id` <= ?", &["5"]),
            ("`name` LIKE ?", &[r"%50\%\_[%"]),
            ("`name` NOT LIKE ?", &[r"%50\%\_[%"]),
            ("`name` LIKE ?", &[r"50\%\_[%"]),
            ("`name` LIKE ?", &[r"%50\%\_["]),
            ("`name` LIKE ?", &["a%"]),
            ("`name` NOT LIKE ?", &["a%"]),
            ("`name` REGEXP ?", &["a%"]),
            ("`name` NOT REGEXP ?", &["a%"]),
            ("`name` IN (?, ?)", &["x", "y,z"]),
            ("`name` NOT IN (?, ?)", &["x", "y,z"]),
            ("`id` BETWEEN ? AND ?", &["1", "9"]),
            ("`id` NOT BETWEEN ? AND ?", &["1", "9"]),
            ("`id` IS NULL", &[]),
            ("`id` IS NOT NULL", &[]),
            ("`name` = ''", &[]),
            ("`name` <> ''", &[]),
        ],
    );
}

#[test]
fn mysql_numbers_rows_as_it_did() {
    assert_rows(
        mysql,
        &Rows {
            two_rows: ("`name` = ? AND `id` BETWEEN ? AND ?", &["x", "1", "9"]),
            after_dropped_rows: ("`name` = ?", &["x"]),
            no_value: ("`name` = ?", &[""]),
        },
    );
}

#[test]
fn sqlite_writes_every_operator_as_it_did() {
    assert_every_operator(
        sqlite,
        &[
            (r#"CAST("name" AS TEXT) = ?"#, &["x"]),
            (r#"CAST("name" AS TEXT) <> ?"#, &["x"]),
            (r#""id" > ?"#, &["5"]),
            (r#""id" >= ?"#, &["5"]),
            (r#""id" < ?"#, &["5"]),
            (r#""id" <= ?"#, &["5"]),
            (r#"CAST("name" AS TEXT) LIKE ? ESCAPE '\'"#, &[r"%50\%\_[%"]),
            (
                r#"CAST("name" AS TEXT) NOT LIKE ? ESCAPE '\'"#,
                &[r"%50\%\_[%"],
            ),
            (r#"CAST("name" AS TEXT) LIKE ? ESCAPE '\'"#, &[r"50\%\_[%"]),
            (r#"CAST("name" AS TEXT) LIKE ? ESCAPE '\'"#, &[r"%50\%\_["]),
            (r#"CAST("name" AS TEXT) LIKE ?"#, &["a%"]),
            (r#"CAST("name" AS TEXT) NOT LIKE ?"#, &["a%"]),
            (REFUSED, &[]),
            (REFUSED, &[]),
            (r#"CAST("name" AS TEXT) IN (?, ?)"#, &["x", "y,z"]),
            (r#"CAST("name" AS TEXT) NOT IN (?, ?)"#, &["x", "y,z"]),
            (r#""id" BETWEEN ? AND ?"#, &["1", "9"]),
            (r#""id" NOT BETWEEN ? AND ?"#, &["1", "9"]),
            (r#""id" IS NULL"#, &[]),
            (r#""id" IS NOT NULL"#, &[]),
            (r#"CAST("name" AS TEXT) = ''"#, &[]),
            (r#"CAST("name" AS TEXT) <> ''"#, &[]),
        ],
    );
}

#[test]
fn sqlite_numbers_rows_as_it_did() {
    assert_rows(
        sqlite,
        &Rows {
            two_rows: (
                r#"CAST("name" AS TEXT) = ? AND "id" BETWEEN ? AND ?"#,
                &["x", "1", "9"],
            ),
            after_dropped_rows: (r#"CAST("name" AS TEXT) = ?"#, &["x"]),
            no_value: (r#"CAST("name" AS TEXT) = ?"#, &[""]),
        },
    );
}

#[test]
fn postgres_writes_every_operator_as_it_did() {
    assert_every_operator(
        postgres,
        &[
            (r#""name"::text = $1"#, &["x"]),
            (r#""name"::text <> $1"#, &["x"]),
            (r#""id" > CAST($1 AS integer)"#, &["5"]),
            (r#""id" >= CAST($1 AS integer)"#, &["5"]),
            (r#""id" < CAST($1 AS integer)"#, &["5"]),
            (r#""id" <= CAST($1 AS integer)"#, &["5"]),
            (r#""name"::text LIKE $1"#, &[r"%50\%\_[%"]),
            (r#""name"::text NOT LIKE $1"#, &[r"%50\%\_[%"]),
            (r#""name"::text LIKE $1"#, &[r"50\%\_[%"]),
            (r#""name"::text LIKE $1"#, &[r"%50\%\_["]),
            (r#""name"::text LIKE $1"#, &["a%"]),
            (r#""name"::text NOT LIKE $1"#, &["a%"]),
            (r#""name"::text ~ $1"#, &["a%"]),
            (r#""name"::text !~ $1"#, &["a%"]),
            (r#""name"::text IN ($1, $2)"#, &["x", "y,z"]),
            (r#""name"::text NOT IN ($1, $2)"#, &["x", "y,z"]),
            (
                r#""id" BETWEEN CAST($1 AS integer) AND CAST($2 AS integer)"#,
                &["1", "9"],
            ),
            (
                r#""id" NOT BETWEEN CAST($1 AS integer) AND CAST($2 AS integer)"#,
                &["1", "9"],
            ),
            (r#""id" IS NULL"#, &[]),
            (r#""id" IS NOT NULL"#, &[]),
            (r#""name"::text = ''"#, &[]),
            (r#""name"::text <> ''"#, &[]),
        ],
    );
}

#[test]
fn postgres_numbers_rows_as_it_did() {
    assert_rows(
        postgres,
        &Rows {
            two_rows: (
                r#""name"::text = $1 AND "id" BETWEEN CAST($2 AS integer) AND CAST($3 AS integer)"#,
                &["x", "1", "9"],
            ),
            after_dropped_rows: (r#""name"::text = $1"#, &["x"]),
            no_value: (r#""name"::text = $1"#, &[""]),
        },
    );
}

#[test]
fn mssql_writes_every_operator_as_it_did() {
    assert_every_operator(
        mssql,
        &[
            ("CAST([name] AS nvarchar(max)) = @P1", &["x"]),
            ("CAST([name] AS nvarchar(max)) <> @P1", &["x"]),
            ("[id] > @P1", &["5"]),
            ("[id] >= @P1", &["5"]),
            ("[id] < @P1", &["5"]),
            ("[id] <= @P1", &["5"]),
            (
                r"CAST([name] AS nvarchar(max)) LIKE @P1 ESCAPE '\'",
                &[r"%50\%\_\[%"],
            ),
            (
                r"CAST([name] AS nvarchar(max)) NOT LIKE @P1 ESCAPE '\'",
                &[r"%50\%\_\[%"],
            ),
            (
                r"CAST([name] AS nvarchar(max)) LIKE @P1 ESCAPE '\'",
                &[r"50\%\_\[%"],
            ),
            (
                r"CAST([name] AS nvarchar(max)) LIKE @P1 ESCAPE '\'",
                &[r"%50\%\_\["],
            ),
            (
                r"CAST([name] AS nvarchar(max)) LIKE @P1 ESCAPE '\'",
                &["a%"],
            ),
            (
                r"CAST([name] AS nvarchar(max)) NOT LIKE @P1 ESCAPE '\'",
                &["a%"],
            ),
            (REFUSED, &[]),
            (REFUSED, &[]),
            ("CAST([name] AS nvarchar(max)) IN (@P1, @P2)", &["x", "y,z"]),
            (
                "CAST([name] AS nvarchar(max)) NOT IN (@P1, @P2)",
                &["x", "y,z"],
            ),
            ("[id] BETWEEN @P1 AND @P2", &["1", "9"]),
            ("[id] NOT BETWEEN @P1 AND @P2", &["1", "9"]),
            ("[id] IS NULL", &[]),
            ("[id] IS NOT NULL", &[]),
            ("CAST([name] AS nvarchar(max)) = ''", &[]),
            ("CAST([name] AS nvarchar(max)) <> ''", &[]),
        ],
    );
}

#[test]
fn mssql_numbers_rows_as_it_did() {
    assert_rows(
        mssql,
        &Rows {
            two_rows: (
                "CAST([name] AS nvarchar(max)) = @P1 AND [id] BETWEEN @P2 AND @P3",
                &["x", "1", "9"],
            ),
            after_dropped_rows: ("CAST([name] AS nvarchar(max)) = @P1", &["x"]),
            no_value: ("CAST([name] AS nvarchar(max)) = @P1", &[""]),
        },
    );
}

#[test]
fn clickhouse_writes_every_operator_as_it_did() {
    assert_every_operator(
        clickhouse,
        &[
            ("toString(`name`) = {p0:String}", &["p0=x"]),
            ("toString(`name`) <> {p0:String}", &["p0=x"]),
            ("`id` > {p0:Nullable(UInt64)}", &["p0=5"]),
            ("`id` >= {p0:Nullable(UInt64)}", &["p0=5"]),
            ("`id` < {p0:Nullable(UInt64)}", &["p0=5"]),
            ("`id` <= {p0:Nullable(UInt64)}", &["p0=5"]),
            ("toString(`name`) LIKE {p0:String}", &[r"p0=%50\%\_[%"]),
            ("toString(`name`) NOT LIKE {p0:String}", &[r"p0=%50\%\_[%"]),
            ("toString(`name`) LIKE {p0:String}", &[r"p0=50\%\_[%"]),
            ("toString(`name`) LIKE {p0:String}", &[r"p0=%50\%\_["]),
            ("toString(`name`) LIKE {p0:String}", &["p0=a%"]),
            ("toString(`name`) NOT LIKE {p0:String}", &["p0=a%"]),
            (REFUSED, &[]),
            (REFUSED, &[]),
            (
                "toString(`name`) IN ({p0:String}, {p1:String})",
                &["p0=x", "p1=y,z"],
            ),
            (
                "toString(`name`) NOT IN ({p0:String}, {p1:String})",
                &["p0=x", "p1=y,z"],
            ),
            (
                "`id` BETWEEN {p0:Nullable(UInt64)} AND {p1:Nullable(UInt64)}",
                &["p0=1", "p1=9"],
            ),
            (
                "`id` NOT BETWEEN {p0:Nullable(UInt64)} AND {p1:Nullable(UInt64)}",
                &["p0=1", "p1=9"],
            ),
            ("`id` IS NULL", &[]),
            ("`id` IS NOT NULL", &[]),
            ("toString(`name`) = ''", &[]),
            ("toString(`name`) <> ''", &[]),
        ],
    );
}

#[test]
fn clickhouse_numbers_rows_as_it_did() {
    assert_rows(
        clickhouse,
        &Rows {
            two_rows: (
                "toString(`name`) = {p0:String} AND `id` BETWEEN {p1:Nullable(UInt64)} AND {p2:Nullable(UInt64)}",
                &["p0=x", "p1=1", "p2=9"],
            ),
            after_dropped_rows: ("toString(`name`) = {p0:String}", &["p0=x"]),
            no_value: ("toString(`name`) = {p0:String}", &["p0="]),
        },
    );
}
