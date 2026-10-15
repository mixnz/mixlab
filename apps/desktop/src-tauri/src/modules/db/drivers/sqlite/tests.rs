use super::*;

/// A real database file, built from `sqlite_fixture.sql` and deleted when the test ends.
///
/// The file is real rather than `:memory:` because that is the thing under test: `connect`
/// refuses to create one, checks the path first, and reads its own name back out for the
/// header. None of that has a meaning for an in-memory database.
pub struct Fixture {
    pub path: std::path::PathBuf,
}

impl Fixture {
    pub async fn open() -> (Self, SqlitePool) {
        let path = std::env::temp_dir().join(format!("mixlab-sqlite-{}.db", uuid::Uuid::new_v4()));
        // The one place in the app that creates a database file, and it is a test: `connect`
        // never does — see D5 of the plan this was built from.
        let pool = SqlitePoolOptions::new()
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .expect("create the fixture database");
        for statement in include_str!("fixture.sql").split(";\n") {
            if statement.trim().is_empty() {
                continue;
            }
            sqlx::raw_sql(statement)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("fixture statement failed: {e}\n{statement}"));
        }
        pool.close().await;

        let pool = connect(path.to_str().unwrap())
            .await
            .expect("open the fixture through `connect`");
        (Self { path }, pool)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        // WAL leaves two more behind if anything ever turns it on. Removing them costs nothing
        // and keeps a failed run from leaving three files in the temp directory instead of one.
        let _ = std::fs::remove_file(self.path.with_extension("db-wal"));
        let _ = std::fs::remove_file(self.path.with_extension("db-shm"));
    }
}

fn page(page_size: i64) -> PageQuery {
    PageQuery {
        page: 0,
        page_size,
        ..PageQuery::default()
    }
}

fn filtered(column: &str, operator: &str, value: Option<&str>) -> PageQuery {
    PageQuery {
        page: 0,
        page_size: 50,
        filters: vec![Filter {
            column: column.to_string(),
            operator: operator.to_string(),
            value: value.map(str::to_string),
        }],
        ..PageQuery::default()
    }
}

fn map(pairs: &[(&str, Option<&str>)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_string(),
                v.map(|s| Value::String(s.to_string()))
                    .unwrap_or(Value::Null),
            )
        })
        .collect()
}

/// One column of one row, read straight back out of the file rather than through `table_data` —
/// so a test that asserts a write is not also asserting the read path.
async fn cell(pool: &SqlitePool, sql: &str) -> Value {
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(pool)
        .await
        .unwrap();
    column_value(&row, 0)
}

#[tokio::test]
async fn an_edited_value_lands_in_the_column_type_rather_than_as_text() {
    let (_fixture, pool) = Fixture::open().await;
    update_row(
        &pool,
        "post",
        &map(&[("views", Some("42"))]),
        &map(&[("id", Some("1"))]),
    )
    .await
    .unwrap();

    /* Bound as the text the grid sent, and stored as an integer: SQLite applies the column's
    affinity on the way in, which is why nothing here needs the casts PostgreSQL's binds
    carry. A stored `"42"` would come back as a JSON string. */
    assert_eq!(
        cell(&pool, "select views from post where id = 1").await,
        Value::from(42)
    );
}

#[tokio::test]
async fn an_explicit_null_is_written_as_one() {
    let (_fixture, pool) = Fixture::open().await;
    update_row(
        &pool,
        "post",
        &map(&[("views", None)]),
        &map(&[("id", Some("1"))]),
    )
    .await
    .unwrap();
    assert_eq!(
        cell(&pool, "select views from post where id = 1").await,
        Value::Null
    );
}

#[tokio::test]
async fn a_null_key_column_matches_the_null_it_names() {
    let (_fixture, pool) = Fixture::open().await;
    // `=` on a null is null and matches nothing, so a row keyed on one would be unreachable.
    // The predicate uses `IS`, which is the null-safe comparison.
    update_row(
        &pool,
        "post",
        &map(&[("title", Some("Renamed"))]),
        &map(&[("id", Some("3")), ("views", None)]),
    )
    .await
    .unwrap();
    assert_eq!(
        cell(&pool, "select title from post where id = 3").await,
        Value::String("Renamed".into())
    );
}

#[tokio::test]
async fn a_key_matching_two_rows_changes_neither() {
    let (_fixture, pool) = Fixture::open().await;
    sqlx::raw_sql("insert into loose (label) values ('twin'), ('twin')")
        .execute(&pool)
        .await
        .unwrap();

    let error = update_row(
        &pool,
        "loose",
        &map(&[("label", Some("changed"))]),
        &map(&[("label", Some("twin"))]),
    )
    .await
    .expect_err("should refuse");
    assert_eq!(error.code, "error.rowsMatched");

    // The count and the write share a transaction, so the refusal leaves both rows as they
    // were rather than changing one of them on the way to finding out.
    let still: i64 = sqlx::query_scalar("select count(*) from loose where label = 'twin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still, 2);
}

#[tokio::test]
async fn an_update_without_a_key_is_refused() {
    let (_fixture, pool) = Fixture::open().await;
    let error = update_row(&pool, "post", &map(&[("title", Some("x"))]), &Map::new())
        .await
        .expect_err("should refuse");
    assert_eq!(error.code, "error.updateWithoutKey");
}

#[tokio::test]
async fn a_column_left_out_of_an_insert_gets_the_tables_default() {
    let (_fixture, pool) = Fixture::open().await;
    insert_rows(
        &pool,
        "post",
        &[map(&[("author_id", Some("2")), ("title", Some("Fresh"))])],
    )
    .await
    .unwrap();

    assert_eq!(
        cell(&pool, "select views from post where title = 'Fresh'").await,
        Value::from(0)
    );
    // The generated column was never named and is computed anyway.
    assert_eq!(
        cell(&pool, "select slug from post where title = 'Fresh'").await,
        Value::String("fresh".into())
    );
}

#[tokio::test]
async fn a_row_that_names_nothing_is_all_defaults() {
    let (_fixture, pool) = Fixture::open().await;
    insert_rows(&pool, "loose", &[Map::new()]).await.unwrap();
    let rows: i64 = sqlx::query_scalar("select count(*) from loose")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn one_rejected_row_means_none_of_them_land() {
    let (_fixture, pool) = Fixture::open().await;
    let error = insert_rows(
        &pool,
        "post",
        &[
            map(&[("author_id", Some("1")), ("title", Some("First of two"))]),
            // No author 9: the foreign key is enforced, which is what `connect` leaves on.
            map(&[("author_id", Some("9")), ("title", Some("Second of two"))]),
        ],
    )
    .await
    .expect_err("should refuse");
    assert_eq!(error.code, "error.rowFailed");

    let landed: i64 = sqlx::query_scalar("select count(*) from post where title like '% of two'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(landed, 0);
}

#[tokio::test]
async fn a_delete_on_a_table_with_no_key_takes_one_row_not_its_twin() {
    let (_fixture, pool) = Fixture::open().await;
    sqlx::raw_sql("insert into loose (label) values ('twin'), ('twin')")
        .execute(&pool)
        .await
        .unwrap();

    delete_rows(
        &pool,
        "loose",
        &[map(&[("label", Some("twin"))])],
        false,
        false,
    )
    .await
    .unwrap();

    /* Aimed at the rowid of one matching row rather than at the predicate, which matches both.
    Deleting "the rows that look like this one" would have taken a row the user did not
    select. */
    let left: i64 = sqlx::query_scalar("select count(*) from loose where label = 'twin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 1);
}

#[tokio::test]
async fn deleting_everything_empties_the_table() {
    let (_fixture, pool) = Fixture::open().await;
    delete_rows(&pool, "post", &[], true, false).await.unwrap();
    let left: i64 = sqlx::query_scalar("select count(*) from post")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn resetting_the_counter_numbers_the_next_row_from_one() {
    let (_fixture, pool) = Fixture::open().await;
    delete_rows(&pool, "post", &[], true, true).await.unwrap();
    insert_rows(
        &pool,
        "post",
        &[map(&[("author_id", Some("1")), ("title", Some("Again"))])],
    )
    .await
    .unwrap();
    // `post` is AUTOINCREMENT, so without clearing sqlite_sequence this would be 4.
    assert_eq!(cell(&pool, "select id from post").await, Value::from(1));
}

#[tokio::test]
async fn a_table_with_no_counter_is_not_an_error_to_reset() {
    let (_fixture, pool) = Fixture::open().await;
    // `tag` has no AUTOINCREMENT, so there is no row in sqlite_sequence naming it — and the
    // reset offered beside the delete has to be a no-op rather than a failure.
    delete_rows(&pool, "tag", &[], true, true).await.unwrap();
}

#[tokio::test]
async fn a_missing_file_is_an_error_and_stays_missing() {
    let path = std::env::temp_dir().join(format!("mixlab-absent-{}.db", uuid::Uuid::new_v4()));
    let error = connect(path.to_str().unwrap())
        .await
        .expect_err("should refuse");
    assert_eq!(error.code, "error.sqliteFileNotFound");
    // The point of the check, not a side effect of it: opening a path that is not there must
    // not leave an empty database behind for the user to wonder about.
    assert!(!path.exists());
}

#[tokio::test]
async fn a_created_file_is_an_empty_database_that_opens() {
    let path = std::env::temp_dir().join(format!("mixlab-new-{}.db", uuid::Uuid::new_v4()));
    create_file(path.to_str().unwrap()).await.unwrap();

    // Not zero bytes: one write is made so the file carries SQLite's header and another tool
    // looking at it sees a database rather than an empty file.
    assert!(std::fs::metadata(&path).unwrap().len() > 0);

    let pool = connect(path.to_str().unwrap()).await.expect("opens");
    assert!(list_tables(&pool).await.unwrap().is_empty());
    pool.close().await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn creating_over_an_existing_file_is_refused_and_leaves_it_alone() {
    let (fixture, pool) = Fixture::open().await;
    pool.close().await;

    /* The save dialog will have asked about replacing it and been told yes. That must not
    reach here as "delete that database": nothing else in MixLab deletes a database file. */
    let error = create_file(fixture.path.to_str().unwrap())
        .await
        .expect_err("should refuse");
    assert_eq!(error.code, "error.sqliteFileExists");

    let pool = connect(fixture.path.to_str().unwrap())
        .await
        .expect("still there");
    assert!(!list_tables(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn an_empty_path_is_an_error() {
    assert_eq!(
        connect("   ").await.expect_err("should refuse").code,
        "error.sqlitePathRequired"
    );
}

#[tokio::test]
async fn the_header_names_the_file_rather_than_a_machine() {
    let (fixture, pool) = Fixture::open().await;
    let info = server_info(&pool).await.unwrap();
    assert!(
        info.version.starts_with('3'),
        "version was {}",
        info.version
    );
    assert_eq!(info.os, fixture.path.file_name().unwrap().to_string_lossy());
}

#[tokio::test]
async fn there_is_one_database_and_it_is_called_main() {
    assert_eq!(list_databases(), vec!["main".to_string()]);
}

#[tokio::test]
async fn tables_and_views_are_listed_and_sqlites_own_are_not() {
    let (_fixture, pool) = Fixture::open().await;
    let tables = list_tables(&pool).await.unwrap();
    // `recent` is a view and is in; `sqlite_sequence` exists because `post` is AUTOINCREMENT,
    // and is out.
    assert_eq!(tables, vec!["author", "loose", "post", "recent", "tag"]);
}

#[tokio::test]
async fn a_generated_column_is_read_and_marked() {
    let (_fixture, pool) = Fixture::open().await;
    let data = table_data(&pool, "post", &page(50)).await.unwrap();

    // In the grid, in table order — `pragma_table_info` would have left it out entirely.
    assert_eq!(
        data.columns,
        vec![
            "id",
            "author_id",
            "title",
            "slug",
            "body",
            "views",
            "created_at"
        ]
    );
    assert_eq!(data.column_meta["slug"].extra, "generated");
    assert_eq!(data.rows[0]["slug"], Value::String("hello world".into()));
}

#[tokio::test]
async fn only_an_integer_primary_key_counts_as_generated_by_the_engine() {
    let (_fixture, pool) = Fixture::open().await;

    let post = table_data(&pool, "post", &page(50)).await.unwrap();
    assert_eq!(post.primary_key, vec!["id"]);
    assert_eq!(post.auto_increment_column.as_deref(), Some("id"));
    assert_eq!(post.column_meta["id"].extra, "rowid");

    /* `tag.id` is declared INTEGER and is first in the key, and is still an ordinary column: a
    rowid alias is a *single*-column key. An INSERT has to give it a value, so reporting it
    as server-assigned would be a row the grid refuses to write. */
    let tag = table_data(&pool, "tag", &page(50)).await.unwrap();
    assert_eq!(tag.primary_key, vec!["id", "label"]);
    assert_eq!(tag.auto_increment_column, None);
    assert_eq!(tag.column_meta["id"].extra, "");
}

#[tokio::test]
async fn a_column_says_what_it_holds() {
    let (_fixture, pool) = Fixture::open().await;
    let data = table_data(&pool, "post", &page(50)).await.unwrap();

    assert_eq!(data.column_meta["title"].data_type, "TEXT");
    assert!(!data.column_meta["title"].nullable);
    assert!(data.column_meta["body"].nullable);
    assert_eq!(
        data.column_meta["views"].default_value.as_deref(),
        Some("0")
    );
    assert_eq!(
        data.column_meta["created_at"].default_value.as_deref(),
        Some("CURRENT_TIMESTAMP")
    );

    let key = data.column_meta["author_id"].foreign_key.as_ref().unwrap();
    assert_eq!(key.table, "author");
    assert_eq!(key.column, "id");
}

#[tokio::test]
async fn a_blob_comes_back_base64_and_a_null_comes_back_null() {
    let (_fixture, pool) = Fixture::open().await;
    let data = table_data(&pool, "post", &page(50)).await.unwrap();
    // x'00ff10' — bytes have no JSON of their own.
    assert_eq!(data.rows[0]["body"], Value::String("AP8Q".into()));
    assert_eq!(data.rows[1]["body"], Value::Null);
    assert_eq!(data.rows[2]["views"], Value::Null);
    // Read off the storage class, not the declared type.
    assert_eq!(data.rows[0]["views"], Value::from(7));
}

#[tokio::test]
async fn a_page_is_cut_out_of_the_sorted_whole() {
    let (_fixture, pool) = Fixture::open().await;
    let query = PageQuery {
        page: 1,
        page_size: 2,
        sort_column: Some("id".to_string()),
        sort_desc: true,
        filters: Vec::new(),
    };
    let data = table_data(&pool, "post", &query).await.unwrap();
    // `total` counts the table, not the page.
    assert_eq!(data.total, 3);
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0]["id"], Value::from(1));
}

#[tokio::test]
async fn a_sort_column_that_is_not_a_column_is_ignored() {
    let (_fixture, pool) = Fixture::open().await;
    let query = PageQuery {
        page: 0,
        page_size: 50,
        // This is what keeps the value out of the SQL text: it never names a real column, so
        // it never reaches the ORDER BY at all.
        sort_column: Some("id; DROP TABLE post".to_string()),
        sort_desc: false,
        filters: Vec::new(),
    };
    assert_eq!(table_data(&pool, "post", &query).await.unwrap().total, 3);
}

#[tokio::test]
async fn filters_compare_as_text_so_a_typed_value_matches_what_is_shown() {
    let (_fixture, pool) = Fixture::open().await;

    let contains = table_data(&pool, "post", &filtered("title", "contains", Some("post")))
        .await
        .unwrap();
    assert_eq!(contains.total, 1);

    // An integer column, matched against text typed into the box.
    let eq = table_data(&pool, "post", &filtered("views", "eq", Some("7")))
        .await
        .unwrap();
    assert_eq!(eq.total, 1);

    let null = table_data(&pool, "post", &filtered("body", "isNull", None))
        .await
        .unwrap();
    assert_eq!(null.total, 2);
}

#[tokio::test]
async fn a_filter_on_a_column_that_is_not_there_is_refused() {
    let (_fixture, pool) = Fixture::open().await;
    // The column name is the one part of a filter that is interpolated, so it is checked
    // against the table rather than trusted.
    let error = table_data(
        &pool,
        "post",
        &filtered("title\" IS NULL --", "eq", Some("x")),
    )
    .await
    .expect_err("should refuse");
    assert_eq!(error.code, "error.unknownFilterColumn");
}

#[tokio::test]
async fn regexp_is_refused_rather_than_sent() {
    let (_fixture, pool) = Fixture::open().await;
    /* The dropdown does not offer it, so this only happens to a filter that arrived some other
    way — and then it says which operator is unknown, rather than SQLite saying "no such
    function: regexp" about a query nobody wrote. */
    let error = table_data(&pool, "post", &filtered("title", "regexp", Some("^H")))
        .await
        .expect_err("should refuse");
    assert_eq!(error.code, "error.unknownFilterOperator");
}
