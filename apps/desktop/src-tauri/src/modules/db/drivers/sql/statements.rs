//! Carving a script into the statements that are sent one at a time, for every SQL engine but
//! ClickHouse (see `syntax.rs` for why that one reads its own way).
//!
//! What differs between engines is lexical and travels in a [`SqlSyntax`]; everything else is the
//! same for all of them. Only a semicolon outside a string, a quoted name, a comment and, where the
//! engine has them, a dollar-quoted body separates two statements, and on SQL Server so does a
//! `GO` line. Comments are kept in the text: they may carry a hint, and dropping them would change
//! what the server is asked to run.
//!
//! This is ported to `src/modules/db/sql/statements.ts`, which the editor splits with so that the
//! statement it highlights is the one the server ends up running. **A change to either splitter
//! belongs in the same commit as the other**, and in both sets of tests.

use super::syntax::SqlSyntax;

/// One statement carved out of the editor's text.
pub struct Statement {
    pub text: String,
    /// The keyword it opens with, upper-cased. Never empty: a run of nothing but whitespace and
    /// comments is not a statement.
    pub verb: String,
}

/// What may sit inside a PostgreSQL identifier after its first character, `$` included.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Whether the `'` at `chars[i]` opens an `E'...'` literal, the one PostgreSQL string a backslash
/// escapes in. The prefix has to be touching, and cannot be the tail of a longer name.
fn opens_escape_string(chars: &[char], i: usize) -> bool {
    if i == 0 || !matches!(chars[i - 1], 'E' | 'e') {
        return false;
    }
    i < 2 || !is_ident_char(chars[i - 2])
}

/// The tag of a dollar quote opening at `chars[i]`, when one does: `$$` gives `""`, `$body$` gives
/// `"body"`, and anything else, `$1` or a lone `$`, gives `None`.
///
/// A tag is an identifier: letters, digits and underscores, not starting with a digit. That last
/// rule is what keeps the placeholders `$1, $2` out of this, since they would otherwise read as an
/// opening quote that never closes and swallow the rest of the script.
fn dollar_tag(chars: &[char], i: usize) -> Option<String> {
    if chars.get(i) != Some(&'$') {
        return None;
    }
    // Glued to the end of a name it belongs to that name: `$` is legal inside a PostgreSQL
    // identifier and the server reads the longest one it can before looking for a quote, so
    // `SELECT 1 AS a$b$c` names a column `a$b$c`. Read as an opening quote, `$b$` swallows
    // everything up to the next one of itself, or the rest of the script.
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    let mut tag = String::new();
    let mut j = i + 1;
    while let Some(&c) = chars.get(j) {
        match c {
            '$' => return Some(tag),
            _ if c.is_alphanumeric() || c == '_' => {
                // A tag cannot open with a digit: `$1` is a placeholder, not a quote.
                if tag.is_empty() && c.is_numeric() {
                    return None;
                }
                tag.push(c);
                j += 1;
            }
            _ => return None,
        }
    }
    None
}

/// If a `GO` batch separator begins at `chars[i]`, the index right after its line, so the caller
/// can skip past it; `None` when this is not one.
///
/// The same rule as `matchBatchSeparator` in `statements.ts`: nothing but whitespace between the
/// previous newline and here, read from the script itself rather than from the statement being
/// built, so `SELECT 1; GO` is not a separator. Then `GO` in any case, not followed by a word
/// character, so `GOOD` and `GO3` are not either; spaces or tabs; an optional repeat count with
/// spaces of its own; an optional `\r`; and a newline or the end of the text.
fn batch_separator_end(chars: &[char], i: usize) -> Option<usize> {
    let line_start = chars[..i]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |n| n + 1);
    if chars[line_start..i].iter().any(|c| !c.is_whitespace()) {
        return None;
    }
    if !matches!(chars.get(i), Some('g' | 'G')) || !matches!(chars.get(i + 1), Some('o' | 'O')) {
        return None;
    }
    let mut j = i + 2;
    if chars
        .get(j)
        .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    {
        return None;
    }
    let skip_blanks = |mut j: usize| {
        while matches!(chars.get(j), Some(' ' | '\t')) {
            j += 1;
        }
        j
    };
    j = skip_blanks(j);
    if chars.get(j).is_some_and(char::is_ascii_digit) {
        while chars.get(j).is_some_and(char::is_ascii_digit) {
            j += 1;
        }
        j = skip_blanks(j);
    }
    if chars.get(j) == Some(&'\r') {
        j += 1;
    }
    match chars.get(j) {
        None => Some(j),
        Some('\n') => Some(j + 1),
        Some(_) => None,
    }
}

/// Splits a script into the statements that are to be sent one at a time, reading it by `syntax`.
pub fn split_statements(sql: &str, syntax: &SqlSyntax) -> Vec<Statement> {
    let chars: Vec<char> = sql.chars().collect();
    let mut statements: Vec<Statement> = Vec::new();
    let mut current = String::new();
    let mut verb = String::new();
    // Set once the opening word has ended, so the words after it can't overwrite it.
    let mut verb_done = false;
    let mut i = 0;

    fn push(
        statements: &mut Vec<Statement>,
        current: &mut String,
        verb: &mut String,
        verb_done: &mut bool,
    ) {
        let text = current.trim().to_string();
        current.clear();
        *verb_done = false;
        let verb = std::mem::take(verb);
        // No opening keyword means there was no statement here: only whitespace, or a comment
        // sitting between two separators.
        if verb.is_empty() {
            return;
        }
        statements.push(Statement { text, verb });
    }

    while i < chars.len() {
        let c = chars[i];

        if c == '-'
            && chars.get(i + 1) == Some(&'-')
            && (!syntax.dash_comment_needs_space
                || matches!(chars.get(i + 2), None | Some(' ' | '\t' | '\n' | '\r')))
        {
            while i < chars.len() && chars[i] != '\n' {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '#' && syntax.hash_comments {
            while i < chars.len() && chars[i] != '\n' {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    current.push_str("/*");
                    i += 2;
                    continue;
                }
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    current.push_str("*/");
                    i += 2;
                    // Where comments do not nest the first close ends it, whatever the count says.
                    if depth == 0 || !syntax.nested_block_comments {
                        break;
                    }
                    continue;
                }
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // A dollar-quoted body ends only at its own tag, so a `$$ ... ; ... $$` function body is
        // one statement however many semicolons it holds.
        if syntax.dollar_quoting {
            if let Some(tag) = dollar_tag(&chars, i) {
                let close: Vec<char> = format!("${tag}$").chars().collect();
                current.extend(&close);
                i += close.len();
                while i < chars.len() {
                    if chars[i..].starts_with(&close) {
                        current.extend(&close);
                        i += close.len();
                        break;
                    }
                    current.push(chars[i]);
                    i += 1;
                }
                if !verb.is_empty() {
                    verb_done = true;
                }
                continue;
            }
        }

        let identifier = syntax.identifier_quote.filter(|(open, _)| *open == c);
        if c == '\'' || c == '"' || identifier.is_some() {
            // The character that closes this run: the one it opened with, unless it opened an
            // identifier quote whose close differs, like SQL Server's `[` ... `]`.
            let close = identifier.map_or(c, |(_, close)| close);
            // Whether a backslash escapes inside this literal. On MySQL it always does; on
            // PostgreSQL only in an `E'...'`. Inside a quoted name it never does: there, doubling
            // is the only escape.
            let escapes = identifier.is_none()
                && (syntax.backslash_escapes
                    || (syntax.escape_string_prefix
                        && c == '\''
                        && opens_escape_string(&chars, i)));
            current.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                current.push(ch);
                i += 1;
                if ch == '\\' && escapes {
                    if let Some(&next) = chars.get(i) {
                        current.push(next);
                        i += 1;
                    }
                    continue;
                }
                if ch == close {
                    // Two of the close character in a row are an escaped one, not the end of the
                    // run.
                    if chars.get(i) == Some(&close) {
                        current.push(close);
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }

        if c == ';' {
            push(&mut statements, &mut current, &mut verb, &mut verb_done);
            i += 1;
            continue;
        }

        if syntax.batch_separator && matches!(c, 'g' | 'G') {
            if let Some(end) = batch_separator_end(&chars, i) {
                push(&mut statements, &mut current, &mut verb, &mut verb_done);
                i = end;
                continue;
            }
        }

        // Plain code: the first word of it is the statement's keyword.
        if !verb_done {
            if c.is_alphanumeric() || c == '_' {
                verb.extend(c.to_uppercase());
            } else if !verb.is_empty() {
                verb_done = true;
            }
        }
        current.push(c);
        i += 1;
    }

    push(&mut statements, &mut current, &mut verb, &mut verb_done);
    statements
}

#[cfg(test)]
mod tests {
    use super::split_statements;
    use crate::modules::db::drivers::sql::syntax::{SqlSyntax, MYSQL, POSTGRES, SQLITE};

    fn verbs(sql: &str, syntax: &SqlSyntax) -> Vec<String> {
        split_statements(sql, syntax)
            .into_iter()
            .map(|s| s.verb)
            .collect()
    }

    fn texts(sql: &str, syntax: &SqlSyntax) -> Vec<String> {
        split_statements(sql, syntax)
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    #[test]
    fn dash_dash_needs_whitespace_only_where_the_syntax_says() {
        assert_eq!(
            texts("SELECT 5--3; SELECT 2", &MYSQL),
            ["SELECT 5--3", "SELECT 2"]
        );
        assert_eq!(verbs("SELECT 5--3; SELECT 2", &POSTGRES), ["SELECT"]);
    }

    #[test]
    fn hash_opens_a_comment_only_on_mysql() {
        assert_eq!(
            texts("SELECT 1 # a;b\n; SELECT 2", &MYSQL),
            ["SELECT 1 # a;b", "SELECT 2"]
        );
        assert_eq!(
            verbs("SELECT 1 # a;b\n; SELECT 2", &POSTGRES),
            ["SELECT", "B", "SELECT"]
        );
    }

    #[test]
    fn block_comments_nest_only_where_the_syntax_says() {
        assert_eq!(
            texts("/* /* */ ; */ SELECT 1; SELECT 2", &POSTGRES)[0],
            "/* /* */ ; */ SELECT 1"
        );
        assert_eq!(
            texts("/* /* */ ; */ SELECT 1; SELECT 2", &SQLITE)[0],
            "*/ SELECT 1"
        );
    }

    #[test]
    fn identifier_quotes_hold_semicolons_and_double_to_escape() {
        assert_eq!(
            texts("SELECT `a;``b`; SELECT 2", &MYSQL)[0],
            "SELECT `a;``b`"
        );
        assert_eq!(verbs("SELECT `a;b`", &POSTGRES), ["SELECT", "B"]);
    }

    #[test]
    fn backslash_escapes_follow_the_syntax() {
        assert_eq!(
            texts(r"SELECT 'a\';b'; SELECT 2", &MYSQL),
            [r"SELECT 'a\';b'", "SELECT 2"]
        );
        assert_eq!(verbs(r"SELECT 'a\';b'; SELECT 2", &SQLITE), ["SELECT", "B"]);
        assert_eq!(
            texts(r"SELECT `a\`; SELECT 2", &MYSQL),
            [r"SELECT `a\`", "SELECT 2"]
        );
    }

    #[test]
    fn an_e_string_escapes_with_a_backslash_on_postgres() {
        assert_eq!(
            texts(r"SELECT E'a\';b'; SELECT 2", &POSTGRES),
            [r"SELECT E'a\';b'", "SELECT 2"]
        );
        assert_eq!(
            texts(r"SELECT 'a\'; SELECT 2", &POSTGRES),
            [r"SELECT 'a\'", "SELECT 2"]
        );
    }

    #[test]
    fn a_dollar_quoted_body_is_one_statement() {
        let sql =
            "CREATE FUNCTION f() RETURNS int AS $body$ SELECT 1; $body$ LANGUAGE sql; SELECT 2";
        assert_eq!(verbs(sql, &POSTGRES), ["CREATE", "SELECT"]);
    }

    #[test]
    fn placeholders_and_names_are_not_dollar_quotes() {
        assert_eq!(
            verbs("SELECT $1; SELECT 2", &POSTGRES),
            ["SELECT", "SELECT"]
        );
        assert_eq!(
            verbs("SELECT 1 AS a$b$c; SELECT 2", &POSTGRES),
            ["SELECT", "SELECT"]
        );
    }

    #[test]
    fn a_chunk_without_a_verb_is_not_a_statement() {
        assert_eq!(verbs("SELECT 1;; -- done\n", &SQLITE), ["SELECT"]);
        assert_eq!(verbs("  -- c\n select 1", &SQLITE), ["SELECT"]);
    }
}
