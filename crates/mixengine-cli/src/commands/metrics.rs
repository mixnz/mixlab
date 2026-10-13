//! `mix metrics`: one frame, a watch, or the history since a moment.

use std::process::ExitCode;
use std::time::SystemTime;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{Error, ErrorCode, MetricsFrame, MetricsHistory, ServiceId, Timestamp, rpc};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::output::{emit, rendered};
use crate::render;

/// A name if one was typed, and this directory if none was.
///
/// **Not a default this client invents**: the path is sent as it stands and the daemon does the
/// walking, which is the same answer the shim gets.
/// `mix site …` — ask, and render what came back.
///
/// **Nothing is decided here.** No domain is validated, no doc root is made relative and no kind is
/// defaulted: all of that is the daemon's, and a `mix` that could refuse what the GUI could not
/// would be the first bug `CLAUDE.md` names.
/// `mix metrics` — roadmap task **T71**.
///
/// Three readings of one namespace and one command, because they are three tenses of one question:
/// what is it costing now (`metrics.snapshot`), what has it been costing (`metrics.history`), and
/// what is it costing from here on (`GET /metrics`).
///
/// **`--watch` is written out as it arrives**, on `mix service logs --follow`'s reasoning: a stream
/// has no last message, and a buffer that filled until it ended would print nothing at all. It is
/// also the only thing in this repository that opens that route, which is what keeps the daemon's
/// one-second rate exercised rather than merely implemented.
pub(crate) async fn metrics(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    watch: bool,
    since: Option<&str>,
    service: Option<&ServiceId>,
    json: bool,
) -> Result<ExitCode, Error> {
    let subject = service.map(|id| format!("service:{id}"));
    let mut client = Client::connect(endpoint, autostart).await?;

    if watch {
        return watch_metrics(&mut client, json).await;
    }

    if let Some(since) = since {
        let history: MetricsHistory = ask(
            &mut client,
            rpc::method::METRICS_HISTORY,
            encode(&serde_json::json!({
                "subject": subject,
                "since": since_moment(since)?,
            })),
        )
        .await?;

        emit(&rendered(json, &history, || {
            render::metrics_history(&history, SystemTime::now())
        }))?;

        return Ok(ExitCode::SUCCESS);
    }

    let frame: MetricsFrame = ask(
        &mut client,
        rpc::method::METRICS_SNAPSHOT,
        encode(&serde_json::json!({})),
    )
    .await?;

    // Narrowed here rather than by the daemon: `metrics.snapshot` takes no parameters, because one
    // reading measures every subject anyway and a filter on the wire would be a second way of asking
    // for the same pass.
    let frame = match subject {
        None => frame,
        Some(wanted) => MetricsFrame {
            samples: frame
                .samples
                .into_iter()
                .filter(|sample| sample.subject.to_string() == wanted)
                .collect(),
            ..frame
        },
    };

    emit(&rendered(json, &frame, || render::metrics_frame(&frame)))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix metrics --watch`: print each reading as it arrives, until interrupted.
async fn watch_metrics(client: &mut Client, json: bool) -> Result<ExitCode, Error> {
    let mut stream = client.stream("/metrics").await?;

    while let Some(frame) = stream.next::<MetricsFrame>().await? {
        emit(&rendered(json, &frame, || {
            format!(
                "{}
",
                render::metrics_frame(&frame)
            )
        }))?;
    }

    Ok(ExitCode::SUCCESS)
}

/// How far back `--since 30m` reaches, as a moment this machine's clock names.
///
/// **Resolved here rather than sent as a duration**, because the API takes moments: a client that
/// sent "30m" would be asking the daemon to apply its own clock to a word, and the two clocks are
/// the same one — the endpoint is a local socket.
fn since_moment(value: &str) -> Result<Timestamp, Error> {
    let (count, unit) = value.split_at(
        value
            .find(|character: char| !character.is_ascii_digit())
            .ok_or_else(|| since_refusal(value))?,
    );

    let count: i64 = count.parse().map_err(|_| since_refusal(value))?;

    let millis = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return Err(since_refusal(value)),
    };

    let Timestamp(now) = Timestamp::from_system_time(SystemTime::now());

    Ok(Timestamp(
        now.saturating_sub(count.saturating_mul(millis).abs()),
    ))
}

/// What `--since` says when it cannot read what was typed.
fn since_refusal(value: &str) -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        format!("`--since {value}` is not a length of time"),
    )
    .with_hint("write it as a number and one of s, m, h, d, for example `--since 2h`")
}
