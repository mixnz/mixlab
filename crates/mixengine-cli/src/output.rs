//! How an answer reaches the person or program running `mix`: the two renderings, stdout, stderr,
//! and the ASCII spelling a legacy code page asks for.

use std::io::Write as _;

use mixengine_proto::{Error, ErrorCode};

/// One of the two renderings of an answer, ready to be written.
///
/// The `--json` half is the daemon's answer **verbatim**, unlike `mix status`, whose envelope exists
/// so a captured diagnostic says which `mix` produced it. A script asking about services wants
/// `.services[]` and `.failed.reason.kind` where the API names them, and the daemon's build is one
/// `mix status` away.
pub(crate) fn rendered(
    json: bool,
    answer: &impl serde::Serialize,
    human: impl FnOnce() -> String,
) -> String {
    match json {
        true => format!(
            "{}\n",
            serde_json::to_string(answer).expect("a proto answer type always serialises")
        ),
        false => human(),
    }
}

/// Put the command's answer on stdout.
///
/// `write!` and not `print!`, for the reason [`report`] gives for stderr — the macro panics when the
/// write fails — but the two failures it can meet are not the same failure and are not answered the
/// same way. A reader that went away, `mix status | head -1`, is not this program's problem and is
/// what every well-behaved tool exits quietly on. Anything else — a full disk, a handle closed
/// before the process started — is a command that did not deliver its answer, and says so in the
/// same wire error every other failure here uses.
///
/// Flushed explicitly, because the lock is a `LineWriter`: a rendering that reaches the buffer and
/// no further would otherwise fail on drop, where the error is discarded and this run would have
/// exited zero having printed nothing.
pub(crate) fn emit(rendered: &str) -> Result<(), Error> {
    let mut stdout = std::io::stdout().lock();

    // T182e: a reader that cannot decode UTF-8 — the Windows uninstaller, through `nsExec` — asks
    // for ASCII look-alikes of the typography this crate's sentences use.
    let rendered = match std::env::var_os(PLAIN_TEXT) {
        Some(_) => plain_text(rendered),
        None => rendered.to_owned(),
    };

    stdout
        .write_all(rendered.as_bytes())
        .and_then(|()| stdout.flush())
        .or_else(|source| match source.kind() {
            std::io::ErrorKind::BrokenPipe => Ok(()),
            _ => Err(Error::new(
                ErrorCode::Io,
                format!("cannot write to stdout: {source}"),
            )),
        })
}

/// Set, to anything, by a caller that reads `mix`'s output in a legacy code page — T182e. The
/// Windows uninstaller does, since `nsExec` decodes what it captures as ANSI.
const PLAIN_TEXT: &str = "MIXENGINE_PLAIN_TEXT";

/// `text` with the typographic characters `mix` and the daemon write — dashes, curly quotes, the
/// ellipsis — spelled in ASCII, and everything else, a person's name in a path included, as it was.
fn plain_text(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\u{2014}' | '\u{2013}' | '\u{2012}' | '\u{2212}' => plain.push('-'),
            '\u{2018}' | '\u{2019}' => plain.push('\''),
            '\u{201C}' | '\u{201D}' => plain.push('"'),
            '\u{2026}' => plain.push_str("..."),
            other => plain.push(other),
        }
    }
    plain
}

/// Put a failure where the person or the program running `mix` will find it.
///
/// **stderr, in both renderings.** stdout carries the command's answer and nothing else, so a script
/// that redirects it into a file gets either a status object or an empty file — never an error
/// object where a status was meant to be.
pub(crate) fn report(error: &Error, json: bool) {
    let mut stderr = std::io::stderr().lock();

    // The `Display` in `mixengine-proto` is the human rendering: the message, and the hint on a
    // line of its own the way `cargo` prints one. A wire error is three owned strings and cannot
    // fail to serialise, so the fallback is a formality rather than a case.
    let rendered = match json {
        true => serde_json::to_string(error).unwrap_or_else(|_| format!("error: {error}")),
        false => format!("error: {error}"),
    };

    // `writeln!` and not `eprintln!`, which panics if stderr is closed — `mix status 2>&-` in a
    // pipeline that has already gone away is not worth a panic message about a panic.
    let _ = writeln!(stderr, "{rendered}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T182e. The uninstaller reads `mix` through `nsExec`, which decodes in the ANSI code page, so
    /// under `MIXENGINE_PLAIN_TEXT` the typographic characters become their ASCII look-alikes — the
    /// firewall row read `MixEngine â€” shared sites` in the uninstaller's log.
    #[test]
    fn plain_text_spells_typography_in_ascii() {
        assert_eq!(
            plain_text("MixEngine — shared sites, 1–2, ‘a’ “b” …"),
            "MixEngine - shared sites, 1-2, 'a' \"b\" ..."
        );
    }

    /// And leaves everything else alone — a path with a non-English name is still that path.
    #[test]
    fn plain_text_leaves_other_characters_alone() {
        assert_eq!(plain_text(r"C:\Users\Nguyễn\x"), r"C:\Users\Nguyễn\x");
    }
}
