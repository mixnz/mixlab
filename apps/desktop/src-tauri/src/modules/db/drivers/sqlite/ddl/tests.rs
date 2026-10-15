use super::super::structure::table_structure;
use super::super::tests::Fixture;
use super::*;

fn column(name: &str, data_type: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        data_type: data_type.to_string(),
        nullable: true,
        default_value: None,
        default_is_expression: false,
        collation: None,
    }
}

fn index(name: &str, kind: &str, columns: &[&str]) -> IndexSpec {
    IndexSpec {
        name: name.to_string(),
        kind: kind.to_string(),
        columns: columns
            .iter()
            .map(|c| IndexColumnSpec {
                name: (*c).to_string(),
            })
            .collect(),
    }
}

#[tokio::test]
async fn a_new_table_has_the_one_column_the_rest_are_added_to() {
    let (_fixture, pool) = Fixture::open().await;
    create_table(&pool, "note").await.unwrap();
    let structure = table_structure(&pool, "note").await.unwrap();
    assert_eq!(structure.columns.len(), 1);
    assert_eq!(structure.columns[0].name, "id");
    // The rowid under a name, which is what makes it fill itself in.
    assert!(structure.columns[0].auto_increment);
}

#[tokio::test]
async fn a_column_is_added_with_its_default_and_its_collation() {
    let (_fixture, pool) = Fixture::open().await;
    let mut spec = column("nickname", "TEXT");
    spec.default_value = Some("nobody".to_string());
    spec.collation = Some("NOCASE".to_string());
    add_column(&pool, "author", &spec).await.unwrap();

    let structure = table_structure(&pool, "author").await.unwrap();
    let added = structure
        .columns
        .iter()
        .find(|c| c.name == "nickname")
        .unwrap();
    // Quoted going in and unquoted coming back — see `split_default`.
    assert_eq!(added.default_value.as_deref(), Some("nobody"));
    assert!(!added.default_is_expression);
}

#[tokio::test]
async fn an_expression_default_cannot_be_added_and_sqlite_says_so() {
    let (_fixture, pool) = Fixture::open().await;
    let mut spec = column("seen_at", "TEXT");
    spec.default_value = Some("CURRENT_TIMESTAMP".to_string());
    spec.default_is_expression = true;

    /* A rule of `ADD COLUMN` rather than of the column: SQLite will not add one whose default
    is not a constant, though a `CREATE TABLE` may declare exactly that. The definition
    written here is right, and the engine's own refusal is what reaches the user — it states
    the rule better than a second copy of it here would. */
    let error = add_column(&pool, "author", &spec)
        .await
        .expect_err("should refuse");
    assert_eq!(error.code, "error.sqlite");
    assert!(
        error.params["message"].contains("non-constant default"),
        "unexpected message: {}",
        error.params["message"]
    );
}

#[tokio::test]
async fn renaming_a_column_is_the_one_edit_that_goes_through() {
    let (_fixture, pool) = Fixture::open().await;
    let mut spec = column("biography", "TEXT");
    spec.default_value = Some("anonymous".to_string());
    modify_column(&pool, "author", "bio", &spec).await.unwrap();

    let structure = table_structure(&pool, "author").await.unwrap();
    assert!(structure.columns.iter().any(|c| c.name == "biography"));
    assert!(!structure.columns.iter().any(|c| c.name == "bio"));
}

#[tokio::test]
async fn a_type_that_differs_only_in_case_is_the_same_type() {
    let (_fixture, pool) = Fixture::open().await;
    // SQLite stores the declaration verbatim, so `text` and `TEXT` come back different and
    // mean the same — comparing them as written would refuse a plain rename.
    let mut spec = column("biography", "text");
    spec.default_value = Some("anonymous".to_string());
    modify_column(&pool, "author", "bio", &spec).await.unwrap();
}

#[tokio::test]
async fn an_index_left_unnamed_is_named_after_what_it_covers() {
    let (_fixture, pool) = Fixture::open().await;
    // MySQL names one after its first column; SQLite requires a name and offers none.
    add_index(&pool, "post", &index("", "index", &["views"]))
        .await
        .unwrap();
    let structure = table_structure(&pool, "post").await.unwrap();
    assert!(structure.indexes.iter().any(|i| i.name == "post_views"));
}

#[tokio::test]
async fn a_unique_index_is_created_unique() {
    let (_fixture, pool) = Fixture::open().await;
    add_index(&pool, "author", &index("author_name", "unique", &["name"]))
        .await
        .unwrap();
    let structure = table_structure(&pool, "author").await.unwrap();
    let added = structure
        .indexes
        .iter()
        .find(|i| i.name == "author_name")
        .unwrap();
    assert!(added.unique && !added.primary);
}

#[tokio::test]
async fn a_primary_key_cannot_be_added_afterwards() {
    let (_fixture, pool) = Fixture::open().await;
    // Refused here rather than sent: there is no statement that would do it.
    assert_eq!(
        add_index(&pool, "loose", &index("", "primary", &["label"]))
            .await
            .expect_err("should refuse")
            .code,
        "error.sqliteNoPrimaryKeyAfterwards"
    );
}

#[tokio::test]
async fn replacing_an_index_leaves_the_table_with_the_new_one() {
    let (_fixture, pool) = Fixture::open().await;
    modify_index(
        &pool,
        "post",
        "post_author",
        &index("post_author", "index", &["views"]),
    )
    .await
    .unwrap();
    let structure = table_structure(&pool, "post").await.unwrap();
    let replaced = structure
        .indexes
        .iter()
        .find(|i| i.name == "post_author")
        .unwrap();
    assert_eq!(replaced.columns[0].name.as_deref(), Some("views"));
}

#[tokio::test]
async fn an_index_a_constraint_owns_cannot_be_dropped_on_its_own() {
    let (_fixture, pool) = Fixture::open().await;

    // `tag`'s key is two columns, so SQLite built an index for it — one that belongs to the
    // constraint, and that `DROP INDEX` refuses.
    let structure = table_structure(&pool, "tag").await.unwrap();
    let owned = structure.indexes[0].name.clone();
    assert_eq!(
        drop_index(&pool, "tag", &owned)
            .await
            .expect_err("owned")
            .code,
        "error.sqliteIndexBelongsToConstraint"
    );

    // And the primary key of a rowid table, which is not an index at all — it is the row the
    // Structure tab synthesises.
    assert_eq!(
        drop_index(&pool, "post", "PRIMARY")
            .await
            .expect_err("implicit")
            .code,
        "error.sqliteIndexBelongsToConstraint"
    );
}

#[tokio::test]
async fn a_created_index_can_be_dropped() {
    let (_fixture, pool) = Fixture::open().await;
    drop_index(&pool, "post", "post_author").await.unwrap();
    let structure = table_structure(&pool, "post").await.unwrap();
    assert!(!structure.indexes.iter().any(|i| i.name == "post_author"));
}

#[tokio::test]
async fn a_table_is_renamed_and_dropped() {
    let (_fixture, pool) = Fixture::open().await;
    rename_table(&pool, "tag", "label").await.unwrap();
    assert!(table_structure(&pool, "label").await.unwrap().columns.len() == 2);
    drop_table(&pool, "label").await.unwrap();
    assert!(table_structure(&pool, "label")
        .await
        .unwrap()
        .columns
        .is_empty());
}

#[tokio::test]
async fn a_column_is_dropped() {
    let (_fixture, pool) = Fixture::open().await;
    drop_column(&pool, "author", "bio").await.unwrap();
    let structure = table_structure(&pool, "author").await.unwrap();
    assert!(!structure.columns.iter().any(|c| c.name == "bio"));
}

#[test]
fn splits_simple_columns() {
    let (clauses, suffix) = split_column_clauses("CREATE TABLE t (a INTEGER, b TEXT)").unwrap();
    assert_eq!(clauses, vec!["a INTEGER", "b TEXT"]);
    assert_eq!(suffix, "");
}

#[test]
fn does_not_split_a_comma_inside_a_type_arg() {
    let (clauses, _) =
        split_column_clauses("CREATE TABLE t (price DECIMAL(10,2), b TEXT)").unwrap();
    assert_eq!(clauses, vec!["price DECIMAL(10,2)", "b TEXT"]);
}

#[test]
fn does_not_split_a_comma_inside_a_default_expression() {
    let (clauses, _) =
        split_column_clauses("CREATE TABLE t (a INTEGER DEFAULT (max(0, 1)))").unwrap();
    assert_eq!(clauses, vec!["a INTEGER DEFAULT (max(0, 1))"]);
}

#[test]
fn does_not_split_a_comma_inside_a_line_comment() {
    let (clauses, _) =
        split_column_clauses("CREATE TABLE t (a INTEGER, -- note, with a comma\n  b TEXT)")
            .unwrap();
    assert_eq!(clauses.len(), 2);
    assert!(clauses[0].starts_with("a INTEGER"));
    assert_eq!(clauses[1], "b TEXT");
}

#[test]
fn does_not_split_a_comma_inside_a_block_comment() {
    let (clauses, _) =
        split_column_clauses("CREATE TABLE t (a INTEGER /* x, y */, b TEXT)").unwrap();
    assert_eq!(clauses.len(), 2);
}

#[test]
fn reads_a_quoted_column_name_with_a_space_in_it() {
    let (clauses, _) = split_column_clauses("CREATE TABLE t (\"full name\" TEXT, b TEXT)").unwrap();
    assert_eq!(clauses[0], "\"full name\" TEXT");
}

#[test]
fn captures_the_table_constraint_clause() {
    let (clauses, _) =
        split_column_clauses("CREATE TABLE t (id INTEGER, label TEXT, PRIMARY KEY (id, label))")
            .unwrap();
    assert_eq!(clauses[2], "PRIMARY KEY (id, label)");
}

#[test]
fn captures_the_table_options_suffix() {
    let (_, suffix) =
        split_column_clauses("CREATE TABLE t (a INTEGER PRIMARY KEY) WITHOUT ROWID").unwrap();
    assert_eq!(suffix, "WITHOUT ROWID");
}

#[test]
fn no_suffix_is_an_empty_string() {
    let (_, suffix) = split_column_clauses("CREATE TABLE t (a INTEGER)").unwrap();
    assert_eq!(suffix, "");
}

#[test]
fn an_unbalanced_statement_is_a_parse_failure() {
    assert!(split_column_clauses("CREATE TABLE t (a INTEGER").is_err());
}

#[test]
fn recognises_every_table_constraint_keyword() {
    assert!(is_table_constraint("PRIMARY KEY (id)"));
    assert!(is_table_constraint("UNIQUE (a, b)"));
    assert!(is_table_constraint("CHECK(x > 0)"));
    assert!(is_table_constraint("FOREIGN KEY (a) REFERENCES t (id)"));
    assert!(is_table_constraint(
        "CONSTRAINT fk FOREIGN KEY (a) REFERENCES t (id)"
    ));
}

#[test]
fn a_column_whose_name_starts_with_a_keyword_is_not_a_table_constraint() {
    assert!(!is_table_constraint("primary_email TEXT"));
    assert!(!is_table_constraint("checked_out INTEGER"));
}

#[test]
fn reads_the_bare_and_quoted_column_name() {
    assert_eq!(
        clause_column_name("id INTEGER PRIMARY KEY"),
        Some("id".to_string())
    );
    assert_eq!(
        clause_column_name("\"full name\" TEXT"),
        Some("full name".to_string())
    );
    assert_eq!(
        clause_column_name("`weird` TEXT"),
        Some("weird".to_string())
    );
    assert_eq!(
        clause_column_name("[bracketed] TEXT"),
        Some("bracketed".to_string())
    );
}

#[test]
fn a_table_constraint_clause_has_no_column_name_of_its_own() {
    assert_eq!(clause_column_name("PRIMARY KEY (id, label)"), None);
}

#[test]
fn finds_a_column_named_in_a_constraint_clause() {
    assert!(clause_mentions_column("PRIMARY KEY (id, label)", "label"));
    assert!(clause_mentions_column(
        "FOREIGN KEY (\"author_id\") REFERENCES author (id)",
        "author_id"
    ));
}

#[test]
fn does_not_match_a_column_name_that_is_only_a_substring() {
    assert!(!clause_mentions_column("PRIMARY KEY (id)", "i"));
    assert!(!clause_mentions_column("UNIQUE (author_id)", "author"));
}

#[test]
fn reads_a_trailing_collate() {
    assert_eq!(
        clause_collation("bio TEXT COLLATE NOCASE"),
        Some("NOCASE".to_string())
    );
    assert_eq!(
        clause_collation("bio TEXT DEFAULT 'x' COLLATE NOCASE"),
        Some("NOCASE".to_string())
    );
}

#[test]
fn no_collate_is_none() {
    assert_eq!(clause_collation("bio TEXT"), None);
}

#[tokio::test]
async fn current_column_reads_a_real_collation() {
    let (_fixture, pool) = Fixture::open().await;
    sqlx::raw_sql("ALTER TABLE author ADD COLUMN nickname TEXT COLLATE NOCASE")
        .execute(&pool)
        .await
        .unwrap();
    let current = current_column(&pool, "author", "nickname").await.unwrap();
    assert_eq!(current.collation.as_deref(), Some("NOCASE"));
}

#[tokio::test]
async fn current_column_reports_no_collation_when_there_is_none() {
    let (_fixture, pool) = Fixture::open().await;
    let current = current_column(&pool, "author", "bio").await.unwrap();
    assert_eq!(current.collation, None);
}

#[test]
fn rebuild_blockers_refuses_a_column_named_in_a_table_constraint() {
    let (clauses, _) =
        split_column_clauses("CREATE TABLE t (id INTEGER, label TEXT, PRIMARY KEY (id, label))")
            .unwrap();
    let error = rebuild_blockers(&clauses, "label").expect_err("should refuse");
    assert_eq!(error.code, "error.sqliteColumnInTableConstraint");
}

#[test]
fn rebuild_blockers_allows_a_column_outside_any_table_constraint() {
    let (clauses, _) =
        split_column_clauses("CREATE TABLE t (id INTEGER, label TEXT, note TEXT)").unwrap();
    rebuild_blockers(&clauses, "note").unwrap();
}

#[tokio::test]
async fn generated_or_rowid_alias_flags_a_generated_column() {
    let (_fixture, pool) = Fixture::open().await;
    let (generated, rowid) = generated_or_rowid_alias(&pool, "post", "slug")
        .await
        .unwrap();
    assert!(generated);
    assert!(!rowid);
}

#[tokio::test]
async fn generated_or_rowid_alias_flags_the_rowid_alias() {
    let (_fixture, pool) = Fixture::open().await;
    let (generated, rowid) = generated_or_rowid_alias(&pool, "author", "id")
        .await
        .unwrap();
    assert!(!generated);
    assert!(rowid);
}

#[tokio::test]
async fn a_two_column_primary_key_is_not_a_rowid_alias() {
    let (_fixture, pool) = Fixture::open().await;
    // `tag.id` is declared INTEGER and leads the key, but the key is two columns — the exact
    // shape that is *not* a rowid alias (same fixture note `sqlite/structure.rs` already
    // relies on).
    let (generated, rowid) = generated_or_rowid_alias(&pool, "tag", "id").await.unwrap();
    assert!(!generated);
    assert!(!rowid);
}

#[tokio::test]
async fn generated_or_rowid_alias_flags_neither_for_a_plain_column() {
    let (_fixture, pool) = Fixture::open().await;
    let (generated, rowid) = generated_or_rowid_alias(&pool, "author", "name")
        .await
        .unwrap();
    assert!(!generated);
    assert!(!rowid);
}

#[tokio::test]
async fn rebuild_changes_a_columns_type_and_keeps_its_data() {
    let (_fixture, pool) = Fixture::open().await;
    // `views` holds numbers stored as INTEGER already — retype to TEXT is round-trippable
    // without loss, and lets the test assert on the affinity actually applied.
    let spec = column("views", "TEXT");
    modify_column(&pool, "post", "views", &spec).await.unwrap();
    let structure = table_structure(&pool, "post").await.unwrap();
    let views = structure
        .columns
        .iter()
        .find(|c| c.name == "views")
        .unwrap();
    assert_eq!(views.data_type, "TEXT");

    let value: String = sqlx::query_scalar("select views from post where title = 'Hello world'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(value, "7");
}

#[tokio::test]
async fn rebuild_adds_not_null_when_no_row_violates_it() {
    let (_fixture, pool) = Fixture::open().await;
    // `post.created_at` is nullable in the fixture but every row's value came from its own
    // `DEFAULT CURRENT_TIMESTAMP` — no row is actually NULL, so NOT NULL is safe to add.
    // `author.name` was not picked here: it is already `NOT NULL` in the fixture, so setting
    // `nullable = false` on it would not change anything and would not exercise rebuild at all.
    let mut spec = column("created_at", "TEXT");
    spec.nullable = false;
    spec.default_value = Some("CURRENT_TIMESTAMP".to_string());
    spec.default_is_expression = true;
    modify_column(&pool, "post", "created_at", &spec)
        .await
        .unwrap();
    let structure = table_structure(&pool, "post").await.unwrap();
    assert!(
        !structure
            .columns
            .iter()
            .find(|c| c.name == "created_at")
            .unwrap()
            .nullable
    );
}

#[tokio::test]
async fn rebuild_refuses_not_null_when_a_row_would_violate_it_and_leaves_the_table_untouched() {
    let (_fixture, pool) = Fixture::open().await;
    // `author.bio` has a NULL row (Grace) in the fixture.
    let mut spec = column("bio", "TEXT");
    spec.nullable = false;
    spec.default_value = Some("anonymous".to_string());
    let error = modify_column(&pool, "author", "bio", &spec)
        .await
        .expect_err("should refuse");
    assert_eq!(error.code, "error.sqlite");
    assert!(
        error.params["message"]
            .to_ascii_uppercase()
            .contains("NOT NULL"),
        "unexpected message: {}",
        error.params["message"]
    );

    // Rolled back cleanly: the table is exactly as it was.
    let structure = table_structure(&pool, "author").await.unwrap();
    assert!(
        structure
            .columns
            .iter()
            .find(|c| c.name == "bio")
            .unwrap()
            .nullable
    );
    let count: i64 = sqlx::query_scalar("select count(*) from author")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn rebuild_changes_a_default() {
    let (_fixture, pool) = Fixture::open().await;
    let mut spec = column("bio", "TEXT");
    spec.default_value = Some("unknown".to_string());
    modify_column(&pool, "author", "bio", &spec).await.unwrap();
    let structure = table_structure(&pool, "author").await.unwrap();
    assert_eq!(
        structure
            .columns
            .iter()
            .find(|c| c.name == "bio")
            .unwrap()
            .default_value
            .as_deref(),
        Some("unknown")
    );
}

#[tokio::test]
async fn rebuild_adds_a_collation() {
    let (_fixture, pool) = Fixture::open().await;
    let mut spec = column("bio", "TEXT");
    spec.default_value = Some("anonymous".to_string());
    spec.collation = Some("NOCASE".to_string());
    modify_column(&pool, "author", "bio", &spec).await.unwrap();

    let current = current_column(&pool, "author", "bio").await.unwrap();
    assert_eq!(current.collation.as_deref(), Some("NOCASE"));
}

#[tokio::test]
async fn rebuild_keeps_the_tables_indexes() {
    let (_fixture, pool) = Fixture::open().await;
    let spec = column("views", "TEXT");
    modify_column(&pool, "post", "views", &spec).await.unwrap();
    let structure = table_structure(&pool, "post").await.unwrap();
    let names: Vec<&str> = structure.indexes.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"post_author"));
    assert!(names.contains(&"post_title"));
}

#[tokio::test]
async fn rebuild_keeps_without_rowid() {
    let (_fixture, pool) = Fixture::open().await;
    sqlx::raw_sql("CREATE TABLE settings (name TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID")
        .execute(&pool)
        .await
        .unwrap();
    let mut spec = column("value", "TEXT");
    spec.nullable = false;
    spec.default_value = Some("".to_string());
    modify_column(&pool, "settings", "value", &spec)
        .await
        .unwrap();

    let create_sql: String =
        sqlx::query_scalar("select sql from sqlite_master where name = 'settings'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(create_sql.to_ascii_uppercase().contains("WITHOUT ROWID"));
}
