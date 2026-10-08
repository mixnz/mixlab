//! The one `.env` line a blueprint may ask an apply to write — roadmap task **T205a**.
//!
//! **Appended, never rewritten.** A project's `.env` is the person's file: every line already in it
//! is kept byte for byte, and a key that is already there — with `export` or without — is theirs,
//! so nothing is written over it.

use std::path::Path;

use crate::Result;

/// The file, at the project root.
pub const FILE: &str = ".env";

/// Whether `key` is a variable name every `.env` reader accepts: ASCII letters, digits and `_`,
/// not starting with a digit.
#[must_use]
pub fn is_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// RFC 3986's unreserved set passes; every other byte is `%XX`. A password a person chose with
/// `mix database create --password` may hold anything, and a URL that read back as another
/// password would be worse than none.
fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The database's address as a URL, user, password and name percent-encoded.
#[must_use]
pub fn database_url(
    scheme: &str,
    user: &str,
    password: &str,
    host: &str,
    port: u16,
    database: &str,
) -> String {
    let host = match host.contains(':') {
        true => format!("[{host}]"),
        false => host.to_owned(),
    };
    format!(
        "{scheme}://{}:{}@{host}:{port}/{}",
        encoded(user),
        encoded(password),
        encoded(database)
    )
}

/// Whether `text` already sets `key`: `KEY=`, `export KEY=`, spaces around either. A commented
/// line does not count — `# KEY` is not the key.
#[must_use]
pub fn has_key(text: &str, key: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        let line = line.strip_prefix("export ").unwrap_or(line);
        line.split_once('=')
            .is_some_and(|(name, _)| name.trim() == key)
    })
}

/// `existing` with one `KEY=value` line after it, starting on a line of its own.
#[must_use]
pub fn appended(existing: &str, key: &str, value: &str) -> String {
    let mut text = existing.to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(key);
    text.push('=');
    text.push_str(value);
    text.push('\n');
    text
}

/// Append `key=value` to `<root>/.env` unless the key is already there; `true` when written.
///
/// **Private afterwards**, through the call that writes the CA's key: the line holds a password,
/// and an existing `.env` is restricted before it reaches it.
///
/// # Errors
///
/// When the file cannot be read, or cannot be written privately.
pub fn write(root: &Path, key: &str, value: &str) -> Result<bool> {
    let path = root.join(FILE);
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(crate::Error::Io {
                action: "read",
                path,
                source,
            });
        }
    };
    if has_key(&existing, key) {
        return Ok(false);
    }
    mixengine_platform::write_private(&path, appended(&existing, key, value).as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_variable_name() {
        assert!(is_key("DATABASE_URL"));
        assert!(is_key("_X1"));
        assert!(!is_key("1X"));
        assert!(!is_key(""));
        assert!(!is_key("A-B"));
        assert!(!is_key("A B"));
        assert!(!is_key("Ä"));
    }

    #[test]
    fn database_url_percent_encodes_every_reserved_character() {
        let url = database_url(
            "postgres",
            "my-blog",
            "p@ss:w/rd#%",
            "127.0.0.1",
            5432,
            "my-blog",
        );
        assert_eq!(
            url,
            "postgres://my-blog:p%40ss%3Aw%2Frd%23%25@127.0.0.1:5432/my-blog"
        );
    }

    #[test]
    fn an_ipv6_host_is_bracketed() {
        assert_eq!(
            database_url("postgres", "u", "p", "::1", 5432, "d"),
            "postgres://u:p@[::1]:5432/d"
        );
    }

    #[test]
    fn has_key_reads_export_spaces_and_ignores_comments() {
        assert!(has_key("DATABASE_URL=x\n", "DATABASE_URL"));
        assert!(has_key("export DATABASE_URL=x\n", "DATABASE_URL"));
        assert!(has_key("  DATABASE_URL = x\r\n", "DATABASE_URL"));
        assert!(!has_key("# DATABASE_URL=x\n", "DATABASE_URL"));
        assert!(!has_key("DATABASE_URL_2=x\n", "DATABASE_URL"));
        assert!(!has_key("", "DATABASE_URL"));
    }

    #[test]
    fn appending_to_text_without_a_final_newline_starts_a_new_line() {
        assert_eq!(appended("A=1", "B", "2"), "A=1\nB=2\n");
        assert_eq!(appended("", "B", "2"), "B=2\n");
    }

    #[test]
    fn appending_keeps_crlf_text_and_adds_a_line() {
        assert_eq!(appended("A=1\r\n", "B", "2"), "A=1\r\nB=2\n");
    }

    #[test]
    fn write_creates_appends_and_leaves_an_existing_key() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path();
        let read = || std::fs::read_to_string(root.join(FILE)).expect("written");

        assert!(write(root, "K", "v1").expect("created"));
        assert_eq!(read(), "K=v1\n");

        assert!(!write(root, "K", "v2").expect("left"));
        assert_eq!(read(), "K=v1\n");

        assert!(write(root, "L", "w").expect("appended"));
        assert_eq!(read(), "K=v1\nL=w\n");
    }
}
