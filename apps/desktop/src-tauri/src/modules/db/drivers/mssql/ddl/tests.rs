use super::*;

#[test]
fn a_quote_inside_a_string_literal_is_doubled() {
    assert_eq!(quote_string("it's"), "'it''s'");
    assert_eq!(quote_string("a\\b"), "'a\\b'");
}

#[test]
fn an_nstring_carries_the_unicode_prefix() {
    assert_eq!(quote_nstring("mới"), "N'mới'");
    assert_eq!(quote_nstring("it's"), "N'it''s'");
}

fn spec(name: &str, data_type: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        data_type: data_type.to_string(),
        nullable: true,
        default_value: None,
        default_is_expression: false,
        auto_increment: false,
        collation: None,
        comment: String::new(),
    }
}

#[test]
fn a_plain_column_is_named_typed_and_nullable() {
    assert_eq!(
        column_definition(&spec("title", "nvarchar(255)")).unwrap(),
        "[title] nvarchar(255) NULL"
    );
}

#[test]
fn an_identity_column_takes_neither_null_nor_default() {
    let mut spec = spec("id", "int");
    spec.auto_increment = true;
    spec.default_value = Some("7".to_string());
    assert_eq!(
        column_definition(&spec).unwrap(),
        "[id] int IDENTITY(1,1) NOT NULL"
    );
}

#[test]
fn a_collation_is_written_bare_not_bracket_quoted() {
    // `COLLATE [Latin1_General_CI_AS]` is a syntax error on real SQL Server — confirmed against
    // the test server. A collation name is validated against an allow-list instead of quoted.
    let mut spec = spec("name", "nvarchar(50)");
    spec.collation = Some("Latin1_General_CI_AS".to_string());
    assert_eq!(
        column_definition(&spec).unwrap(),
        "[name] nvarchar(50) COLLATE Latin1_General_CI_AS NULL"
    );
}

#[test]
fn a_collation_with_a_character_outside_the_allow_list_is_refused() {
    let mut spec = spec("name", "nvarchar(50)");
    spec.collation = Some("Latin1_General_CI_AS; DROP TABLE t".to_string());
    assert!(column_definition(&spec).is_err());
}

#[test]
fn a_literal_default_is_quoted_and_an_expression_is_not() {
    assert_eq!(default_clause("new", false), "'new'");
    assert_eq!(default_clause("it's", false), "'it''s'");
    assert_eq!(default_clause("getdate()", true), "getdate()");
    assert_eq!(default_clause("NULL", false), "NULL");
}

#[test]
fn a_new_column_with_a_comment_only_ever_adds_one() {
    assert_eq!(
        comment_statement("dbo", "t", "c", "hello", false),
        Some(
            "EXEC sp_addextendedproperty @name = N'MS_Description', @value = N'hello', \
                 @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'TABLE', \
                 @level1name = N't', @level2type = N'COLUMN', @level2name = N'c'"
                .to_string()
        )
    );
}

#[test]
fn clearing_an_existing_comment_drops_the_property_rather_than_setting_it_empty() {
    assert_eq!(
        comment_statement("dbo", "t", "c", "", true),
        Some(
            "EXEC sp_dropextendedproperty @name = N'MS_Description', @level0type = N'SCHEMA', \
                 @level0name = N'dbo', @level1type = N'TABLE', @level1name = N't', \
                 @level2type = N'COLUMN', @level2name = N'c'"
                .to_string()
        )
    );
}

#[test]
fn no_comment_and_none_before_writes_nothing() {
    assert_eq!(comment_statement("dbo", "t", "c", "", false), None);
}
