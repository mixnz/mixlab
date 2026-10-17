use super::*;

fn column(name: &str) -> IndexColumnSpec {
    IndexColumnSpec {
        name: name.to_string(),
    }
}

#[test]
fn a_unique_index_names_its_method_and_columns() {
    let spec = IndexSpec {
        name: "orders_customer".to_string(),
        kind: "unique".to_string(),
        index_type: Some("nonclustered".to_string()),
        columns: vec![column("customer"), column("placed")],
    };
    assert_eq!(
        create_index_statements("mixlab_agent_test", "sales", "orders", &spec).unwrap(),
        vec![
            "CREATE UNIQUE NONCLUSTERED INDEX [orders_customer] ON [mixlab_agent_test].[sales].[orders] ([customer], [placed])"
        ]
    );
}

#[test]
fn an_unnamed_index_gets_one_generated_from_its_table_and_columns() {
    let spec = IndexSpec {
        name: String::new(),
        kind: "index".to_string(),
        index_type: None,
        columns: vec![column("email")],
    };
    assert_eq!(
        create_index_statements("db", "dbo", "users", &spec).unwrap(),
        vec!["CREATE INDEX [IX_users_email] ON [db].[dbo].[users] ([email])"]
    );
}

#[test]
fn a_primary_key_may_go_unnamed_unlike_a_plain_index() {
    let spec = IndexSpec {
        name: String::new(),
        kind: "primary".to_string(),
        index_type: Some("CLUSTERED".to_string()),
        columns: vec![column("id")],
    };
    assert_eq!(
        create_index_statements("db", "dbo", "t", &spec).unwrap(),
        vec!["ALTER TABLE [db].[dbo].[t] ADD PRIMARY KEY CLUSTERED ([id])"]
    );
}

#[test]
fn an_index_kind_mssql_has_not_is_refused() {
    let spec = IndexSpec {
        name: String::new(),
        kind: "fulltext".to_string(),
        index_type: None,
        columns: vec![column("body")],
    };
    assert!(create_index_statements("db", "dbo", "t", &spec).is_err());
}

#[test]
fn only_clustered_or_nonclustered_reach_the_sql() {
    assert_eq!(
        validated_index_type(Some("clustered")).unwrap(),
        Some("CLUSTERED")
    );
    assert_eq!(validated_index_type(Some("  ")).unwrap(), None);
    assert!(validated_index_type(Some("btree")).is_err());
}

#[test]
fn a_generated_name_is_never_longer_than_the_identifier_limit() {
    let many_columns: Vec<IndexColumnSpec> =
        (0..40).map(|i| column(&format!("column_{i}"))).collect();
    assert!(generated_index_name("a_very_long_table_name_indeed", &many_columns).len() <= 128);
}
