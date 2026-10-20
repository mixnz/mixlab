//! The exact SQL each sqlx engine writes for the grid's edits: the count before an update, the
//! update, an insert, the delete of one row and the delete of all of them. Written out in full, so
//! that any change to what reaches a server shows here as a failing line.

use crate::modules::db::drivers::mysql::MySqlRows;
use crate::modules::db::drivers::postgres::PgRows;
use crate::modules::db::drivers::sqlite::SqliteRows;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A map in the order given, which is the order the SQL names the columns in.
fn map(pairs: &[(&str, Option<&str>)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(column, value)| {
            let value = value.map_or(Value::Null, |v| Value::String(v.to_string()));
            (column.to_string(), value)
        })
        .collect()
}

fn key() -> Map<String, Value> {
    map(&[("id", Some("1"))])
}

/// A key of two columns, one of them null: a table with no primary key names a row this way.
fn key2() -> Map<String, Value> {
    map(&[("a", Some("1")), ("b", None)])
}

fn updates() -> Map<String, Value> {
    map(&[("title", Some("x")), ("body", None)])
}

fn row() -> Map<String, Value> {
    map(&[("title", Some("x")), ("body", Some("y"))])
}

fn mysql() -> MySqlRows {
    MySqlRows::new("shop", "post")
}

/// `body` has no type on purpose: a column the catalogue does not know gets a bare placeholder.
fn postgres(keyed: bool) -> PgRows {
    let types: BTreeMap<String, String> = [
        ("id", "integer"),
        ("title", "text"),
        ("a", "integer"),
        ("b", "text"),
    ]
    .into_iter()
    .map(|(c, t)| (c.to_string(), t.to_string()))
    .collect();
    PgRows {
        qualified: "\"app\".\"post\"".to_string(),
        types,
        keyed,
    }
}

fn sqlite(keyed: bool) -> SqliteRows {
    SqliteRows {
        quoted: "\"post\"".to_string(),
        keyed,
    }
}

#[test]
fn mysql_counts_with_the_null_safe_match() {
    assert_eq!(
        mysql().count(&key2()),
        "SELECT COUNT(*) FROM `shop`.`post` WHERE `a` <=> ? AND `b` <=> ?"
    );
}

#[test]
fn mysql_updates_the_set_then_the_key() {
    assert_eq!(
        mysql().update(&updates(), &key()),
        "UPDATE `shop`.`post` SET `title` = ?, `body` = ? WHERE `id` <=> ?"
    );
}

#[test]
fn mysql_inserts_the_columns_a_row_names() {
    assert_eq!(
        mysql().insert(&row()),
        "INSERT INTO `shop`.`post` (`title`, `body`) VALUES (?, ?)"
    );
}

#[test]
fn mysql_inserts_a_row_of_nothing_but_defaults() {
    assert_eq!(
        mysql().insert(&Map::new()),
        "INSERT INTO `shop`.`post` () VALUES ()"
    );
}

#[test]
fn mysql_deletes_one_row_with_a_limit() {
    assert_eq!(
        mysql().delete_one(&key2()),
        "DELETE FROM `shop`.`post` WHERE `a` <=> ? AND `b` <=> ? LIMIT 1"
    );
}

#[test]
fn mysql_deletes_every_row() {
    assert_eq!(mysql().delete_all(), "DELETE FROM `shop`.`post`");
}

#[test]
fn postgres_counts_with_typed_placeholders() {
    assert_eq!(
        postgres(true).count(&key2()),
        "SELECT COUNT(*) FROM \"app\".\"post\" WHERE \"a\" IS NOT DISTINCT FROM $1::integer \
         AND \"b\" IS NOT DISTINCT FROM $2::text"
    );
}

/// The key's numbers carry on from the SET clause's: one numbering through the statement, which
/// has to agree with the order the values are bound in.
#[test]
fn postgres_numbers_the_key_after_the_set() {
    assert_eq!(
        postgres(true).update(&updates(), &key2()),
        "UPDATE \"app\".\"post\" SET \"title\" = $1::text, \"body\" = $2 \
         WHERE \"a\" IS NOT DISTINCT FROM $3::integer AND \"b\" IS NOT DISTINCT FROM $4::text"
    );
}

#[test]
fn postgres_inserts_the_columns_a_row_names() {
    assert_eq!(
        postgres(true).insert(&row()),
        "INSERT INTO \"app\".\"post\" (\"title\", \"body\") VALUES ($1::text, $2)"
    );
}

#[test]
fn postgres_inserts_a_row_of_nothing_but_defaults() {
    assert_eq!(
        postgres(true).insert(&Map::new()),
        "INSERT INTO \"app\".\"post\" DEFAULT VALUES"
    );
}

#[test]
fn postgres_deletes_by_the_primary_key() {
    assert_eq!(
        postgres(true).delete_one(&key()),
        "DELETE FROM \"app\".\"post\" WHERE \"id\" IS NOT DISTINCT FROM $1::integer"
    );
}

#[test]
fn postgres_deletes_one_physical_row_without_a_primary_key() {
    assert_eq!(
        postgres(false).delete_one(&key()),
        "DELETE FROM \"app\".\"post\" WHERE (tableoid, ctid) = (SELECT tableoid, ctid FROM \
         \"app\".\"post\" WHERE \"id\" IS NOT DISTINCT FROM $1::integer LIMIT 1)"
    );
}

#[test]
fn postgres_deletes_every_row() {
    assert_eq!(postgres(true).delete_all(), "DELETE FROM \"app\".\"post\"");
}

#[test]
fn sqlite_counts_with_is() {
    assert_eq!(
        sqlite(true).count(&key2()),
        "SELECT COUNT(*) FROM \"post\" WHERE \"a\" IS ? AND \"b\" IS ?"
    );
}

#[test]
fn sqlite_updates_the_set_then_the_key() {
    assert_eq!(
        sqlite(true).update(&updates(), &key()),
        "UPDATE \"post\" SET \"title\" = ?, \"body\" = ? WHERE \"id\" IS ?"
    );
}

#[test]
fn sqlite_inserts_the_columns_a_row_names() {
    assert_eq!(
        sqlite(true).insert(&row()),
        "INSERT INTO \"post\" (\"title\", \"body\") VALUES (?, ?)"
    );
}

#[test]
fn sqlite_inserts_a_row_of_nothing_but_defaults() {
    assert_eq!(
        sqlite(true).insert(&Map::new()),
        "INSERT INTO \"post\" DEFAULT VALUES"
    );
}

#[test]
fn sqlite_deletes_by_the_primary_key() {
    assert_eq!(
        sqlite(true).delete_one(&key()),
        "DELETE FROM \"post\" WHERE \"id\" IS ?"
    );
}

#[test]
fn sqlite_deletes_one_rowid_without_a_primary_key() {
    assert_eq!(
        sqlite(false).delete_one(&key()),
        "DELETE FROM \"post\" WHERE rowid = (SELECT rowid FROM \"post\" WHERE \"id\" IS ? LIMIT 1)"
    );
}

#[test]
fn sqlite_deletes_every_row() {
    assert_eq!(sqlite(true).delete_all(), "DELETE FROM \"post\"");
}
