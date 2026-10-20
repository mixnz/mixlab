//! The part of the grid's filter bar that reads the same whichever database is underneath: how
//! the one text box a row gives an `IN`/`BETWEEN` operator is split into its items. Turning those
//! items into a query is per-database: `build_filter` in `mongo.rs` builds a document, and
//! [`build_where`] here builds the SQL engines' `WHERE` from what each says of itself in a
//! [`WhereSyntax`] and its [`Placeholders`].

use crate::error::AppError;
use crate::modules::db::drivers::sql::types::Filter;

/// One item of a split value, and whether it arrived in quotes. The quotes are what a caller that
/// infers types from the text goes by: `123` is a number, `'123'` is that number spelled out.
pub struct ListItem {
    pub text: String,
    pub quoted: bool,
}

/// Splits an `IN`/`BETWEEN` value into its items: comma-separated, with each item trimmed.
///
/// An item may be wrapped in single or double quotes, which is how a value that itself contains a
/// comma (or leading/trailing spaces that matter) gets through: the quotes are stripped and the
/// text inside them is taken as-is. Empty items are dropped, so `1,2,` is two items — but a
/// quoted `''` is kept, as that is the only way to ask for an empty string in a list.
///
/// The frontend mirrors this in `src/filters.ts` to decide whether a row has enough to be worth
/// sending; the two must agree on how many items a value holds.
pub fn split_list_parts(raw: &str) -> Vec<ListItem> {
    let mut items: Vec<ListItem> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut quoted = false;

    let mut flush = |current: &mut String, quoted: &mut bool| {
        let text = if *quoted {
            std::mem::take(current)
        } else {
            current.trim().to_string()
        };
        if *quoted || !text.is_empty() {
            items.push(ListItem {
                text,
                quoted: *quoted,
            });
        }
        current.clear();
        *quoted = false;
    };

    for ch in raw.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => {
                // Whitespace between the comma and the opening quote is padding around the item
                // rather than part of it — a quoted item is not trimmed afterwards, so `'a', 'b'`
                // would otherwise ask for a value beginning with a space.
                if current.trim().is_empty() {
                    current.clear();
                }
                quote = Some(ch);
                quoted = true;
            }
            None if ch == ',' => flush(&mut current, &mut quoted),
            // The same padding on the other side of the closing quote.
            None if quoted && ch.is_whitespace() => {}
            None => current.push(ch),
        }
    }
    flush(&mut current, &mut quoted);
    items
}

/// {@link split_list_parts} for a caller that binds every item as text anyway and so has no use
/// for knowing which of them were quoted.
pub fn split_list(raw: &str) -> Vec<String> {
    split_list_parts(raw)
        .into_iter()
        .map(|item| item.text)
        .collect()
}

/// Takes the quotes off a value that is wrapped in a matching pair of them, which is how a single
/// value is spelled when it is to be read as text and nothing else. Anything not so wrapped comes
/// back as `None` and is left to whatever the caller infers from it.
pub fn unquote(raw: &str) -> Option<&str> {
    let mut chars = raw.chars();
    let first = chars.next()?;
    if first != '\'' && first != '"' {
        return None;
    }
    let last = chars.next_back()?;
    if last != first {
        return None;
    }
    Some(&raw[first.len_utf8()..raw.len() - last.len_utf8()])
}

/// The wildcards out of a value going into a `LIKE`.
///
/// MySQL and PostgreSQL both take `\` as the escape character in a `LIKE` pattern unless told
/// otherwise, so this is one function rather than two — the backslash first, or it would escape
/// the escapes added after it.
///
/// A `contains` filter for `50%` must match the text `50%`, not "starts with 50". The user is
/// filtering a column, not writing a pattern; the bar has no syntax for one and this is what keeps
/// it that way.
pub fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Which `LIKE` clauses end in `ESCAPE '\'`, naming the escape character [`escape_like`] uses.
pub enum LikeEscape {
    /// None of them: the server takes a backslash as the escape character on its own.
    Never,
    /// The four whose value this code escaped: `contains`, `notContains`, `startsWith`,
    /// `endsWith`. `like` and `notLike` hand over the pattern as the user wrote it.
    Escaped,
    /// Every one, `like` and `notLike` included, so a pattern of the user's own can escape too.
    Always,
}

/// What one SQL engine writes differently in a `WHERE` built from the filter bar. How a value is
/// spelled and bound is the other half, [`Placeholders`].
pub struct WhereSyntax {
    pub quote_ident: fn(&str) -> String,
    /// The quoted column as compared with text. An engine that coerces text on its own returns it
    /// as it is.
    pub as_text: fn(&str) -> String,
    /// The regex operator and its negation, or `None` when the engine has none and the operator
    /// is refused like any unknown one.
    pub regexp: Option<(&'static str, &'static str)>,
    pub escape_like: fn(&str) -> String,
    pub like_escape: LikeEscape,
}

/// How an engine spells a bound value in the SQL text, and what it keeps to bind into it.
pub trait Placeholders {
    type Bind;
    /// Binds a value compared as text, and returns what stands for it in the SQL.
    fn text(&mut self, value: String) -> String;
    /// Binds a value compared as the column's own type, which an ordering operator needs: as text,
    /// `10` sorts before `9`.
    fn typed(&mut self, value: String, column_type: &str) -> String;
    fn into_binds(self) -> Vec<Self::Bind>;
}

/// `?` for every value, as MySQL and SQLite write them. Both coerce a text value to the column's
/// type themselves, so a typed value is spelled the same.
#[derive(Default)]
pub struct QuestionMarks {
    binds: Vec<String>,
}

impl Placeholders for QuestionMarks {
    type Bind = String;

    fn text(&mut self, value: String) -> String {
        self.binds.push(value);
        "?".to_string()
    }

    fn typed(&mut self, value: String, _column_type: &str) -> String {
        self.text(value)
    }

    fn into_binds(self) -> Vec<String> {
        self.binds
    }
}

/// Numbered placeholders, `$1` or `@P1`, counted from one in the order they are written. With
/// `cast_typed`, a typed value is written `CAST($n AS <type>)`, for a server that will not
/// compare a column with a value bound as text.
pub struct Numbered {
    prefix: &'static str,
    cast_typed: bool,
    binds: Vec<String>,
}

impl Numbered {
    pub fn new(prefix: &'static str, cast_typed: bool) -> Self {
        Self {
            prefix,
            cast_typed,
            binds: Vec::new(),
        }
    }
}

impl Placeholders for Numbered {
    type Bind = String;

    fn text(&mut self, value: String) -> String {
        self.binds.push(value);
        format!("{}{}", self.prefix, self.binds.len())
    }

    fn typed(&mut self, value: String, column_type: &str) -> String {
        let placeholder = self.text(value);
        if self.cast_typed {
            format!("CAST({placeholder} AS {column_type})")
        } else {
            placeholder
        }
    }

    fn into_binds(self) -> Vec<String> {
        self.binds
    }
}

/// Turns the filter rows into a WHERE clause (leading space included, empty when nothing filters)
/// and the values to bind into its placeholders, in order.
///
/// Every value reaches the server as a bound parameter, never as SQL text. The column name is the
/// one part that has to be interpolated, which is why `column_type` is asked first: `None` refuses
/// a column the table does not have. An engine that does not need the type answers `Some("")`.
///
/// A row whose operator wants more than it was given is dropped rather than matched literally: an
/// `in` with no items, a `between` with one bound. A missing value is the empty string.
///
/// Text comparisons use the column's text form, so a value typed into the bar compares as typed.
/// The ordering operators, `between` and the null checks use the column as it stands, since
/// `>` on text would sort 10 before 9.
pub fn build_where<'c, P: Placeholders>(
    filters: &[Filter],
    column_type: impl Fn(&str) -> Option<&'c str>,
    syntax: &WhereSyntax,
    mut placeholders: P,
) -> Result<(String, Vec<P::Bind>), AppError> {
    let mut clauses: Vec<String> = Vec::new();

    for filter in filters {
        let Some(base_type) = column_type(&filter.column) else {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        };
        let raw = (syntax.quote_ident)(&filter.column);
        let col = (syntax.as_text)(&raw);
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();
        let p = &mut placeholders;

        let like = |negated: bool, escaped: bool, placeholder: String| {
            let not = if negated { "NOT " } else { "" };
            let escape = match syntax.like_escape {
                LikeEscape::Never => false,
                LikeEscape::Escaped => escaped,
                LikeEscape::Always => true,
            };
            let escape = if escape { " ESCAPE '\\'" } else { "" };
            format!("{col} {not}LIKE {placeholder}{escape}")
        };
        let escaped = (syntax.escape_like)(value);

        let clause = match operator {
            "eq" => format!("{col} = {}", p.text(value.to_string())),
            "ne" => format!("{col} <> {}", p.text(value.to_string())),
            "gt" => format!("{raw} > {}", p.typed(value.to_string(), base_type)),
            "gte" => format!("{raw} >= {}", p.typed(value.to_string(), base_type)),
            "lt" => format!("{raw} < {}", p.typed(value.to_string(), base_type)),
            "lte" => format!("{raw} <= {}", p.typed(value.to_string(), base_type)),
            "contains" => like(false, true, p.text(format!("%{escaped}%"))),
            "notContains" => like(true, true, p.text(format!("%{escaped}%"))),
            "startsWith" => like(false, true, p.text(format!("{escaped}%"))),
            "endsWith" => like(false, true, p.text(format!("%{escaped}"))),
            "like" => like(false, false, p.text(value.to_string())),
            "notLike" => like(true, false, p.text(value.to_string())),
            "regexp" | "notRegexp" => {
                let Some((matches, not_matches)) = syntax.regexp else {
                    return Err(err!("error.unknownFilterOperator", operator = operator));
                };
                let sql_op = if operator == "regexp" {
                    matches
                } else {
                    not_matches
                };
                format!("{col} {sql_op} {}", p.text(value.to_string()))
            }
            "in" | "notIn" => {
                let items = split_list(value);
                if items.is_empty() {
                    continue;
                }
                let list = items
                    .into_iter()
                    .map(|item| p.text(item))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql_op = if operator == "in" { "IN" } else { "NOT IN" };
                format!("{col} {sql_op} ({list})")
            }
            "between" | "notBetween" => {
                let mut items = split_list(value).into_iter();
                // Two bounds or nothing: one of them alone says nothing about a range.
                let (Some(low), Some(high)) = (items.next(), items.next()) else {
                    continue;
                };
                let low = p.typed(low, base_type);
                let high = p.typed(high, base_type);
                let sql_op = if operator == "between" {
                    "BETWEEN"
                } else {
                    "NOT BETWEEN"
                };
                format!("{raw} {sql_op} {low} AND {high}")
            }
            "isNull" => format!("{raw} IS NULL"),
            "isNotNull" => format!("{raw} IS NOT NULL"),
            "isEmpty" => format!("{col} = ''"),
            "isNotEmpty" => format!("{col} <> ''"),
            other => return Err(err!("error.unknownFilterOperator", operator = other)),
        };

        clauses.push(clause);
    }

    let binds = placeholders.into_binds();
    if clauses.is_empty() {
        return Ok((String::new(), binds));
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), binds))
}

#[cfg(test)]
mod where_clauses;

#[cfg(test)]
mod tests {
    use super::{
        build_where, escape_like, split_list_parts, unquote, LikeEscape, Numbered, QuestionMarks,
        WhereSyntax,
    };
    use crate::modules::db::drivers::sql::types::Filter;

    fn texts(raw: &str) -> Vec<String> {
        split_list_parts(raw)
            .into_iter()
            .map(|item| item.text)
            .collect()
    }

    #[test]
    fn splits_on_commas_and_trims_what_is_left() {
        assert_eq!(texts("1, 2 ,3"), ["1", "2", "3"]);
        // A trailing comma names no item, so `1,2,` is two items and not three.
        assert_eq!(texts("1,2,"), ["1", "2"]);
        assert_eq!(texts("   "), Vec::<String>::new());
    }

    /// Quotes are how a value carrying a comma — or spaces that matter — gets through in one
    /// piece, and the only way to ask for the empty string.
    #[test]
    fn quotes_hold_a_value_together() {
        assert_eq!(texts("'a,b', c"), ["a,b", "c"]);
        assert_eq!(texts("' padded '"), [" padded "]);
        assert_eq!(texts("''"), [""]);
        assert_eq!(texts(r#""double", 'single'"#), ["double", "single"]);
    }

    /// The spaces a list is typed with are around the items, not in them: only what stands between
    /// the quotes is the value.
    #[test]
    fn ignores_the_spacing_around_a_quoted_item() {
        assert_eq!(texts("'a' , 'b'"), ["a", "b"]);
        assert_eq!(texts("  'a'  "), ["a"]);
    }

    /// Whether an item arrived quoted is what a caller inferring types goes by: `123` is a number,
    /// `'123'` is that number spelled out.
    #[test]
    fn reports_which_items_were_quoted() {
        let parts = split_list_parts("123, '123'");
        assert_eq!(parts.len(), 2);
        assert!(!parts[0].quoted);
        assert!(parts[1].quoted);
    }

    #[test]
    fn unquotes_only_a_matching_pair() {
        assert_eq!(unquote("'x'"), Some("x"));
        assert_eq!(unquote("\"x\""), Some("x"));
        assert_eq!(unquote("''"), Some(""));
        assert_eq!(unquote("'x\""), None);
        assert_eq!(unquote("x"), None);
        assert_eq!(unquote("'"), None);
    }

    #[test]
    fn like_wildcards_are_escaped_so_a_filter_is_not_a_pattern() {
        // A `contains` filter for `50%` means the text `50%`, not "starts with 50".
        assert_eq!(escape_like("50%"), r"50\%");
        assert_eq!(escape_like("a_b"), r"a\_b");
    }

    #[test]
    fn the_escape_character_is_escaped_first() {
        // Backslash last would escape the escapes already added, and `\%` would come out as a
        // literal backslash followed by a live wildcard.
        assert_eq!(escape_like(r"a\b"), r"a\\b");
        assert_eq!(escape_like(r"\%"), r"\\\%");
    }

    #[test]
    fn text_with_no_wildcards_in_it_is_left_alone() {
        assert_eq!(escape_like("plain text"), "plain text");
    }

    /// A made-up engine, so that what `build_where` writes is told apart from what an engine
    /// fills in: names in `<…>`, the text form as `T(…)`, and a regex spelled `RX`.
    fn syntax(regexp: bool, like_escape: LikeEscape) -> WhereSyntax {
        WhereSyntax {
            quote_ident: |name| format!("<{name}>"),
            as_text: |quoted| format!("T({quoted})"),
            regexp: regexp.then_some(("RX", "NOT RX")),
            escape_like,
            like_escape,
        }
    }

    fn filter(column: &str, operator: &str, value: &str) -> Filter {
        Filter {
            column: column.to_string(),
            operator: operator.to_string(),
            value: Some(value.to_string()),
        }
    }

    fn column_type(name: &str) -> Option<&'static str> {
        (name == "id").then_some("t")
    }

    fn clause(filters: &[Filter], syntax: &WhereSyntax) -> String {
        build_where(filters, column_type, syntax, QuestionMarks::default())
            .expect("a known column and operator")
            .0
    }

    #[test]
    fn like_escape_never_escaped_and_always() {
        let contains = [filter("id", "contains", "a")];
        let like = [filter("id", "like", "a")];

        let never = syntax(false, LikeEscape::Never);
        assert_eq!(clause(&contains, &never), " WHERE T(<id>) LIKE ?");
        assert_eq!(clause(&like, &never), " WHERE T(<id>) LIKE ?");

        let escaped = syntax(false, LikeEscape::Escaped);
        assert_eq!(
            clause(&contains, &escaped),
            r" WHERE T(<id>) LIKE ? ESCAPE '\'"
        );
        assert_eq!(clause(&like, &escaped), " WHERE T(<id>) LIKE ?");

        let always = syntax(false, LikeEscape::Always);
        assert_eq!(
            clause(&contains, &always),
            r" WHERE T(<id>) LIKE ? ESCAPE '\'"
        );
        assert_eq!(
            clause(&[filter("id", "notLike", "a")], &always),
            r" WHERE T(<id>) NOT LIKE ? ESCAPE '\'"
        );
    }

    #[test]
    fn a_syntax_without_regexp_refuses_it() {
        let error = build_where(
            &[filter("id", "regexp", "^a")],
            column_type,
            &syntax(false, LikeEscape::Never),
            QuestionMarks::default(),
        )
        .expect_err("this syntax has no regex");
        assert_eq!(error.code, "error.unknownFilterOperator");

        assert_eq!(
            clause(
                &[filter("id", "notRegexp", "^a")],
                &syntax(true, LikeEscape::Never)
            ),
            " WHERE T(<id>) NOT RX ?"
        );
    }

    #[test]
    fn numbered_casts_only_the_typed_values() {
        let (clause, binds) = build_where(
            &[
                filter("id", "eq", "x"),
                filter("id", "gt", "1"),
                filter("id", "between", "2,3"),
            ],
            column_type,
            &syntax(false, LikeEscape::Never),
            Numbered::new("$", true),
        )
        .expect("known columns and operators");
        assert_eq!(
            clause,
            " WHERE T(<id>) = $1 AND <id> > CAST($2 AS t) \
             AND <id> BETWEEN CAST($3 AS t) AND CAST($4 AS t)"
        );
        assert_eq!(binds, ["x", "1", "2", "3"]);
    }

    #[test]
    fn question_marks_ignore_the_type() {
        assert_eq!(
            clause(
                &[filter("id", "gt", "1")],
                &syntax(false, LikeEscape::Never)
            ),
            " WHERE <id> > ?"
        );
    }
}
