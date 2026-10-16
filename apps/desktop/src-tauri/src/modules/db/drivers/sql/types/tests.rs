//! The JSON each shape is written as, pinned through every engine's own path to it.
//!
//! The window reads one TypeScript type for each of these whatever the engine, so every engine has
//! to write the same JSON. Each test builds the same value through each engine's path and compares
//! it with one literal.

use crate::modules::db::drivers::clickhouse::structure::SkipIndex;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

fn page_json() -> Value {
    json!({
        "columns": ["id"],
        "columnMeta": {
            "id": {
                "dataType": "int",
                "nullable": false,
                "defaultValue": "0",
                "extra": "auto_increment",
                "foreignKey": {"table": "users", "column": "id"}
            }
        },
        "primaryKey": ["id"],
        "autoIncrementColumn": "id",
        "rows": [{"id": 1}],
        "total": 1
    })
}

fn row() -> Map<String, Value> {
    let mut row = Map::new();
    row.insert("id".into(), json!(1));
    row
}

macro_rules! page {
    ($engine:ident, $foreign_key:expr) => {{
        use crate::modules::db::drivers::$engine as e;
        e::TablePage {
            columns: vec!["id".into()],
            column_meta: BTreeMap::from([(
                "id".to_string(),
                e::ColumnMeta {
                    data_type: "int".into(),
                    nullable: false,
                    default_value: Some("0".into()),
                    extra: "auto_increment".into(),
                    foreign_key: $foreign_key,
                },
            )]),
            primary_key: vec!["id".into()],
            auto_increment_column: Some("id".into()),
            rows: vec![row()],
            total: 1,
        }
    }};
}

macro_rules! fk {
    ($engine:ident) => {
        Some(crate::modules::db::drivers::$engine::ForeignKey {
            table: "users".into(),
            column: "id".into(),
        })
    };
}

#[test]
fn a_table_page_is_one_shape_on_every_engine() {
    let expected = page_json();
    assert_eq!(
        serde_json::to_value(page!(mysql, fk!(mysql))).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::to_value(page!(postgres, fk!(postgres))).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::to_value(page!(mssql, fk!(mssql))).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::to_value(page!(sqlite, fk!(sqlite))).unwrap(),
        expected
    );
    // ClickHouse has no foreign keys to report, so its page carries none.
    let mut without = expected.clone();
    without["columnMeta"]["id"]["foreignKey"] = Value::Null;
    assert_eq!(
        serde_json::to_value(page!(clickhouse, None)).unwrap(),
        without
    );
}

fn structure_json() -> Value {
    json!({
        "columns": [{
            "name": "id",
            "dataType": "int",
            "nullable": false,
            "defaultValue": null,
            "defaultIsExpression": false,
            "autoIncrement": true,
            "onUpdateCurrentTimestamp": false,
            "generated": false,
            "collation": "utf8mb4_bin",
            "comment": "key",
            "key": "PRI",
            "extra": "auto_increment"
        }],
        "indexes": [{
            "name": "PRIMARY",
            "unique": true,
            "primary": true,
            "indexType": "BTREE",
            "columns": [{"name": "id", "prefixLength": 4}],
            "comment": ""
        }],
        "skipIndexes": [{
            "name": "idx",
            "expr": "id",
            "indexType": "minmax",
            "args": [],
            "granularity": 1
        }],
        "engine": "InnoDB"
    })
}

macro_rules! structure {
    ($engine:ident) => {{
        use crate::modules::db::drivers::$engine::structure as s;
        s::TableStructure {
            columns: vec![s::StructureColumn {
                name: "id".into(),
                data_type: "int".into(),
                nullable: false,
                default_value: None,
                default_is_expression: false,
                auto_increment: true,
                on_update_current_timestamp: false,
                generated: false,
                collation: Some("utf8mb4_bin".into()),
                comment: "key".into(),
                key: "PRI".into(),
                extra: "auto_increment".into(),
            }],
            indexes: vec![s::TableIndex {
                name: "PRIMARY".into(),
                unique: true,
                primary: true,
                index_type: "BTREE".into(),
                columns: vec![s::IndexColumn {
                    name: Some("id".into()),
                    prefix_length: Some(4),
                }],
                comment: String::new(),
            }],
            skip_indexes: vec![SkipIndex {
                name: "idx".into(),
                expr: "id".into(),
                index_type: "minmax".into(),
                args: Vec::new(),
                granularity: 1,
            }],
            engine: Some("InnoDB".into()),
        }
    }};
}

#[test]
fn a_table_structure_is_one_shape_on_every_engine() {
    let expected = structure_json();
    assert_eq!(serde_json::to_value(structure!(mysql)).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(structure!(postgres)).unwrap(),
        expected
    );
    assert_eq!(serde_json::to_value(structure!(mssql)).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(structure!(clickhouse)).unwrap(),
        expected
    );
    assert_eq!(serde_json::to_value(structure!(sqlite)).unwrap(), expected);
}

macro_rules! outline {
    ($engine:ident) => {{
        use crate::modules::db::drivers::$engine::structure as s;
        s::SchemaOutline {
            database: "shop".into(),
            tables: vec![s::OutlineTable {
                name: "orders".into(),
                columns: vec![s::OutlineColumn {
                    name: "user_id".into(),
                    data_type: "int".into(),
                    nullable: true,
                    key: "MUL".into(),
                    references: Some("users.id".into()),
                }],
            }],
        }
    }};
}

#[test]
fn a_schema_outline_is_one_shape_on_every_engine() {
    let expected = json!({
        "database": "shop",
        "tables": [{
            "name": "orders",
            "columns": [{
                "name": "user_id",
                "dataType": "int",
                "nullable": true,
                "key": "MUL",
                "references": "users.id"
            }]
        }]
    });
    assert_eq!(serde_json::to_value(outline!(mysql)).unwrap(), expected);
    assert_eq!(serde_json::to_value(outline!(postgres)).unwrap(), expected);
    assert_eq!(serde_json::to_value(outline!(mssql)).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(outline!(clickhouse)).unwrap(),
        expected
    );
    assert_eq!(serde_json::to_value(outline!(sqlite)).unwrap(), expected);
}

macro_rules! stats {
    ($engine:ident) => {
        crate::modules::db::drivers::$engine::structure::TableStats {
            name: "orders".into(),
            rows: 2,
            data_size: 3,
            index_size: 4,
            avg_record_size: 5,
        }
    };
}

#[test]
fn table_stats_are_one_shape_on_every_engine() {
    let expected = json!({
        "name": "orders", "rows": 2, "dataSize": 3, "indexSize": 4, "avgRecordSize": 5
    });
    assert_eq!(serde_json::to_value(stats!(mysql)).unwrap(), expected);
    assert_eq!(serde_json::to_value(stats!(postgres)).unwrap(), expected);
    assert_eq!(serde_json::to_value(stats!(mssql)).unwrap(), expected);
    assert_eq!(serde_json::to_value(stats!(clickhouse)).unwrap(), expected);
    assert_eq!(serde_json::to_value(stats!(sqlite)).unwrap(), expected);
}

macro_rules! collation {
    ($engine:ident) => {
        crate::modules::db::drivers::$engine::structure::Collation {
            name: "utf8mb4_bin".into(),
            charset: "utf8mb4".into(),
            is_default: true,
        }
    };
}

#[test]
fn a_collation_is_one_shape_on_every_engine_that_has_them() {
    let expected = json!({"name": "utf8mb4_bin", "charset": "utf8mb4", "isDefault": true});
    assert_eq!(serde_json::to_value(collation!(mysql)).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(collation!(postgres)).unwrap(),
        expected
    );
    assert_eq!(serde_json::to_value(collation!(mssql)).unwrap(), expected);
    assert_eq!(serde_json::to_value(collation!(sqlite)).unwrap(), expected);
}

macro_rules! read_query {
    ($engine:ident, $json:expr) => {{
        let query: crate::modules::db::drivers::$engine::PageQuery =
            serde_json::from_value($json).unwrap();
        (
            query.page,
            query.page_size,
            query.sort_column,
            query.sort_desc,
            query
                .filters
                .into_iter()
                .map(|f| (f.column, f.operator, f.value))
                .collect::<Vec<_>>(),
        )
    }};
}

#[test]
fn a_page_query_is_read_the_same_way_on_every_engine() {
    let full = json!({
        "page": 2, "pageSize": 50, "sortColumn": "id", "sortDesc": true,
        "filters": [{"column": "id", "operator": "gt", "value": "3"}, {"column": "x", "operator": "isNull"}]
    });
    let bare = json!({"page": 1, "pageSize": 10});
    let expected_full = (
        2,
        50,
        Some("id".to_string()),
        true,
        vec![
            ("id".to_string(), "gt".to_string(), Some("3".to_string())),
            ("x".to_string(), "isNull".to_string(), None),
        ],
    );
    let expected_bare = (1, 10, None, false, Vec::new());
    assert_eq!(read_query!(mysql, full.clone()), expected_full);
    assert_eq!(read_query!(postgres, full.clone()), expected_full);
    assert_eq!(read_query!(mssql, full.clone()), expected_full);
    assert_eq!(read_query!(clickhouse, full.clone()), expected_full);
    assert_eq!(read_query!(sqlite, full), expected_full);
    assert_eq!(read_query!(mysql, bare.clone()), expected_bare);
    assert_eq!(read_query!(postgres, bare.clone()), expected_bare);
    assert_eq!(read_query!(mssql, bare.clone()), expected_bare);
    assert_eq!(read_query!(clickhouse, bare.clone()), expected_bare);
    assert_eq!(read_query!(sqlite, bare), expected_bare);
}

/// The column the window sends, every field set, as `ColumnDialog`'s `toSpec` writes it.
fn column_spec_json() -> Value {
    json!({
        "name": "total",
        "dataType": "decimal(10,2)",
        "nullable": false,
        "defaultValue": "0",
        "defaultIsExpression": false,
        "autoIncrement": false,
        "onUpdateCurrentTimestamp": false,
        "collation": "utf8mb4_bin",
        "comment": "sum",
        "after": "id"
    })
}

fn index_spec_json() -> Value {
    json!({
        "name": "by_total",
        "kind": "index",
        "indexType": "BTREE",
        "columns": [{"name": "total", "prefixLength": 4}],
        "comment": "fast"
    })
}

/// What an engine made of the shared spec is what it made of the same JSON read directly, which
/// is how it read it before the shared type existed.
macro_rules! converts_like_it_reads {
    ($shared:ty, $own:ty, $json:expr) => {{
        let shared: $shared = serde_json::from_value($json).unwrap();
        let converted: $own = shared.into();
        let read: $own = serde_json::from_value($json).unwrap();
        assert_eq!(format!("{converted:?}"), format!("{read:?}"));
    }};
}

#[test]
fn every_engine_reads_the_shared_column_spec_as_it_read_its_own() {
    use crate::modules::db::drivers::sql::types::ColumnSpec;
    use crate::modules::db::drivers::{clickhouse, mssql, mysql, postgres, sqlite};
    converts_like_it_reads!(ColumnSpec, mysql::structure::ColumnSpec, column_spec_json());
    converts_like_it_reads!(ColumnSpec, postgres::ddl::ColumnSpec, column_spec_json());
    converts_like_it_reads!(ColumnSpec, mssql::ddl::ColumnSpec, column_spec_json());
    converts_like_it_reads!(ColumnSpec, clickhouse::ddl::ColumnSpec, column_spec_json());
    converts_like_it_reads!(ColumnSpec, sqlite::ddl::ColumnSpec, column_spec_json());
}

#[test]
fn every_engine_reads_the_shared_index_spec_as_it_read_its_own() {
    use crate::modules::db::drivers::sql::types::IndexSpec;
    use crate::modules::db::drivers::{mssql, mysql, postgres, sqlite};
    converts_like_it_reads!(IndexSpec, mysql::structure::IndexSpec, index_spec_json());
    converts_like_it_reads!(IndexSpec, postgres::ddl::IndexSpec, index_spec_json());
    converts_like_it_reads!(IndexSpec, mssql::ddl::IndexSpec, index_spec_json());
    converts_like_it_reads!(IndexSpec, sqlite::ddl::IndexSpec, index_spec_json());
}
