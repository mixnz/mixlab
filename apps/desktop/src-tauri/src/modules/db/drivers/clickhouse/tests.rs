use super::page::build_where;
use super::{build_key_where, parse_written_rows, quote_literal};
use super::{is_decodable, quote_ident, Filter, QueryResult};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

fn str_val(s: &str) -> Value {
    Value::String(s.to_string())
}

#[test]
fn parse_written_rows_reads_the_string_field() {
    let header = r#"{"read_rows":"0","written_rows":"2","written_bytes":"18"}"#;
    assert_eq!(parse_written_rows(header), 2);
}

#[test]
fn parse_written_rows_defaults_to_zero_when_the_field_is_missing() {
    assert_eq!(parse_written_rows(r#"{"read_rows":"0"}"#), 0);
}

#[test]
fn parse_written_rows_defaults_to_zero_on_malformed_json() {
    assert_eq!(parse_written_rows("not json"), 0);
    assert_eq!(parse_written_rows(""), 0);
}

/// `FORMAT JSON`'s own shape, exactly as the server sends it — the fixture this module's
/// deserialization is checked against without a server in the room.
#[test]
fn reads_clickhouse_s_own_json_format() {
    let text = r#"{
            "meta": [
                {"name": "id", "type": "UInt64"},
                {"name": "name", "type": "String"}
            ],
            "data": [
                {"id": "1", "name": "a"},
                {"id": "2", "name": "b"}
            ],
            "rows": 2
        }"#;
    let result: QueryResult = serde_json::from_str(text).unwrap();
    assert_eq!(result.data.len(), 2);
    assert_eq!(result.data[1]["name"], "b");
}

/// A statement that returns nothing — `CREATE TABLE`, or a `SELECT` with no rows — still
/// parses: `data` is `#[serde(default)]` for exactly this.
#[test]
fn reads_a_result_with_no_rows() {
    let text = r#"{"meta": [{"name": "id", "type": "UInt64"}], "data": [], "rows": 0}"#;
    let result: QueryResult = serde_json::from_str(text).unwrap();
    assert!(result.data.is_empty());
}

/// Backslash, not doubling — checked against the test server, see `quote_ident`'s own doc.
#[test]
fn quotes_identifiers_the_way_clickhouse_does() {
    assert_eq!(quote_ident("events"), "`events`");
    assert_eq!(quote_ident("a`b"), "`a\\`b`");
}

#[test]
fn scalar_types_and_their_nullable_wrapper_decode_directly() {
    for ty in [
        "UInt64",
        "String",
        "Float64",
        "DateTime64(3)",
        "Decimal(10, 2)",
        "Bool",
    ] {
        assert!(is_decodable(ty), "{ty}");
        assert!(is_decodable(&format!("Nullable({ty})")), "Nullable({ty})");
    }
}

#[test]
fn complex_types_are_read_as_text_instead() {
    for ty in [
        "Array(String)",
        "Map(String, UInt64)",
        "Tuple(UInt64, String)",
        "LowCardinality(String)",
        "AggregateFunction(sum, UInt64)",
        "SimpleAggregateFunction(sum, UInt64)",
        "IPv4",
        "IPv6",
    ] {
        assert!(!is_decodable(ty), "{ty}");
    }
}

fn filter(column: &str, operator: &str, value: Option<&str>) -> Filter {
    Filter {
        column: column.to_string(),
        operator: operator.to_string(),
        value: value.map(str::to_string),
    }
}

fn columns() -> BTreeMap<String, String> {
    [("id", "UInt64"), ("name", "String")]
        .into_iter()
        .map(|(name, ty)| (name.to_string(), ty.to_string()))
        .collect()
}

/// Every comparison is `toString(col) = {pN:String}` — never the raw value spliced into the
/// text — and each placeholder gets its own name so the caller can send every value as a
/// separate URL parameter.
#[test]
fn builds_a_parameterized_where_clause() {
    let filters = vec![
        filter("id", "eq", Some("1")),
        filter("name", "contains", Some("ann")),
    ];
    let (clause, params) = build_where(&filters, &columns()).unwrap();
    assert_eq!(
        clause,
        " WHERE toString(`id`) = {p0:String} AND toString(`name`) LIKE {p1:String}"
    );
    assert_eq!(
        params,
        [
            ("p0".to_string(), "1".to_string()),
            ("p1".to_string(), "%ann%".to_string())
        ]
    );
}

/// The ordering operators compare the raw column, typed to match it — `toString()` on a number
/// would sort `10` before `9`.
#[test]
fn orders_by_the_columns_own_type_rather_than_text() {
    let (clause, params) = build_where(&[filter("id", "gt", Some("5"))], &columns()).unwrap();
    assert_eq!(clause, " WHERE `id` > {p0:UInt64}");
    assert_eq!(params, [("p0".to_string(), "5".to_string())]);
}

#[test]
fn a_list_operator_mints_one_placeholder_per_item() {
    let (clause, params) = build_where(&[filter("id", "in", Some("1,2,3"))], &columns()).unwrap();
    assert_eq!(
        clause,
        " WHERE toString(`id`) IN ({p0:String}, {p1:String}, {p2:String})"
    );
    assert_eq!(params.len(), 3);
}

#[test]
fn a_column_the_table_does_not_have_is_refused() {
    assert!(build_where(&[filter("nope", "eq", Some("1"))], &columns()).is_err());
}

#[test]
fn quotes_a_literal_backslash_escaped_like_an_identifier() {
    assert_eq!(quote_literal("ann"), "'ann'");
    assert_eq!(quote_literal("a'b"), "'a\\'b'");
    assert_eq!(quote_literal("a\\b"), "'a\\\\b'");
}

#[test]
fn matches_a_decodable_column_directly() {
    let mut key = Map::new();
    key.insert("id".to_string(), str_val("1"));
    let where_clause = build_key_where(&columns(), &key).unwrap();
    assert_eq!(where_clause, "`id` = '1'");
}

#[test]
fn matches_a_non_decodable_column_through_to_string() {
    let mut cols = columns();
    cols.insert("tags".to_string(), "Array(String)".to_string());
    let mut key = Map::new();
    key.insert("tags".to_string(), str_val("['a','b']"));
    let where_clause = build_key_where(&cols, &key).unwrap();
    assert_eq!(where_clause, "toString(`tags`) = '[\\'a\\',\\'b\\']'");
}

#[test]
fn matches_a_null_key_value_with_is_null() {
    let mut key = Map::new();
    key.insert("name".to_string(), Value::Null);
    let where_clause = build_key_where(&columns(), &key).unwrap();
    assert_eq!(where_clause, "`name` IS NULL");
}

#[test]
fn joins_several_key_columns_with_and() {
    let mut key = Map::new();
    key.insert("id".to_string(), str_val("1"));
    key.insert("name".to_string(), Value::Null);
    let where_clause = build_key_where(&columns(), &key).unwrap();
    // `Map`/serde_json orders by insertion for the fixture, but the function must not depend on
    // that — both orders are accepted.
    assert!(
        where_clause == "`id` = '1' AND `name` IS NULL"
            || where_clause == "`name` IS NULL AND `id` = '1'"
    );
}

#[test]
fn refuses_a_key_column_the_table_does_not_have() {
    let mut key = Map::new();
    key.insert("nope".to_string(), str_val("1"));
    assert!(build_key_where(&columns(), &key).is_err());
}

use super::combined_key_where;

#[test]
fn combines_several_keys_with_or_between_them() {
    let mut key1 = Map::new();
    key1.insert("id".to_string(), str_val("1"));
    let mut key2 = Map::new();
    key2.insert("id".to_string(), str_val("2"));
    let where_clause = combined_key_where(&columns(), &[key1, key2]).unwrap();
    assert_eq!(where_clause, "(`id` = '1') OR (`id` = '2')");
}

#[test]
fn refuses_an_empty_key_in_a_multi_row_delete() {
    let mut key1 = Map::new();
    key1.insert("id".to_string(), str_val("1"));
    let empty = Map::new();
    assert!(combined_key_where(&columns(), &[key1, empty]).is_err());
}

use super::same_columns;

fn row_with(pairs: &[(&str, &str)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), str_val(v)))
        .collect()
}

#[test]
fn accepts_rows_that_all_fill_in_the_same_columns() {
    let rows = vec![
        row_with(&[("id", "1"), ("name", "a")]),
        row_with(&[("name", "b"), ("id", "2")]), // different order, same set
    ];
    assert!(same_columns(&rows));
}

#[test]
fn rejects_rows_that_fill_in_different_columns() {
    let rows = vec![
        row_with(&[("id", "1"), ("name", "a")]),
        row_with(&[("id", "2")]),
    ];
    assert!(!same_columns(&rows));
}

#[test]
fn a_single_row_or_no_rows_always_passes() {
    assert!(same_columns(&[]));
    assert!(same_columns(&[row_with(&[("id", "1")])]));
}

use super::{find_new_mutation, mutation_fail_reason, mutation_is_done};
use std::collections::HashSet;

fn mutation_row(id: &str, command: &str, is_done: Value, fail_reason: &str) -> Map<String, Value> {
    let mut row = Map::new();
    row.insert("mutation_id".to_string(), str_val(id));
    row.insert("command".to_string(), str_val(command));
    row.insert("is_done".to_string(), is_done);
    row.insert("latest_fail_reason".to_string(), str_val(fail_reason));
    row
}

#[test]
fn mutation_is_done_reads_both_json_shapes_of_uint8() {
    assert!(mutation_is_done(&mutation_row(
        "1",
        "x",
        Value::Number(1.into()),
        ""
    )));
    assert!(mutation_is_done(&mutation_row("1", "x", str_val("1"), "")));
    assert!(!mutation_is_done(&mutation_row(
        "1",
        "x",
        Value::Number(0.into()),
        ""
    )));
    assert!(!mutation_is_done(&mutation_row("1", "x", str_val("0"), "")));
}

#[test]
fn mutation_fail_reason_reads_the_column_or_falls_back_to_empty() {
    assert_eq!(
        mutation_fail_reason(&mutation_row("1", "x", str_val("0"), "boom")),
        "boom"
    );
    assert_eq!(
        mutation_fail_reason(&mutation_row("1", "x", str_val("0"), "")),
        ""
    );
}

#[test]
fn finds_the_one_mutation_not_in_the_baseline() {
    let baseline: HashSet<String> = ["old-1".to_string()].into_iter().collect();
    let rows = vec![
        mutation_row("old-1", "(UPDATE x)", str_val("1"), ""),
        mutation_row("new-1", "(UPDATE x)", str_val("0"), ""),
    ];
    let found = find_new_mutation(&rows, &baseline).unwrap();
    assert_eq!(found.get("mutation_id").unwrap(), "new-1");
}

#[test]
fn finds_nothing_when_every_row_is_in_the_baseline() {
    let baseline: HashSet<String> = ["old-1".to_string()].into_iter().collect();
    let rows = vec![mutation_row("old-1", "(UPDATE x)", str_val("0"), "")];
    assert!(find_new_mutation(&rows, &baseline).is_none());
}

#[test]
fn finds_nothing_when_more_than_one_new_row_appears() {
    let baseline: HashSet<String> = HashSet::new();
    let rows = vec![
        mutation_row("new-1", "(UPDATE x)", str_val("0"), ""),
        mutation_row("new-2", "(UPDATE y)", str_val("0"), ""),
    ];
    assert!(find_new_mutation(&rows, &baseline).is_none());
}
