//! The handful of lexical rules that differ between the SQL engines: what a reader of SQL text has
//! to know before it can tell code from a string, a comment or a name.
//!
//! This mirrors `SqlSyntax` in `src/modules/db/sql/syntax.ts`, field for field, and the two must
//! agree: the window splits a script with that one and the backend with this one, and the
//! statement the editor highlights has to be the one the server ends up running.
//!
//! ClickHouse has no constant here. Its script is read by the `Scanner` in `clickhouse/script.rs`,
//! which resumes across the chunks a dump file is read in, and which escapes a quoted name with a
//! backslash, a rule none of these engines has.

/// The lexical rules one engine reads a script by.
pub struct SqlSyntax {
    /// `#` opens a comment to the end of the line. MySQL only; elsewhere it is an operator or, on
    /// SQL Server, the start of a temporary table's name.
    pub hash_comments: bool,
    /// `--` opens a comment only when whitespace or the end of the text follows it, so that
    /// `5--3` stays arithmetic. MySQL only.
    pub dash_comment_needs_space: bool,
    /// `/* a /* b */ c */` is one comment. PostgreSQL only; elsewhere the first `*/` closes it.
    pub nested_block_comments: bool,
    /// The engine's own identifier quote beside the standard `"`, as its open and close character.
    /// SQL Server's is not symmetric: `[name]`, with `]` doubled to escape one inside the name.
    pub identifier_quote: Option<(char, char)>,
    /// A backslash escapes the next character inside a string literal, never inside a quoted name.
    pub backslash_escapes: bool,
    /// `E'...'` is a string in which a backslash does escape. PostgreSQL only.
    pub escape_string_prefix: bool,
    /// `$tag$ ... $tag$` holds text that may contain anything, semicolons included. PostgreSQL only.
    pub dollar_quoting: bool,
    /// A line holding nothing but `GO`, optionally with a repeat count, ends the batch. A client
    /// convention rather than a keyword: sent to the server it is a syntax error. SQL Server only.
    pub batch_separator: bool,
}

pub const MYSQL: SqlSyntax = SqlSyntax {
    hash_comments: true,
    dash_comment_needs_space: true,
    nested_block_comments: false,
    identifier_quote: Some(('`', '`')),
    backslash_escapes: true,
    escape_string_prefix: false,
    dollar_quoting: false,
    batch_separator: false,
};

pub const POSTGRES: SqlSyntax = SqlSyntax {
    hash_comments: false,
    dash_comment_needs_space: false,
    nested_block_comments: true,
    identifier_quote: None,
    backslash_escapes: false,
    escape_string_prefix: true,
    dollar_quoting: true,
    batch_separator: false,
};

/// SQLite understands MySQL's backtick as well as the standard double quote, and a backslash inside
/// a string is just a backslash. Its `[name]` quote is deliberately not read, because the window's
/// splitter does not read it either.
pub const SQLITE: SqlSyntax = SqlSyntax {
    hash_comments: false,
    dash_comment_needs_space: false,
    nested_block_comments: false,
    identifier_quote: Some(('`', '`')),
    backslash_escapes: false,
    escape_string_prefix: false,
    dollar_quoting: false,
    batch_separator: false,
};
