//! Splits Server-Sent Events frames.
//!
//! Four rules, and no more: a line starting with `:` is a comment (an idle stream sends one every
//! 15 seconds — that is what tells a live connection from a dead one); `data:` lines are gathered
//! and joined with `\n`; an empty line closes a message; any other line is ignored.
//!
//! MixEngine's events are **internally tagged** — there is no `event:` line to read; the
//! discriminator lives inside the JSON itself. So there is no notion of "event type" here: it only
//! carries the payload out, and understanding the payload is the job of the layer above.

/// Gathers bytes into messages. One per connection.
#[derive(Default)]
pub struct Frames {
    buffer: String,
}

impl Frames {
    pub fn new() -> Self {
        Self::default()
    }

    /// Swallows a chunk that just arrived, returns each completed payload, in order.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();

        loop {
            // An empty line closes a message, and it arrives in one of two shapes. Take whichever
            // appears first: a `\r\n\r\n` also contains a `\n\n` one byte off, so searching for
            // `\n\n` first and only then deciding the width is wrong.
            let crlf = self.buffer.find("\r\n\r\n");
            let lf = self.buffer.find("\n\n");
            let (at, width) = match (crlf, lf) {
                (Some(crlf_at), Some(lf_at)) if crlf_at <= lf_at => (crlf_at, 4),
                (_, Some(lf_at)) => (lf_at, 2),
                (Some(crlf_at), None) => (crlf_at, 4),
                (None, None) => break,
            };

            let block: String = self.buffer.drain(..at + width).collect();
            let mut data: Vec<&str> = Vec::new();
            for line in block.lines() {
                let line = line.trim_end_matches('\r');
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("data:") {
                    data.push(rest.strip_prefix(' ').unwrap_or(rest));
                }
            }
            if !data.is_empty() {
                out.push(data.join("\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One message, cleanly.
    #[test]
    fn one_message_comes_out_whole() {
        let mut frames = Frames::new();
        assert_eq!(
            frames.push("data: {\"type\":\"resync\",\"missed\":3}\n\n"),
            vec!["{\"type\":\"resync\",\"missed\":3}"]
        );
    }

    /// An idle stream sends a `:` comment every 15 seconds — that is what tells a live connection
    /// from a dead one, and it is not a message.
    #[test]
    fn a_comment_is_not_a_message() {
        let mut frames = Frames::new();
        assert!(frames.push(": keepalive\n\n").is_empty());
    }

    /// Several `data:` lines of the same message are joined with `\n`, exactly as the SSE spec
    /// says.
    #[test]
    fn several_data_lines_join() {
        let mut frames = Frames::new();
        assert_eq!(frames.push("data: a\ndata: b\n\n"), vec!["a\nb"]);
    }

    /// TCP may cut anywhere, even in the middle of a line. Nothing comes out until the empty line
    /// arrives.
    #[test]
    fn a_message_split_across_chunks_waits_for_its_blank_line() {
        let mut frames = Frames::new();
        assert!(frames.push("data: {\"type\":\"job_pro").is_empty());
        assert!(frames.push("gress\"}").is_empty());
        assert_eq!(frames.push("\n\n"), vec!["{\"type\":\"job_progress\"}"]);
    }

    /// Two messages in one chunk both come out, in order.
    #[test]
    fn two_messages_in_one_chunk_both_come_out() {
        let mut frames = Frames::new();
        assert_eq!(frames.push("data: a\n\ndata: b\n\n"), vec!["a", "b"]);
    }

    /// `\r\n` has to be swallowed too: this is HTTP.
    #[test]
    fn carriage_returns_do_not_end_up_in_the_payload() {
        let mut frames = Frames::new();
        assert_eq!(frames.push("data: a\r\n\r\n"), vec!["a"]);
    }
}
