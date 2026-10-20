//! The part of the grid's filter bar that reads the same whichever database is underneath: how
//! the one text box a row gives an `IN`/`BETWEEN` operator is split into its items. Turning those
//! items into a query is per-database — `build_where` in `mysql.rs`, `build_filter` in `mongo.rs`.
//! What both SQL servers spell the same way is here too.

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

#[cfg(test)]
mod where_clauses;

#[cfg(test)]
mod tests {
    use super::{escape_like, split_list_parts, unquote};

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
}
