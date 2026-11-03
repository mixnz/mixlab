//! One tunnel's process: what is read from its output, and the record macOS needs to end it after a
//! crash. Pure, so each rule is a test rather than a run of cloudflared.

/// Something the row should suggest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Hint {
    /// A request reached the tunnel and nothing answered at the target: the server is not running,
    /// or listens on another port.
    NothingListening,
    /// The dev server refused the tunnel's host — Vite's `server.allowedHosts`.
    AllowedHosts,
    /// ngrok found no authtoken in its config or its environment (`ERR_NGROK_4018`).
    NeedsAuthtoken,
}

/// What one line of ngrok's JSON log means to the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NgrokLine {
    /// The tunnel opened at this `https://` address.
    Url(String),
    /// Something the row should suggest.
    Hint(Hint),
    /// Anything else, as the failure tail shows it: the line's error, else its message, else the
    /// line itself.
    Other(String),
}

/// Reads one line of `ngrok … --log stdout --log-format json`. The URL is taken from the
/// `started tunnel` line whatever its domain: the assigned one differs by plan and has changed over
/// time.
pub fn read_ngrok(line: &str) -> NgrokLine {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return NgrokLine::Other(line.to_owned());
    };
    let field = |name: &str| value.get(name).and_then(serde_json::Value::as_str);
    let msg = field("msg").unwrap_or_default();
    let err = field("err").unwrap_or_default();
    if msg == "started tunnel" {
        if let Some(url) = field("url").filter(|url| url.starts_with("https://")) {
            return NgrokLine::Url(url.to_owned());
        }
    }
    if err.contains("ERR_NGROK_4018") {
        return NgrokLine::Hint(Hint::NeedsAuthtoken);
    }
    let refused = err.contains("dial tcp")
        && (err.contains("refused") || err.contains("No connection could be made"));
    if err.contains("ERR_NGROK_8012") || refused {
        return NgrokLine::Hint(Hint::NothingListening);
    }
    let text = if err.is_empty() { msg } else { err };
    if text.is_empty() {
        return NgrokLine::Other(line.to_owned());
    }
    NgrokLine::Other(
        text.lines()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    )
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

    /// ngrok's line for an opened tunnel, in the shape its JSON log documents.
    const NGROK_STARTED: &str = r#"{"addr":"http://localhost:5173","lvl":"info","msg":"started tunnel","name":"command_line","obj":"tunnels","t":"2026-11-02T20:00:00+0700","url":"https://quiet-river-1234.ngrok-free.app"}"#;

    /// What ngrok 3.39.13 printed, measured, with no authtoken anywhere.
    const NGROK_NO_AUTHTOKEN: &str = r#"{"t":"2026-11-02T20:01:44+0700","lvl":"crit","msg":"command failed","err":"authentication failed: This ngrok session is not authenticated. ngrok requires an account and a valid credential to start a session.\n\nSign up for an account: https://dashboard.ngrok.com/signup\nGet your credential: https://dashboard.ngrok.com/get-started/your-authtoken\r\n\r\nERR_NGROK_4018\r\n"}"#;

    /// A request through the tunnel with nothing listening at the target.
    const NGROK_UPSTREAM_REFUSED: &str = r#"{"t":"2026-11-02T20:05:00+0700","lvl":"warn","msg":"failed to open private leg","obj":"tunnels.session","id":"1","privaddr":"localhost:59999","err":"dial tcp [::1]:59999: connectex: No connection could be made because the target machine actively refused it."}"#;

    /// A second agent on a free account, refused by its session limit.
    const NGROK_SECOND_AGENT: &str = r#"{"t":"2026-11-02T20:06:00+0700","lvl":"crit","msg":"command failed","err":"authentication failed: Your account is limited to 1 simultaneous ngrok agent sessions.\r\n\r\nERR_NGROK_108\r\n"}"#;

    #[test]
    fn ngrok_s_started_tunnel_line_gives_its_url() {
        assert_eq!(
            read_ngrok(NGROK_STARTED),
            NgrokLine::Url("https://quiet-river-1234.ngrok-free.app".into())
        );
    }

    #[test]
    fn a_url_on_any_domain_is_taken_but_only_https() {
        assert_eq!(
            read_ngrok(r#"{"msg":"started tunnel","url":"https://a.example.dev"}"#),
            NgrokLine::Url("https://a.example.dev".into())
        );
        assert!(matches!(
            read_ngrok(r#"{"msg":"started tunnel","url":"http://a.example.dev"}"#),
            NgrokLine::Other(_)
        ));
    }

    #[test]
    fn no_authtoken_is_named() {
        assert_eq!(
            read_ngrok(NGROK_NO_AUTHTOKEN),
            NgrokLine::Hint(Hint::NeedsAuthtoken)
        );
    }

    #[test]
    fn an_unreachable_upstream_is_named() {
        assert_eq!(
            read_ngrok(NGROK_UPSTREAM_REFUSED),
            NgrokLine::Hint(Hint::NothingListening)
        );
    }

    #[test]
    fn a_refused_second_agent_shows_ngrok_s_own_error_on_one_line() {
        let NgrokLine::Other(text) = read_ngrok(NGROK_SECOND_AGENT) else {
            panic!("not Other");
        };
        assert!(text.contains("ERR_NGROK_108"), "{text}");
        assert!(!text.contains('\n') && !text.contains('\r'), "{text}");
    }

    #[test]
    fn a_line_with_no_error_shows_its_message() {
        assert_eq!(
            read_ngrok(r#"{"lvl":"info","msg":"open config file","err":null}"#),
            NgrokLine::Other("open config file".into())
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_kept_as_it_is() {
        assert_eq!(
            read_ngrok("t=x lvl=warn msg=\"update available\""),
            NgrokLine::Other("t=x lvl=warn msg=\"update available\"".into())
        );
    }

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
