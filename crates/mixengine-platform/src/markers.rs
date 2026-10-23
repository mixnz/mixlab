//! The block a system file lets MixEngine own, and nothing outside it.
//!
//! **T41 wrote this inside `hosts`, and T42 is what moved it.** `/etc/pf.conf` needs exactly the
//! same machinery with a different file in the error message and an insertion point that is not the
//! end of the file — see the T42 design, D10. The extraction is forced by the second user rather
//! than proposed on taste, which is the only kind of restructuring this project makes to code that
//! already works.
//!
//! **Nothing between the markers is anybody else's, and nothing outside them is ours.** That is the
//! platform layer's first rule and the promise a user never forgives being broken: a file somebody
//! has been editing since 2011 comes back exactly as it was. Both callers assert it by comparing
//! whole files rather than by looking for the lines they expected to keep.

use crate::{Error, Result};

/// The first line of the block. Nothing above it is ours.
pub const BEGIN_MARKER: &str = "# BEGIN MixEngine";

/// The last line of the block. Nothing below it is ours.
pub const END_MARKER: &str = "# END MixEngine";

/// Where a block that is not in the file yet is put.
#[derive(Debug, Clone, Copy)]
pub enum Insertion<'a> {
    /// After everything. What a hosts file wants: order carries no meaning in one.
    End,

    /// On the line after the last line that reads, trimmed, exactly this — and at the end when the
    /// file holds no such line.
    ///
    /// `/etc/pf.conf` is order-sensitive: every translation rule must precede every filter rule, so
    /// a `rdr-anchor` appended at the end of Apple's file is a file `pfctl` refuses to load.
    After(&'a str),
}

/// The managed block of one file.
///
/// Carries the file's name only so a refusal can say which of the files a person has to open. It
/// holds no path and reads nothing: every method takes the text.
///
/// **A tag names whose block it is** — ADR 0066. The hosts file holds one
/// block per home, `# BEGIN MixEngine <tag>`; the untagged block is the one every home shared
/// before. Each block matches only its own markers, so a line that is another block's marker is,
/// to this one, a line like any other.
#[derive(Debug, Clone, Copy)]
pub struct Block<'a> {
    what: &'static str,
    tag: Option<&'a str>,
}

/// Where a block sits: the marker lines included, and the lines between them.
struct Found {
    outer: std::ops::Range<usize>,
    inner: std::ops::Range<usize>,
}

impl Block<'static> {
    /// The untagged block in the file `what` names.
    #[must_use]
    pub const fn in_file(what: &'static str) -> Self {
        Self { what, tag: None }
    }
}

impl<'a> Block<'a> {
    /// The block tagged `tag` in the file `what` names.
    #[must_use]
    pub const fn tagged(what: &'static str, tag: &'a str) -> Self {
        Self {
            what,
            tag: Some(tag),
        }
    }

    /// This block's first line.
    fn begin(&self) -> String {
        match self.tag {
            Some(tag) => format!("{BEGIN_MARKER} {tag}"),
            None => BEGIN_MARKER.to_owned(),
        }
    }

    /// This block's last line.
    fn end(&self) -> String {
        match self.tag {
            Some(tag) => format!("{END_MARKER} {tag}"),
            None => END_MARKER.to_owned(),
        }
    }

    /// The lines between the markers, or [`None`] when the file holds no block of ours.
    ///
    /// An absent block is not an error: it is what a machine that has never run MixEngine looks
    /// like.
    ///
    /// # Errors
    ///
    /// [`Error::MalformedBlock`] for a block that cannot be read without guessing — see
    /// [`splice`](Self::splice).
    pub fn body<'t>(&self, text: &'t str) -> Result<Option<&'t str>> {
        Ok(self.find(text)?.map(|found| &text[found.inner]))
    }

    /// `text` with the managed block set to `body`.
    ///
    /// `body` is the lines that go between the markers, each already ending with the file's own line
    /// ending — [`newline`] is what says which that is. An empty `body` removes the block, markers
    /// and all, which is the reverse of every other call and the reason no backup is kept.
    ///
    /// # Errors
    ///
    /// [`Error::MalformedBlock`] for two `BEGIN` markers, a `BEGIN` with no `END`, or an `END` with
    /// no `BEGIN`. Repairing any of those means guessing at what somebody else meant.
    pub fn splice(&self, text: &str, body: &str, insertion: Insertion<'_>) -> Result<String> {
        let newline = newline(text);

        let rendered = if body.is_empty() {
            String::new()
        } else {
            format!("{}{newline}{body}{}{newline}", self.begin(), self.end())
        };

        let mut spliced = String::with_capacity(text.len() + rendered.len());

        match self.find(text)? {
            Some(found) => {
                spliced.push_str(&text[..found.outer.start]);
                spliced.push_str(&rendered);
                spliced.push_str(&text[found.outer.end..]);
            }
            None if rendered.is_empty() => spliced.push_str(text),
            None => {
                let at = match insertion {
                    Insertion::End => text.len(),
                    Insertion::After(line) => after(text, line).unwrap_or(text.len()),
                };

                spliced.push_str(&text[..at]);

                // The block starts on a line of its own. This is the only byte this engine ever
                // adds outside its own block, and the only reason a removal is not bit-for-bit
                // reversible.
                if at > 0 && !text[..at].ends_with('\n') {
                    spliced.push_str(newline);
                }

                spliced.push_str(&rendered);
                spliced.push_str(&text[at..]);
            }
        }

        Ok(spliced)
    }

    /// Where the block sits in `text`.
    ///
    /// A marker is matched against a **trimmed** line, exactly: leading whitespace and a CR are
    /// ignored, and `# BEGIN MixEngine (do not edit)` is somebody else's comment rather than ours.
    fn find(&self, text: &str) -> Result<Option<Found>> {
        let (begin_marker, end_marker) = (self.begin(), self.end());
        let (mut begin, mut end) = (None, None);
        let (mut inner_start, mut inner_end) = (0, 0);
        let mut offset = 0;

        for line in text.split_inclusive('\n') {
            let trimmed = line.trim();

            if trimmed == begin_marker {
                if begin.is_some() {
                    return Err(self.malformed(&format!("a second `{begin_marker}` marker")));
                }
                begin = Some(offset);
                inner_start = offset + line.len();
            } else if trimmed == end_marker {
                if begin.is_none() || end.is_some() {
                    return Err(self.malformed(&format!(
                        "an `{end_marker}` marker with no `{begin_marker}` above it"
                    )));
                }
                inner_end = offset;
                end = Some(offset + line.len());
            }

            offset += line.len();
        }

        match (begin, end) {
            (Some(start), Some(finish)) => Ok(Some(Found {
                outer: start..finish,
                inner: inner_start..inner_end,
            })),
            (Some(_), None) => Err(self.malformed(&format!(
                "a `{begin_marker}` marker with no `{end_marker}` below it"
            ))),
            // `end` is only ever set once `begin` is, so what remains is a file with no block.
            (None, _) => Ok(None),
        }
    }

    /// A block this code will not edit, and why.
    fn malformed(&self, reason: &str) -> Error {
        Error::MalformedBlock {
            reason: format!("MixEngine's block in {} has {reason}", self.what),
        }
    }
}

/// The line ending `text` already uses.
///
/// CRLF if the file uses it anywhere: rewriting a Windows hosts file with Unix endings is a diff on
/// every line of a file nobody asked us to reformat.
#[must_use]
pub fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The byte just past the last line that reads, trimmed, exactly `wanted`.
///
/// The *last*, not the first: a file naming the line twice is a file whose later occurrence is the
/// one everything below it is ordered against.
fn after(text: &str, wanted: &str) -> Option<usize> {
    let mut offset = 0;
    let mut found = None;

    for line in text.split_inclusive('\n') {
        if line.trim() == wanted {
            found = Some(offset + line.len());
        }
        offset += line.len();
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apple's own `/etc/pf.conf`, which is the file T42 splices into. Order-sensitive: every
    /// translation rule must precede every filter rule, so a block appended at the end of this file
    /// is a file `pfctl` refuses to load.
    const PF: &str = "\
#
# Default PF configuration file.
#
scrub-anchor \"com.apple/*\"
nat-anchor \"com.apple/*\"
rdr-anchor \"com.apple/*\"
dummynet-anchor \"com.apple/*\"
anchor \"com.apple/*\"
load anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\"
";

    const CONF: Block<'static> = Block::in_file("/etc/pf.conf");

    /// The reason this module exists: a block that has to land in the middle of a file, on the line
    /// after one particular line, rather than at the end.
    #[test]
    fn a_block_is_inserted_after_the_line_it_names() {
        let spliced = CONF
            .splice(
                PF,
                "rdr-anchor \"mixengine\"\n",
                Insertion::After("rdr-anchor \"com.apple/*\""),
            )
            .unwrap();

        let ours = spliced.find(BEGIN_MARKER).expect("the block was written");
        let apple = spliced
            .find("rdr-anchor \"com.apple/*\"")
            .expect("Apple's own line survived");
        let filter = spliced
            .find("\nanchor \"com.apple/*\"")
            .expect("the filter anchor survived");

        assert!(
            apple < ours,
            "the block went above Apple's translation anchor"
        );
        assert!(ours < filter, "the block went below a filter rule");
    }

    /// A file with no such line still gets the block, at the end. A machine whose `/etc/pf.conf`
    /// somebody has rewritten is not a machine to refuse.
    #[test]
    fn a_file_that_does_not_hold_the_named_line_takes_the_block_at_the_end() {
        let spliced = CONF
            .splice(
                "# mine\n",
                "rdr-anchor \"x\"\n",
                Insertion::After("nothing here"),
            )
            .unwrap();

        assert_eq!(
            spliced,
            "# mine\n# BEGIN MixEngine\nrdr-anchor \"x\"\n# END MixEngine\n"
        );
    }

    /// T41's criterion, applied to the second file: in, changed, out, and byte-identical.
    #[test]
    fn every_unrelated_line_survives_an_insertion_in_the_middle() {
        let at = Insertion::After("rdr-anchor \"com.apple/*\"");

        let first = CONF.splice(PF, "one\n", at).unwrap();
        let second = CONF.splice(&first, "two\n", at).unwrap();

        assert!(
            !second.contains("one"),
            "the block is replaced, not appended to: {second}"
        );
        assert_eq!(CONF.splice(&second, "", at).unwrap(), PF);
    }

    /// What the probe reads: the lines between the markers, exactly, and nothing when there is no
    /// block of ours.
    #[test]
    fn the_body_of_a_block_reads_back_as_it_was_written() {
        let spliced = CONF.splice(PF, "one\ntwo\n", Insertion::End).unwrap();

        assert_eq!(CONF.body(&spliced).unwrap(), Some("one\ntwo\n"));
        assert_eq!(CONF.body(PF).unwrap(), None);
    }

    /// The file's name reaches the message, which is the whole reason `Block` carries one: a person
    /// told only "MixEngine's block is malformed" has two files to open.
    #[test]
    fn a_malformed_block_names_the_file_it_is_in() {
        let two = format!("{BEGIN_MARKER}\n{BEGIN_MARKER}\n{END_MARKER}\n");

        let error = CONF.body(&two).unwrap_err();

        assert!(error.to_string().contains("/etc/pf.conf"), "{error}");
    }

    /// The legacy block beside one tagged block — the shape a hosts file has once homes keep their
    /// own (ADR 0066).
    const TWO: &str = "a\n# BEGIN MixEngine\nL\n# END MixEngine\n# BEGIN MixEngine ab12\nT\n# END MixEngine ab12\n";

    /// ADR 0066: each block reads only between its own markers, and a tag nobody wrote reads as
    /// no block at all.
    #[test]
    fn a_tagged_block_reads_only_its_own() {
        assert_eq!(Block::tagged("f", "ab12").body(TWO).unwrap(), Some("T\n"));
        assert_eq!(Block::in_file("f").body(TWO).unwrap(), Some("L\n"));
        assert_eq!(Block::tagged("f", "cd34").body(TWO).unwrap(), None);
    }

    /// Removing one home's block leaves every other byte of the file where it was.
    #[test]
    fn splicing_a_tagged_block_leaves_the_others_byte_for_byte() {
        let block = Block::tagged("f", "ab12");

        let changed = block.splice(TWO, "X\n", Insertion::End).unwrap();
        assert_eq!(changed, TWO.replace("\nT\n", "\nX\n"));

        assert_eq!(
            block.splice(TWO, "", Insertion::End).unwrap(),
            "a\n# BEGIN MixEngine\nL\n# END MixEngine\n"
        );
    }

    /// Two blocks under one tag are as unreadable as two untagged ones were.
    #[test]
    fn a_second_block_with_the_same_tag_is_malformed() {
        let twice = format!("{TWO}# BEGIN MixEngine ab12\nU\n# END MixEngine ab12\n");

        let error = Block::tagged("f", "ab12").body(&twice).unwrap_err();

        assert!(
            error.to_string().contains("# BEGIN MixEngine ab12"),
            "{error}"
        );
    }
}
