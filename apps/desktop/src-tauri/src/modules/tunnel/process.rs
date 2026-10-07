//! One tunnel's process — T203, D3 and D4: what is read from its output, and the record macOS needs
//! to end it after a crash. Pure, so each rule is a test rather than a run of cloudflared.

/// Something the row should suggest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Hint {
    /// A request reached the tunnel and nothing answered at the target: the server is not running,
    /// or listens on another port.
    NothingListening,
    /// The dev server refused the tunnel's host — Vite's `server.allowedHosts`.
    AllowedHosts,
}

/// The `https://….trycloudflare.com` address in one line of cloudflared's output.
pub fn public_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '/')))
        .unwrap_or(rest.len());
    let candidate = rest[..end].trim_end_matches('/');
    candidate
        .ends_with(".trycloudflare.com")
        .then(|| candidate.to_owned())
}

/// What to suggest for one line of cloudflared's output. A refused connection to the target reads
/// the same for `localhost` and `127.0.0.1`: cloudflared tries both addresses of `localhost` itself
/// (measured on 2026.10.0), so the only useful thing to say is that nothing answered.
pub fn hint_for(line: &str) -> Option<Hint> {
    line.contains("Unable to reach the origin service")
        .then_some(Hint::NothingListening)
}

/// Whether a response is a dev server refusing the tunnel's host — Vite 5 and later.
pub fn blocks_the_host(status: u16, body: &str) -> bool {
    status == 403 && body.contains("Blocked request") && body.contains("is not allowed")
}

/// A running tunnel as macOS has to remember it: its process group and when it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub pid: u32,
    pub started: i64,
}

impl Record {
    /// Whether the process with this pid is still the one that was recorded.
    pub fn still_ours(&self, started_at: impl Fn(u32) -> Option<i64>) -> bool {
        started_at(self.pid) == Some(self.started)
    }

    /// The file's text: one `pid start` per line.
    pub fn render_all(records: &[Record]) -> String {
        records
            .iter()
            .map(|r| {
                format!(
                    "{} {}
",
                    r.pid, r.started
                )
            })
            .collect()
    }

    /// The records without the one for `pid`.
    pub fn without(records: &[Record], pid: u32) -> Vec<Record> {
        records
            .iter()
            .copied()
            .filter(|record| record.pid != pid)
            .collect()
    }

    /// Every line that reads as a record; anything else is skipped.
    pub fn parse_all(text: &str) -> Vec<Record> {
        text.lines()
            .filter_map(|line| {
                let (pid, started) = line.split_once(' ')?;
                Some(Record {
                    pid: pid.parse().ok()?,
                    started: started.parse().ok()?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_url_is_found_in_cloudflared_s_box() {
        let line = "2026-10-08T09:00:00Z INF |  https://quiet-river-1234.trycloudflare.com                      |";
        assert_eq!(
            public_url(line).as_deref(),
            Some("https://quiet-river-1234.trycloudflare.com")
        );
    }

    #[test]
    fn a_line_without_one_answers_none() {
        assert_eq!(
            public_url("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
        assert_eq!(public_url("see https://developers.cloudflare.com/x"), None);
    }

    /// The line cloudflared 2026.10.0 printed, measured, when a request reached the tunnel and
    /// nothing listened on the target — for `localhost` and `127.0.0.1` alike, since cloudflared
    /// tries both addresses of `localhost` itself.
    #[test]
    fn nothing_listening_at_the_target_is_named() {
        let line = "2026-10-07T19:12:15Z ERR  error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp 127.0.0.1:8097: connect: connection refused\" connIndex=0 event=1 ingressRule=0 originService=http://localhost:8097";
        assert_eq!(hint_for(line), Some(Hint::NothingListening));
        assert_eq!(
            hint_for("2026-10-07T19:10:51Z ERR Connection terminated connIndex=0"),
            None
        );
    }

    #[test]
    fn a_recorded_pid_whose_start_time_differs_is_left_alone() {
        let record = Record {
            pid: std::process::id(),
            started: 1,
        };
        assert!(!record.still_ours(|_| Some(2)));
        assert!(record.still_ours(|_| Some(1)));
        assert!(!record.still_ours(|_| None));
    }

    #[test]
    fn records_round_trip_through_their_file_format() {
        let records = vec![
            Record {
                pid: 41,
                started: 7,
            },
            Record {
                pid: 42,
                started: 9,
            },
        ];
        assert_eq!(Record::parse_all(&Record::render_all(&records)), records);
        assert!(Record::parse_all("garbage\n43\n").is_empty());
    }

    /// A stopped tunnel leaves the file, so the file only ever names what still runs — found on macOS,
    /// where it kept every pid ever started until the next launch.
    #[test]
    fn a_stopped_tunnel_leaves_the_records() {
        let records = vec![
            Record {
                pid: 41,
                started: 7,
            },
            Record {
                pid: 42,
                started: 9,
            },
        ];
        assert_eq!(
            Record::without(&records, 41),
            vec![Record {
                pid: 42,
                started: 9
            }]
        );
        assert_eq!(Record::without(&records, 99), records);
    }

    #[test]
    fn vite_s_refusal_of_an_unknown_host_is_recognised() {
        let body =
            "Blocked request. This host (\"quiet-river-1234.trycloudflare.com\") is not allowed.";
        assert!(blocks_the_host(403, body));
        assert!(!blocks_the_host(200, body));
        assert!(!blocks_the_host(403, "Forbidden"));
    }
}
