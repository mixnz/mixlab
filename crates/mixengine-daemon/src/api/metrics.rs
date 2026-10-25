//! `GET /metrics` — what everything is costing, a frame per tick, while somebody is watching.
//!
//! **Opening this connection is the subscription and closing it is the end of it.** There is no
//! `metrics.subscribe` method, for [ADR
//! 0009](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0009-logs-travel-on-their-own-stream.md)'s
//! reason and for one more of its own: a subscription ended by a second call would leave a client
//! that crashed sampling this machine every second for as long as the daemon ran. A socket closing
//! cannot be forgotten.
//!
//! **The rate is a consequence of this route, not a setting.** The watch the stream holds is what
//! puts the sampler on its one-second period; it lives in the stream's own state, so it is dropped
//! exactly when the body ends — whether the client closed the window, lost the socket, or the daemon
//! is shutting down.
//!
//! **A reader that falls behind is given the newest frame rather than a resync.** Eight frames of
//! back-pressure, where the event bus holds 1024: for a metric the old value is worth nothing and
//! the next one is a second away, so there is nothing to replay and nothing to ask the client to
//! re-fetch.
//!
//! **`metrics.snapshot` and `metrics.history` are here too** — the two methods a client asks once
//! rather than streams.

use std::convert::Infallible;
use std::sync::Arc;

use http_body_util::{BodyExt as _, StreamBody};
use hyper::Response;
use hyper::body::{Bytes, Frame as BodyFrame};
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderValue};
use mixengine_proto::MetricsFrame;
use tokio::sync::broadcast;

use super::Api;
use super::http::ResponseBody;
use crate::error::ToWire as _;
use mixengine_proto::{Error, ErrorCode, MetricsHistory, MetricsHistoryQuery};

/// One frame, as an SSE `data:` line.
///
/// The same shape `GET /events` writes and for the same reason: one `data:` line holding a JSON
/// object, so a client needs one handler rather than one per event name.
fn encode(frame: &MetricsFrame) -> Bytes {
    let json = serde_json::to_string(frame).expect("a proto type always serialises");

    Bytes::from(format!("data: {json}\n\n"))
}

/// Serve the live readings until the client goes away.
pub(super) fn stream(api: &Arc<Api>) -> Response<ResponseBody> {
    let (watch, frames) = api.metrics().stream();
    let shutdown = api.shutdown().token().clone();

    // An unfolding stream rather than a task writing into a queue, exactly as `/events` has it:
    // hyper polls this as fast as the client reads it, so a client that stops reading slows its own
    // stream and nobody else's.
    let body = futures_util::stream::unfold(
        (watch, frames, shutdown),
        |(watch, mut frames, shutdown)| async move {
            let frame = loop {
                let received = tokio::select! {
                    // Ending the body is the honest thing to tell a client whose daemon is going
                    // away, and it is what drops the `Watch`.
                    () = shutdown.cancelled() => return None,
                    received = frames.recv() => received,
                };

                match received {
                    Ok(frame) => break frame,

                    // Behind by more than the channel holds. The next frame is a second away and
                    // the ones it missed describe a moment that has passed, so it simply gets the
                    // newest — there is nothing about a metric worth replaying.
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::debug!(missed, "a metrics reader fell behind");
                    }

                    // The sampler is gone, which can only mean the daemon is on its way out.
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            };

            let data = BodyFrame::data(encode(&frame));

            Some((Ok::<_, Infallible>(data), (watch, frames, shutdown)))
        },
    );

    let mut response = Response::new(StreamBody::new(body).boxed());

    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    // A cached metrics stream is a client that never sees a new number again.
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));

    response
}

impl Api {
    /// `metrics.snapshot` — every subject's reading, taken now.
    ///
    /// **A reading rather than the last one taken**, which is what the method's own documentation
    /// promises: between clients this daemon samples once a minute, so the cached tick would answer
    /// a person with a number up to a minute old and would not mention a service that started ten
    /// seconds ago. Reuse of a reading younger than a second happens inside the loop, which is the
    /// only thing that measures.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Internal`] when the sampling loop is gone, which happens only while this daemon
    /// is shutting down — and is worth saying rather than answering with an empty frame that would
    /// read as a machine using nothing.
    pub(super) async fn metrics_snapshot(&self) -> Result<MetricsFrame, Error> {
        self.metrics.snapshot().await.ok_or_else(|| {
            Error::new(
                ErrorCode::Internal,
                "this daemon is no longer measuring anything",
            )
            .with_hint("it is shutting down; ask the one that starts next")
        })
    }

    /// `metrics.history` — the 24-hour history, one row per subject per minute.
    ///
    /// # Errors
    ///
    /// Whatever [`mixengine_core::metrics::history`] reports when the table cannot be read.
    pub(super) async fn metrics_history(
        &self,
        query: &MetricsHistoryQuery,
    ) -> Result<MetricsHistory, Error> {
        // The machine's logical processors ride along, so a client draws the rows as a
        // share of the machine without asking a second question.
        let cores = self.elevation.host().resource_control().support().cores;

        mixengine_core::metrics::history(&self.store, query, self.metrics.retention_hours(), cores)
            .await
            .map_err(|error| error.to_wire())
    }
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{MetricsSample, MetricsSubject, Timestamp};

    use super::*;
    use crate::api::test_daemon::call;

    #[test]
    fn a_frame_goes_out_as_one_sse_data_line() {
        let encoded = encode(&MetricsFrame {
            at: Timestamp(60_000),
            samples: vec![MetricsSample {
                subject: MetricsSubject::Daemon,
                cpu_percent: None,
                rss_bytes: 1,
                processes: 1,
            }],
            cores: 1,
        });

        let text = String::from_utf8(encoded.to_vec()).expect("utf-8");

        assert!(text.starts_with("data: {"));
        assert!(text.ends_with("\n\n"), "one blank line ends an SSE frame");
        assert_eq!(text.matches("data: ").count(), 1);
    }

    #[tokio::test]
    async fn a_history_read_answers_with_what_this_home_keeps() {
        let answer = call(r#"{"jsonrpc":"2.0","method":"metrics.history","id":1}"#).await;

        assert_eq!(answer["result"]["retention_hours"], 24);
        assert_eq!(
            answer["result"]["minutes"].as_array().map(Vec::len),
            Some(0),
            "a home that has measured nothing has no rows, which is an answer and not a failure"
        );
    }

    #[tokio::test]
    async fn a_history_read_refuses_a_subject_it_cannot_read() {
        let answer = call(
            r#"{"jsonrpc":"2.0","method":"metrics.history","params":{"subject":"nonsense"},"id":1}"#,
        )
        .await;

        assert_eq!(answer["error"]["code"], -32602);
        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn a_snapshot_with_no_loop_behind_it_says_so_rather_than_waiting() {
        // The fixture builds a sampler and keeps only its handle, which is the shape a daemon has
        // while it is shutting down. The answer has to be an error: an empty frame would read as a
        // machine using nothing.
        let answer = call(r#"{"jsonrpc":"2.0","method":"metrics.snapshot","id":1}"#).await;

        assert_eq!(answer["error"]["data"]["code"], "internal");
        assert!(
            answer["error"]["data"]["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("shutting down"))
        );
    }
}
