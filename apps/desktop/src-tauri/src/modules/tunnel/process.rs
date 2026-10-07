//! One tunnel's process — T203, D3 and D4: what is read from its output, and the record macOS needs
//! to end it after a crash. Pure, so each rule is a test rather than a run of cloudflared.

/// Something the row should suggest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Hint {
    /// `localhost` dialled `::1` and the server listens on `127.0.0.1` only.
    TryIpv4,
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

/// What to suggest for a line that reports a failure reaching the target.
pub fn hint_for(target: &str, line: &str) -> Option<Hint> {
    let refused = line.contains("connection refused") && line.contains("[::1]");
    (refused && target.contains("://localhost")).then_some(Hint::TryIpv4)
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

    #[test]
    fn a_refused_connection_to_localhost_suggests_127_0_0_1() {
        let line = "ERR  error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:5173: connect: connection refused\"";
        assert_eq!(hint_for("http://localhost:5173", line), Some(Hint::TryIpv4));
        assert_eq!(hint_for("http://127.0.0.1:5173", line), None);
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

    #[test]
    fn vite_s_refusal_of_an_unknown_host_is_recognised() {
        let body =
            "Blocked request. This host (\"quiet-river-1234.trycloudflare.com\") is not allowed.";
        assert!(blocks_the_host(403, body));
        assert!(!blocks_the_host(200, body));
        assert!(!blocks_the_host(403, "Forbidden"));
    }
}
