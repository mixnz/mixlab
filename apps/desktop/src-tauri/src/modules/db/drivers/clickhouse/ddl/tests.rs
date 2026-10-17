use super::*;

fn skip_spec(
    name: &str,
    expr: &str,
    index_type: &str,
    args: &[&str],
    granularity: u64,
) -> SkipIndexSpec {
    SkipIndexSpec {
        name: name.to_string(),
        expr: expr.to_string(),
        index_type: index_type.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        granularity,
    }
}

#[test]
fn a_type_with_no_arguments_is_written_bare() {
    assert_eq!(
        add_skip_index_statement(
            "shop",
            "orders",
            &skip_spec("ix1", "total", "minmax", &[], 1)
        )
        .unwrap(),
        "ALTER TABLE `shop`.`orders` ADD INDEX `ix1` total TYPE minmax GRANULARITY 1"
    );
}

#[test]
fn a_type_with_arguments_writes_them_in_order() {
    assert_eq!(
        add_skip_index_statement(
            "shop",
            "orders",
            &skip_spec("ix1", "note", "ngrambf_v1", &["3", "256", "2", "0"], 4)
        )
        .unwrap(),
        "ALTER TABLE `shop`.`orders` ADD INDEX `ix1` note TYPE ngrambf_v1(3, 256, 2, 0) GRANULARITY 4"
    );
}

#[test]
fn an_index_needs_a_name() {
    assert_eq!(
        add_skip_index_statement(
            "shop",
            "orders",
            &skip_spec("  ", "total", "minmax", &[], 1)
        )
        .unwrap_err(),
        err!("error.indexNameRequired")
    );
}

#[test]
fn an_index_needs_an_expression() {
    assert_eq!(
        add_skip_index_statement("shop", "orders", &skip_spec("ix1", "  ", "minmax", &[], 1))
            .unwrap_err(),
        err!("error.clickhouseSkipIndexExprRequired")
    );
}

#[test]
fn an_empty_key_writes_tuple() {
    assert_eq!(order_by_clause(&[]), "ORDER BY tuple()");
}

#[test]
fn columns_are_quoted_and_kept_in_order() {
    assert_eq!(
        order_by_clause(&["b".to_string(), "a".to_string()]),
        "ORDER BY (`b`, `a`)"
    );
}

#[test]
fn the_temp_name_carries_the_original_and_a_timestamp() {
    let name = temp_table_name("orders");
    let suffix = name.strip_prefix("orders__mixlab_rebuild_").expect(&name);
    assert!(suffix.parse::<u128>().is_ok(), "{suffix}");
}

/// The exact text verified against the test server for a table with every optional clause at
/// once — `PARTITION BY`, `TTL`, `SETTINGS`, `COMMENT`, and a skip index sitting inside the
/// column list.
const SHOW_CREATE_WITH_EVERY_CLAUSE: &str = "CREATE TABLE shop.orders\n(\n    `a` UInt64,\n    `b` UInt64,\n    `c` String,\n    INDEX ix1 c TYPE minmax GRANULARITY 1\n)\nENGINE = MergeTree\nPARTITION BY toYYYYMM(ts)\nORDER BY (a, b)\nTTL ts + toIntervalDay(30)\nSETTINGS index_granularity = 4096\nCOMMENT 'test comment'";

#[test]
fn rebuild_renames_the_table_and_replaces_only_the_order_by_line() {
    let rebuilt = rebuild_ddl(
        SHOW_CREATE_WITH_EVERY_CLAUSE,
        "shop",
        "orders__mixlab_rebuild_1",
        "ORDER BY (c)",
    )
    .unwrap();
    assert_eq!(
        rebuilt,
        "CREATE TABLE `shop`.`orders__mixlab_rebuild_1`\n(\n    `a` UInt64,\n    `b` UInt64,\n    `c` String,\n    INDEX ix1 c TYPE minmax GRANULARITY 1\n)\nENGINE = MergeTree\nPARTITION BY toYYYYMM(ts)\nORDER BY (c)\nTTL ts + toIntervalDay(30)\nSETTINGS index_granularity = 4096\nCOMMENT 'test comment'"
    );
}

#[test]
fn rebuild_handles_an_empty_sorting_key() {
    let show_create = "CREATE TABLE shop.orders\n(\n    `id` UInt64\n)\nENGINE = MergeTree\nORDER BY tuple()\nSETTINGS index_granularity = 8192";
    let rebuilt = rebuild_ddl(
        show_create,
        "shop",
        "orders__mixlab_rebuild_2",
        "ORDER BY (id)",
    )
    .unwrap();
    assert_eq!(
        rebuilt,
        "CREATE TABLE `shop`.`orders__mixlab_rebuild_2`\n(\n    `id` UInt64\n)\nENGINE = MergeTree\nORDER BY (id)\nSETTINGS index_granularity = 8192"
    );
}

#[test]
fn rebuild_refuses_text_with_no_order_by_line_to_replace() {
    assert_eq!(
        rebuild_ddl(
            "CREATE TABLE shop.orders\n(\n    `id` UInt64\n)\nENGINE = Memory",
            "shop",
            "t",
            "ORDER BY (id)"
        )
        .unwrap_err(),
        err!("error.clickhouseRebuildParse")
    );
}

#[test]
fn modifying_sends_the_drop_before_the_add() {
    assert_eq!(
        modify_skip_index_statements(
            "shop",
            "orders",
            "ix1",
            &skip_spec("ix1", "total", "set", &["100"], 2)
        )
        .unwrap(),
        vec![
            "ALTER TABLE `shop`.`orders` DROP INDEX `ix1`",
            "ALTER TABLE `shop`.`orders` ADD INDEX `ix1` total TYPE set(100) GRANULARITY 2",
        ]
    );
}

#[test]
fn a_quoted_string_is_a_literal_and_loses_its_quotes() {
    assert_eq!(
        read_default("'active'"),
        Some(("active".to_string(), false))
    );
}

#[test]
fn a_function_call_is_an_expression_kept_verbatim() {
    assert_eq!(read_default("now()"), Some(("now()".to_string(), true)));
}

#[test]
fn a_number_is_an_expression_there_being_nothing_to_tell_it_from_one() {
    assert_eq!(read_default("42"), Some(("42".to_string(), true)));
}

#[test]
fn nothing_at_all_is_no_default() {
    assert_eq!(read_default(""), None);
    assert_eq!(read_default("   "), None);
}

#[test]
fn a_concatenation_that_merely_starts_and_ends_with_a_quote_is_not_a_literal() {
    assert_eq!(
        read_default("'a' || 'b'"),
        Some(("'a' || 'b'".to_string(), true))
    );
}

#[test]
fn an_escaped_quote_inside_a_literal_is_unescaped() {
    assert_eq!(read_default("'it\\'s'"), Some(("it's".to_string(), false)));
}

#[test]
fn a_new_table_gets_one_placeholder_column_and_no_sorting_key() {
    assert_eq!(
        create_table_statement("shop", "orders", "MergeTree").unwrap(),
        "CREATE TABLE `shop`.`orders` (`id` UInt64) ENGINE = MergeTree ORDER BY tuple()"
    );
}

#[test]
fn an_engine_outside_the_list_is_refused_rather_than_interpolated() {
    assert_eq!(
        create_table_statement("shop", "orders", "Kafka").unwrap_err(),
        err!("error.clickhouseUnknownEngine", engine = "Kafka")
    );
    assert_eq!(
        create_table_statement("shop", "orders", "MergeTree ORDER BY x; DROP TABLE y")
            .unwrap_err()
            .code,
        "error.clickhouseUnknownEngine"
    );
}

#[test]
fn a_table_with_no_name_is_refused_before_any_sql_is_built() {
    assert_eq!(
        create_table_statement("shop", "  ", "MergeTree").unwrap_err(),
        err!("error.tableNameRequired")
    );
}

fn spec(name: &str, data_type: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        data_type: data_type.to_string(),
        default_value: None,
        default_is_expression: false,
        comment: String::new(),
    }
}

#[test]
fn an_escaped_quote_survives_the_default_round_trip() {
    let (value, is_expression) = read_default("'it\'s'").unwrap();
    assert_eq!(default_clause(&value, is_expression), " DEFAULT 'it\'s'");
}

#[test]
fn an_expression_default_goes_back_out_unquoted() {
    assert_eq!(default_clause("now()", true), " DEFAULT now()");
}

#[test]
fn a_plain_column_is_just_a_name_and_a_type() {
    assert_eq!(
        column_definition(&spec("title", "String")).unwrap(),
        "`title` String"
    );
}

#[test]
fn nullability_travels_inside_the_type_rather_than_as_a_clause_of_its_own() {
    assert_eq!(
        column_definition(&spec("title", "Nullable(String)")).unwrap(),
        "`title` Nullable(String)"
    );
}

#[test]
fn a_literal_default_is_quoted_and_an_expression_is_not() {
    let mut literal = spec("state", "String");
    literal.default_value = Some("active".to_string());
    assert_eq!(
        column_definition(&literal).unwrap(),
        "`state` String DEFAULT 'active'"
    );

    let mut expression = spec("made", "DateTime");
    expression.default_value = Some("now()".to_string());
    expression.default_is_expression = true;
    assert_eq!(
        column_definition(&expression).unwrap(),
        "`made` DateTime DEFAULT now()"
    );
}

#[test]
fn a_comment_is_written_only_when_there_is_one() {
    let mut commented = spec("title", "String");
    commented.comment = "the heading".to_string();
    assert_eq!(
        column_definition(&commented).unwrap(),
        "`title` String COMMENT 'the heading'"
    );
}

#[test]
fn a_column_needs_both_a_name_and_a_type() {
    assert_eq!(
        column_definition(&spec("  ", "String")).unwrap_err(),
        err!("error.columnNameRequired")
    );
    assert_eq!(
        column_definition(&spec("title", " ")).unwrap_err(),
        err!("error.columnTypeRequired")
    );
}

fn current(name: &str, data_type: &str) -> StructureColumn {
    StructureColumn {
        name: name.to_string(),
        data_type: data_type.to_string(),
        nullable: data_type.starts_with("Nullable("),
        default_value: None,
        default_is_expression: false,
        auto_increment: false,
        on_update_current_timestamp: false,
        generated: false,
        collation: None,
        comment: String::new(),
        key: String::new(),
        extra: String::new(),
    }
}

#[test]
fn a_column_saved_without_being_touched_sends_nothing_at_all() {
    let statements = modify_column_statements(
        "`s`.`t`",
        &current("title", "String"),
        &spec("title", "String"),
    )
    .unwrap();
    assert!(statements.is_empty(), "{statements:?}");
}

#[test]
fn renaming_only_sends_only_a_rename() {
    assert_eq!(
        modify_column_statements(
            "`s`.`t`",
            &current("title", "String"),
            &spec("heading", "String")
        )
        .unwrap(),
        vec!["ALTER TABLE `s`.`t` RENAME COLUMN `title` TO `heading`"]
    );
}

#[test]
fn changing_the_comment_only_leaves_the_type_alone() {
    let mut wanted = spec("title", "String");
    wanted.comment = "the heading".to_string();
    assert_eq!(
        modify_column_statements("`s`.`t`", &current("title", "String"), &wanted).unwrap(),
        vec!["ALTER TABLE `s`.`t` MODIFY COLUMN `title` String COMMENT 'the heading'"]
    );
}

#[test]
fn a_rename_and_a_redefinition_are_two_statements_with_the_rename_first() {
    assert_eq!(
        modify_column_statements(
            "`s`.`t`",
            &current("title", "String"),
            &spec("heading", "Nullable(String)")
        )
        .unwrap(),
        vec![
            "ALTER TABLE `s`.`t` RENAME COLUMN `title` TO `heading`",
            "ALTER TABLE `s`.`t` MODIFY COLUMN `heading` Nullable(String)",
        ]
    );
}

#[test]
fn dropping_a_default_is_a_change_like_any_other() {
    let mut had_one = current("state", "String");
    had_one.default_value = Some("active".to_string());
    assert_eq!(
        modify_column_statements("`s`.`t`", &had_one, &spec("state", "String")).unwrap(),
        vec!["ALTER TABLE `s`.`t` MODIFY COLUMN `state` String"]
    );
}
