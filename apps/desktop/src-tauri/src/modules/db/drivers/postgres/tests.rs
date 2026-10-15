use super::{build_where, is_decodable, map_error, qualify, quote_ident, resolve, Filter};
use super::{key_predicate, placeholder};
use serde_json::{Map, Value};

/// A placeholder carries the column's own type with it.
///
/// Every value the frontend sends is text, so without the cast PostgreSQL sees `$1` as
/// `unknown` and refuses to compare it to an `integer` or a `timestamptz` — the update fails
/// on a type error rather than on anything the user did.
#[test]
fn a_placeholder_is_cast_to_the_column_it_stands_for() {
    let mut types = BTreeMap::new();
    types.insert("age".to_string(), "integer".to_string());
    types.insert("tags".to_string(), "text[]".to_string());

    assert_eq!(placeholder(1, "age", &types), "$1::integer");
    assert_eq!(placeholder(2, "tags", &types), "$2::text[]");
    // A column that is not in the catalogue does not exist; the statement will say so, and
    // saying so is better than casting to a type invented here.
    assert_eq!(placeholder(3, "nope", &types), "$3");
}

/// `IS NOT DISTINCT FROM`, not `=`.
///
/// A key column that is itself NULL compares to NULL as NULL under `=` — which is not true —
/// so a row with a NULL in its key could never be found again after being edited once.
#[test]
fn a_key_predicate_can_match_a_null_key() {
    let mut types = BTreeMap::new();
    types.insert("id".to_string(), "integer".to_string());
    types.insert("region".to_string(), "text".to_string());

    let mut key = Map::new();
    key.insert("id".to_string(), Value::String("7".to_string()));
    assert_eq!(
        key_predicate(&key, &types, 1),
        r#""id" IS NOT DISTINCT FROM $1::integer"#,
    );

    // A composite key, numbered on from where the caller left off — the placeholders before
    // it belong to the columns being written.
    key.insert("region".to_string(), Value::Null);
    assert_eq!(
        key_predicate(&key, &types, 4),
        r#""id" IS NOT DISTINCT FROM $4::integer AND "region" IS NOT DISTINCT FROM $5::text"#,
    );
}

use std::collections::BTreeMap;

fn filter(column: &str, operator: &str, value: Option<&str>) -> Filter {
    Filter {
        column: column.to_string(),
        operator: operator.to_string(),
        value: value.map(str::to_string),
    }
}

/// The table the filters below are written against: what a column is, as `format_type` spells
/// it without its modifier.
fn columns() -> BTreeMap<String, String> {
    [("id", "integer"), ("name", "text")]
        .into_iter()
        .map(|(name, base_type)| (name.to_string(), base_type.to_string()))
        .collect()
}

#[test]
fn quotes_identifiers_the_way_postgres_does() {
    assert_eq!(quote_ident("users"), "\"users\"");
    assert_eq!(quote_ident("od\"d"), "\"od\"\"d\"");
}

/// A table of `public` reads as its bare name, which is what makes a database that uses only
/// that schema look exactly as it did under MySQL.
#[test]
fn names_public_tables_without_a_prefix() {
    assert_eq!(qualify("public", "users"), "users");
    assert_eq!(qualify("sales", "orders"), "sales.orders");
}

/// The point of the quoting: a name holding the separator still comes back as itself.
#[test]
fn qualifying_survives_a_round_trip() {
    for (schema, table) in [
        ("public", "users"),
        ("sales", "orders"),
        ("public", "odd.name"),
        ("odd.schema", "t"),
        ("public", "quo\"ted"),
    ] {
        let qualified = qualify(schema, table);
        assert_eq!(
            resolve(&qualified),
            (schema.to_string(), table.to_string()),
            "round trip of {qualified}"
        );
    }
}

/// Placeholders are numbered rather than positional, so every value has to line up with the
/// `$n` that was minted for it.
#[test]
fn numbers_placeholders_in_order() {
    let filters = vec![
        filter("id", "eq", Some("1")),
        filter("name", "contains", Some("ann")),
    ];
    let (clause, binds) = build_where(&filters, &columns()).unwrap();
    assert_eq!(
        clause,
        " WHERE \"id\"::text = $1 AND \"name\"::text LIKE $2"
    );
    assert_eq!(binds, ["1", "%ann%"]);
}

/// A list operator mints one placeholder per item, and the numbering carries on past it.
#[test]
fn numbers_a_list_and_what_follows_it() {
    let filters = vec![
        filter("id", "in", Some("1,2,3")),
        filter("name", "eq", Some("x")),
    ];
    let (clause, binds) = build_where(&filters, &columns()).unwrap();
    assert_eq!(
        clause,
        " WHERE \"id\"::text IN ($1, $2, $3) AND \"name\"::text = $4"
    );
    assert_eq!(binds, ["1", "2", "3", "x"]);
}

/// The ordering operators compare the column itself, not its text: `>` on text would put 10
/// before 9. The value is cast instead — without that the server is asked to compare an
/// `integer` with the `text` sqlx names the bound parameter, and refuses.
#[test]
fn compares_ranges_on_the_column_with_the_value_cast_to_its_type() {
    let (clause, _) = build_where(&[filter("id", "gt", Some("5"))], &columns()).unwrap();
    assert_eq!(clause, " WHERE \"id\" > CAST($1 AS integer)");

    let (clause, binds) = build_where(&[filter("id", "between", Some("5,9"))], &columns()).unwrap();
    assert_eq!(
        clause,
        " WHERE \"id\" BETWEEN CAST($1 AS integer) AND CAST($2 AS integer)"
    );
    assert_eq!(binds, ["5", "9"]);
}

/// An array's `[]` comes after the type's modifier, so a decoder is looked for under the whole
/// spelling: `character varying(255)[]` is an array, and asking sqlx to decode it as a string
/// would show NULL where a value is.
#[test]
fn reads_an_array_of_a_type_that_takes_a_modifier_as_text() {
    assert!(!is_decodable("character varying(255)[]"));
    assert!(!is_decodable("numeric(10,2)[]"));
    assert!(!is_decodable("text[]"));
    assert!(is_decodable("character varying(255)"));
    assert!(is_decodable("timestamp without time zone"));
}

/// The number types are read as text although decoders for them exist: `NaN` and the infinities
/// are values every one of these columns may hold and none of the decoders can carry — Decimal
/// refuses them, and JSON has no way to write them — so they would arrive as null and read in
/// the grid as NULL.
#[test]
fn reads_the_numbers_that_can_be_nan_as_text() {
    assert!(!is_decodable("numeric"));
    assert!(!is_decodable("numeric(10,2)"));
    assert!(!is_decodable("real"));
    assert!(!is_decodable("double precision"));
    // The integers cannot be NaN, and still decode.
    assert!(is_decodable("integer"));
    assert!(is_decodable("bigint"));
}

/// A row whose operator wants a value it wasn't given is dropped rather than matched
/// literally — the bar's opening `id =` row must not become a condition before it is typed in.
#[test]
fn drops_a_list_row_with_nothing_in_it() {
    let (clause, binds) = build_where(&[filter("id", "in", Some(" "))], &columns()).unwrap();
    assert!(clause.is_empty());
    assert!(binds.is_empty());
}

/// A column the table does not have never reaches the SQL text.
#[test]
fn refuses_a_column_the_table_does_not_have() {
    assert!(build_where(&[filter("nope", "eq", Some("1"))], &columns()).is_err());
}

#[test]
fn postgres_tells_a_lost_connection_from_a_server_error() {
    let eof = sqlx::Error::Io(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "expected to read 4 bytes, got 0 bytes at EOF",
    ));
    assert_eq!(map_error(eof).code, "error.connectionLost");
    assert_eq!(
        map_error(sqlx::Error::PoolTimedOut).code,
        "error.connectionLost"
    );
    assert_eq!(map_error(sqlx::Error::RowNotFound).code, "error.postgres");
}
