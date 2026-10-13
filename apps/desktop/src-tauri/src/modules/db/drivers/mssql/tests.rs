use super::{
    build_where, column_value, display_type, escape_like_mssql, extra_tokens, is_binary_type,
    order_by_clause, qualify, quote_ident, resolve, select_expr, strip_default_parens, three_part,
    Filter,
};
use serde_json::Value;
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
    let (clause, binds) =
        build_where(&[filter("id", "gt", Some("9"))], &columns()).expect("gt is a known operator");
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
    let error =
        build_where(&[filter("nope", "eq", Some("x"))], &columns()).expect_err("unknown column");
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
