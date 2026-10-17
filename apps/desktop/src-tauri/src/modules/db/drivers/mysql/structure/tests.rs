use super::*;
use crate::modules::db::drivers::sql::types::IndexColumnSpec;

/// A column spec with nothing set, for a test to change one thing about.
fn column(name: &str, data_type: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        data_type: data_type.to_string(),
        nullable: true,
        default_value: None,
        default_is_expression: false,
        auto_increment: false,
        on_update_current_timestamp: false,
        collation: None,
        comment: String::new(),
        after: None,
    }
}

fn index(kind: &str, columns: &[&str]) -> IndexSpec {
    IndexSpec {
        name: String::new(),
        kind: kind.to_string(),
        index_type: None,
        columns: columns
            .iter()
            .map(|name| IndexColumnSpec {
                name: name.to_string(),
                prefix_length: None,
            })
            .collect(),
        comment: String::new(),
    }
}

/// The clause a column definition adds up to, in the order MySQL's grammar wants it.
#[test]
fn a_column_definition_is_assembled_in_the_order_mysql_reads_it() {
    assert_eq!(
        column_definition(&column("id", "int")).unwrap(),
        "`id` int NULL"
    );

    let mut spec = column("id", "int");
    spec.nullable = false;
    spec.auto_increment = true;
    spec.comment = "the key".to_string();
    assert_eq!(
        column_definition(&spec).unwrap(),
        "`id` int NOT NULL AUTO_INCREMENT COMMENT 'the key'",
    );

    // Every part at once, so that a reordering shows up here rather than at the server.
    let mut spec = column("name", "varchar(50)");
    spec.nullable = false;
    spec.collation = Some("utf8mb4_bin".to_string());
    spec.default_value = Some("anon".to_string());
    spec.comment = "who".to_string();
    assert_eq!(
        column_definition(&spec).unwrap(),
        "`name` varchar(50) COLLATE utf8mb4_bin NOT NULL DEFAULT 'anon' COMMENT 'who'",
    );

    // A timestamp column is the one place both CURRENT_TIMESTAMP clauses appear.
    let mut spec = column("seen", "timestamp");
    spec.default_value = Some("CURRENT_TIMESTAMP".to_string());
    spec.on_update_current_timestamp = true;
    assert_eq!(
        column_definition(&spec).unwrap(),
        "`seen` timestamp NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP",
    );
}

/// The two things a definition refuses, because the server's own message for either is worse.
#[test]
fn a_column_needs_a_name_and_a_type() {
    assert!(column_definition(&column("   ", "int")).is_err());
    assert!(column_definition(&column("id", "  ")).is_err());
}

/// Which defaults are quoted and which are not. Getting this wrong is silent: the column ends
/// up holding the *text* `CURRENT_TIMESTAMP` rather than the time.
#[test]
fn a_default_is_quoted_unless_it_can_only_be_an_expression() {
    // On a temporal column the function is recognised without being declared one.
    assert_eq!(
        default_clause("CURRENT_TIMESTAMP", false, "timestamp"),
        "CURRENT_TIMESTAMP"
    );
    assert_eq!(default_clause("now()", false, "datetime"), "now()");
    // On anything else the same text is a string, which is what it would have to mean.
    assert_eq!(
        default_clause("CURRENT_TIMESTAMP", false, "varchar(64)"),
        "'CURRENT_TIMESTAMP'"
    );
    // Unless it is declared an expression.
    assert_eq!(
        default_clause("CURRENT_TIMESTAMP", true, "varchar(64)"),
        "CURRENT_TIMESTAMP"
    );

    // Any other expression is parenthesised, as MySQL 8 requires — and one that already is
    // comes through untouched rather than doubly wrapped.
    assert_eq!(default_clause("uuid()", true, "char(36)"), "(uuid())");
    assert_eq!(default_clause("(uuid())", true, "char(36)"), "(uuid())");

    // `NULL` typed on its own is SQL NULL; quoted, it is four characters.
    assert_eq!(default_clause("NULL", false, "int"), "NULL");
    assert_eq!(default_clause("null", false, "int"), "NULL");
    assert_eq!(
        default_clause("nullable", false, "varchar(20)"),
        "'nullable'"
    );

    // A number is quoted too: MySQL reads a quoted number back as the number, so there is
    // nothing to gain from telling them apart and a value to lose by guessing wrong.
    assert_eq!(default_clause("42", false, "int"), "'42'");
    // A quote inside a literal is doubled rather than ending it early.
    assert_eq!(default_clause("it's", false, "varchar(20)"), "'it''s'");
}

/// A collation goes into DDL bare — MySQL has no quoted form for one — so anything not shaped
/// like a name is refused rather than escaped.
#[test]
fn a_collation_is_refused_unless_it_is_a_plain_name() {
    assert_eq!(
        validated_collation(Some("utf8mb4_unicode_ci")).unwrap(),
        Some("utf8mb4_unicode_ci"),
    );
    // Nothing asked for, in each of the three ways of asking for nothing.
    assert_eq!(validated_collation(None).unwrap(), None);
    assert_eq!(validated_collation(Some("")).unwrap(), None);
    assert_eq!(validated_collation(Some("   ")).unwrap(), None);

    for hostile in ["utf8mb4_bin; DROP TABLE t", "utf8mb4-bin", "utf8mb4_bin'"] {
        assert!(
            validated_collation(Some(hostile)).is_err(),
            "{hostile} was allowed through"
        );
    }
}

/// Each index kind and what MySQL will accept on it.
#[test]
fn an_index_clause_carries_only_what_its_kind_allows() {
    assert_eq!(
        add_index_clause(&index("index", &["a"])).unwrap(),
        "ADD INDEX (`a`)"
    );
    assert_eq!(
        add_index_clause(&index("unique", &["a", "b"])).unwrap(),
        "ADD UNIQUE INDEX (`a`, `b`)",
    );
    // Case is the frontend's business, not a reason to refuse.
    assert_eq!(
        add_index_clause(&index("FULLTEXT", &["a"])).unwrap(),
        "ADD FULLTEXT INDEX (`a`)"
    );

    // A primary key has no name, and MySQL accepts neither a comment nor USING on it — so a
    // spec carrying both still produces the bare form rather than a statement it would reject.
    let mut spec = index("primary", &["a"]);
    spec.name = "ignored".to_string();
    spec.comment = "ignored".to_string();
    spec.index_type = Some("BTREE".to_string());
    assert_eq!(add_index_clause(&spec).unwrap(), "ADD PRIMARY KEY (`a`)");

    // USING belongs to the two general-purpose kinds only.
    let mut spec = index("index", &["a"]);
    spec.name = "by_a".to_string();
    spec.index_type = Some("hash".to_string());
    spec.comment = "fast".to_string();
    assert_eq!(
        add_index_clause(&spec).unwrap(),
        "ADD INDEX `by_a` (`a`) USING HASH COMMENT 'fast'",
    );

    let mut spec = index("fulltext", &["a"]);
    spec.index_type = Some("BTREE".to_string());
    assert_eq!(add_index_clause(&spec).unwrap(), "ADD FULLTEXT INDEX (`a`)");

    // A prefix length, which a TEXT column cannot be indexed without.
    let mut spec = index("index", &["body"]);
    spec.columns[0].prefix_length = Some(32);
    assert_eq!(add_index_clause(&spec).unwrap(), "ADD INDEX (`body`(32))");
    // Zero is not a prefix; it is the absence of one.
    spec.columns[0].prefix_length = Some(0);
    assert_eq!(add_index_clause(&spec).unwrap(), "ADD INDEX (`body`)");
}

/// What an index clause refuses.
#[test]
fn an_index_needs_columns_and_a_kind_that_exists() {
    assert!(add_index_clause(&index("index", &[])).is_err());
    assert!(add_index_clause(&index("index", &["  "])).is_err());
    assert!(add_index_clause(&index("clustered", &["a"])).is_err());

    let mut spec = index("index", &["a"]);
    spec.index_type = Some("BRIN".to_string());
    assert!(add_index_clause(&spec).is_err());
}

/// The values MariaDB 11.8 actually reports for these declarations, read off a live server.
/// Each is the source text of the default rather than its value, which is the whole difference
/// from MySQL.
#[test]
fn a_mariadb_default_is_read_as_the_literal_it_is() {
    // A quoted literal comes back as its value, with the quoting undone.
    assert_eq!(
        mariadb_default(Some("'abc'".into())),
        (Some("abc".into()), false)
    );
    assert_eq!(
        mariadb_default(Some("'a''b'".into())),
        (Some("a'b".into()), false)
    );
    // A backslash in the value arrives doubled, the way the server would have to write it.
    assert_eq!(
        mariadb_default(Some(r"'a\\b'".into())),
        (Some(r"a\b".into()), false)
    );
    // And a single one is an escape, which is what doubling it is there to avoid: `\b` is a
    // backspace to MySQL and to MariaDB alike.
    assert_eq!(
        mariadb_default(Some(r"'a\b'".into())),
        (Some("a\u{8}".into()), false)
    );
    assert_eq!(
        mariadb_default(Some("''".into())),
        (Some(String::new()), false)
    );
    // A string that reads like NULL is still a string: it arrives quoted.
    assert_eq!(
        mariadb_default(Some("'NULL'".into())),
        (Some("NULL".into()), false)
    );
    // And a number stays the number it is, rather than becoming an expression.
    assert_eq!(mariadb_default(Some("7".into())), (Some("7".into()), false));
    assert_eq!(
        mariadb_default(Some("-3".into())),
        (Some("-3".into()), false)
    );
    assert_eq!(
        mariadb_default(Some("1.50".into())),
        (Some("1.50".into()), false)
    );
}

/// The bare word, which MariaDB writes both for `DEFAULT NULL` and for no default at all. Read
/// as no default either way — on a nullable column the two mean the same thing, and a column
/// that is not nullable reports SQL NULL instead.
#[test]
fn a_mariadb_null_default_is_no_default() {
    assert_eq!(mariadb_default(Some("NULL".into())), (None, false));
    assert_eq!(mariadb_default(None), (None, false));
}

/// What is neither quoted nor a number can only be an expression — which is the distinction
/// MySQL draws with `DEFAULT_GENERATED` in `EXTRA`, and MariaDB does not draw at all.
#[test]
fn a_mariadb_expression_default_is_marked_as_one() {
    assert_eq!(
        mariadb_default(Some("current_timestamp()".into())),
        (Some("current_timestamp()".into()), true)
    );
    assert_eq!(
        mariadb_default(Some("uuid()".into())),
        (Some("uuid()".into()), true)
    );
    assert_eq!(
        mariadb_default(Some("(1 + 1)".into())),
        (Some("(1 + 1)".into()), true)
    );
}

/// Whatever is read out of a default has to survive being written back into one, or editing a
/// column would rewrite the default it was only meant to leave alone.
///
/// `'NULL'` is left out, and not because MariaDB is any trouble: it is read back correctly
/// here, but [`default_clause`] writes the four characters out as SQL NULL by design — typing
/// `NULL` into the default box is how a user asks for SQL NULL. MySQL loses that default in
/// exactly the same way, so it is nothing this reading introduced.
#[test]
fn a_default_read_from_mariadb_goes_back_the_way_it_came() {
    for reported in [
        "'abc'",
        "'a''b'",
        "''",
        "7",
        "current_timestamp()",
        "(1 + 1)",
    ] {
        let (value, is_expression) = mariadb_default(Some(reported.into()));
        let value = value.expect("none of these is an absent default");
        let clause = default_clause(&value, is_expression, "varchar(32)");
        // Re-reading the clause the way MariaDB would report it again lands on the same value.
        let (again, _) = mariadb_default(Some(clause));
        assert_eq!(again.as_deref(), Some(value.as_str()), "{reported}");
    }
}
