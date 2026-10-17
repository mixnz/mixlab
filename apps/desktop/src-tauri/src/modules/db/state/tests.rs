use super::DbHandle;
use crate::modules::db::drivers::clickhouse::ddl::SkipIndexSpec;
use crate::modules::db::drivers::sqlite::tests::Fixture;

/// A SQLite file, reached as a SQL engine the way every `sql_*` command reaches its connection.
#[tokio::test]
async fn a_sqlite_connection_answers_as_a_sql_engine() {
    let (_fixture, pool) = Fixture::open().await;
    let engine = DbHandle::Sqlite(pool)
        .sql()
        .expect("SQLite is a SQL engine");

    let tables = engine.list_tables("main".into()).await.unwrap();
    assert!(tables.iter().any(|t| t == "author"), "{tables:?}");
    assert_eq!(engine.list_databases().await.unwrap(), ["main"]);
}

/// What an engine does not have is a typed error, never a panic and never a silent success.
#[tokio::test]
async fn an_operation_the_engine_lacks_is_refused_by_name() {
    let (_fixture, pool) = Fixture::open().await;
    let engine = DbHandle::Sqlite(pool).sql().unwrap();

    let spec: SkipIndexSpec = serde_json::from_value(serde_json::json!({
        "name": "idx", "expr": "id", "indexType": "minmax", "args": [], "granularity": 1
    }))
    .unwrap();
    let error = engine
        .add_skip_index("main".into(), "author".into(), spec)
        .await
        .unwrap_err();
    assert_eq!(error.code, "error.unsupportedOperation");
    assert_eq!(
        error.params.get("operation").map(String::as_str),
        Some("add_skip_index")
    );

    let error = engine.create_database("x".into(), None).await.unwrap_err();
    assert_eq!(error.code, "error.unsupportedOperation");
}

fn json<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap()
}

/// Every operation the SQL workspace offers on a SQLite file, through the engine the `sql_*`
/// commands reach, in the order a person would use them: make a table, shape it, fill it, read it,
/// change it, and take it apart again.
#[tokio::test]
async fn a_sqlite_file_goes_through_every_operation_the_workspace_offers() {
    let (_fixture, pool) = Fixture::open().await;
    let engine = DbHandle::Sqlite(pool).sql().unwrap();
    let db = || "main".to_string();
    let t = || "orders".to_string();

    engine.create_table(db(), t(), None, None).await.unwrap();
    let column = serde_json::json!({
        "name": "total", "dataType": "INTEGER", "nullable": true, "defaultValue": null,
        "defaultIsExpression": false, "autoIncrement": false, "onUpdateCurrentTimestamp": false,
        "collation": null, "comment": ""
    });
    engine.add_column(db(), t(), json(column)).await.unwrap();
    let index = serde_json::json!({
        "name": "by_total", "kind": "index", "columns": [{"name": "total"}], "comment": ""
    });
    engine.add_index(db(), t(), json(index)).await.unwrap();

    let rows: Vec<_> = (1..=3)
        .map(|n| json(serde_json::json!({"total": (n * 10).to_string()})))
        .collect();
    engine.insert_rows(db(), t(), rows).await.unwrap();
    let query = serde_json::json!({
        "page": 0, "pageSize": 10, "sortColumn": "total", "sortDesc": true,
        "filters": [{"column": "total", "operator": "gte", "value": "20"}]
    });
    let page = engine.table_data(db(), t(), json(query)).await.unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.rows[0]["total"], serde_json::json!(30));

    engine
        .update_row(
            db(),
            t(),
            json(serde_json::json!({"total": "31"})),
            json(serde_json::json!({"id": "3"})),
        )
        .await
        .unwrap();
    engine
        .delete_rows(
            db(),
            t(),
            vec![json(serde_json::json!({"id": "1"}))],
            false,
            false,
        )
        .await
        .unwrap();
    let results = engine
        .run_script("SELECT total FROM orders ORDER BY id".into(), None, &|_| {})
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    let problem = engine.validate_sql("SELEKT 1".into(), None).await.unwrap();
    assert!(
        problem.is_some(),
        "a statement SQLite cannot parse is reported"
    );

    let structure = engine.table_structure(db(), t()).await.unwrap();
    assert!(structure.columns.iter().any(|c| c.name == "total"));
    assert!(structure.indexes.iter().any(|i| i.name == "by_total"));
    let outline = engine.schema_outline(db()).await.unwrap();
    assert!(outline.tables.iter().any(|table| table.name == "orders"));
    let stats = engine.table_stats(db()).await.unwrap();
    assert!(stats.iter().any(|table| table.name == "orders"));

    engine
        .drop_index(db(), t(), "by_total".into())
        .await
        .unwrap();
    engine.drop_column(db(), t(), "total".into()).await.unwrap();
    engine
        .rename_table(db(), t(), "sales".into())
        .await
        .unwrap();
    engine.drop_table(db(), "sales".into()).await.unwrap();
    let tables = engine.list_tables(db()).await.unwrap();
    assert!(!tables
        .iter()
        .any(|name| name == "orders" || name == "sales"));
}
