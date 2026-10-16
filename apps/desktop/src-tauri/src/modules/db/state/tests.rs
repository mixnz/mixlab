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
