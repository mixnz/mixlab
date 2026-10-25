//! Turning a request body into an answer: batches, notifications, dispatch, and panic containment.
//!
//! The whole of `POST /rpc` is here, and it is deliberately separate from [`super::http`]: this
//! module never sees a header, a status code or a socket, so everything it does can be tested by
//! handing it a slice of bytes.

#[cfg(test)]
mod arms;

mod table;

use std::future::Future;
use std::sync::Arc;

use futures_util::FutureExt as _;
use mixengine_proto::rpc::{Id, Request, Response, RpcCode, RpcError};
use mixengine_proto::{Error, ErrorCode};
use serde_json::Value;
use tracing::Instrument as _;

use super::Api;
use crate::background::Background;

/// Answer a `POST /rpc` body.
///
/// `None` means *write nothing back*: the body held only notifications, and the spec is explicit
/// that a server returns nothing for those. The caller turns that into `204 No Content` rather than
/// into an empty `200`, because an empty body is not valid JSON and a client that parses every
/// response would choke on one.
pub(super) async fn answer(api: &Arc<Api>, body: &[u8]) -> Option<Vec<u8>> {
    let payload: Value = match serde_json::from_slice(body) {
        Ok(payload) => payload,

        // The one failure where the id is genuinely unknowable, and the only place a `null` id is
        // correct. Note it is still an HTTP 200: the transport worked, the call did not.
        Err(error) => {
            let response = Response::failure(
                None,
                RpcError::at(
                    RpcCode::PARSE_ERROR,
                    Error::new(
                        ErrorCode::InvalidArgument,
                        format!("the request body is not JSON: {error}"),
                    )
                    .with_hint(
                        "`POST /rpc` takes one JSON-RPC 2.0 request object, or an array of them",
                    ),
                ),
            );

            return Some(encode(&response));
        }
    };

    match payload {
        Value::Array(calls) => {
            // An empty batch is the one array the spec singles out: there is nothing to answer, so
            // a server that returned `[]` would be answering a request it never received.
            if calls.is_empty() {
                let response = Response::failure(
                    None,
                    RpcError::at(
                        RpcCode::INVALID_REQUEST,
                        Error::new(ErrorCode::InvalidArgument, "the batch is empty"),
                    ),
                );

                return Some(encode(&response));
            }

            let mut answers = Vec::with_capacity(calls.len());

            // Sequential rather than concurrent. Running a batch in parallel would be free today —
            // no handler touches shared state — and would stop being free the moment one does,
            // silently, in whichever phase adds it. The spec promises nothing about order, so the
            // conservative reading costs a client nothing.
            for call in calls {
                if let Some(answer) = dispatch(api, call).await {
                    answers.push(answer);
                }
            }

            (!answers.is_empty()).then(|| encode(&answers))
        }

        single => dispatch(api, single).await.as_ref().map(encode),
    }
}

/// One call out of a body, answered — or not, if it was a notification.
async fn dispatch(api: &Arc<Api>, call: Value) -> Option<Response> {
    // Read out before the call is decoded, because a request that fails to decode still has to be
    // answered *to the id it claimed*: a client matching answers to calls has no other way to know
    // which of its five requests was the malformed one.
    let claimed = call.get("id");

    // A notification is a request with no `id` **member**, which is not the same thing as one whose
    // id is `null`: the spec discourages the latter but nowhere lets it mean "answer nothing", so a
    // client that sends one and waits would wait forever. The two are indistinguishable after
    // decoding — `Option<Id>` reads an absent member and a null one alike — so the distinction is
    // taken here, from the JSON, and never from `Request::is_notification`.
    let wants_an_answer = claimed.is_some();
    let id = claimed
        .cloned()
        .and_then(|id| serde_json::from_value::<Id>(id).ok());

    let request: Request = match serde_json::from_value(call) {
        Ok(request) => request,

        Err(error) => {
            return Some(Response::failure(
                id,
                RpcError::at(
                    RpcCode::INVALID_REQUEST,
                    Error::new(
                        ErrorCode::InvalidArgument,
                        format!("not a JSON-RPC request: {error}"),
                    )
                    .with_hint(
                        "a request is `{\"jsonrpc\":\"2.0\",\"method\":\"…\",\"id\":1}`, with \
                         `params` where the method takes them",
                    ),
                ),
            ));
        }
    };

    // The id is rendered into the span up front rather than recorded onto an `Empty` field
    // afterwards: `tracing-subscriber` formats a span's fields once and *appends* what is recorded
    // later, so the second way prints `id=3 id=3`. Each of the three cases is a value and not a
    // missing field — "this call wanted no answer" is worth being able to see in the log, and so is
    // the odd client that asked to be answered to `null`.
    let called = match (&id, wants_an_answer) {
        (Some(id), _) => id.to_string(),
        (None, true) => "null".to_owned(),
        (None, false) => "-".to_owned(),
    };
    let span = tracing::info_span!("rpc", method = %request.method, id = %called);

    let outcome = call_method(Arc::clone(api), request.method, request.params)
        .instrument(span)
        .await;

    // A notification is answered by silence even when it failed. The client asked not to be told,
    // and the daemon's own log is where that failure goes — which is why `call_method` logs it
    // rather than leaving it to whoever builds the response.
    if !wants_an_answer {
        return None;
    }

    // `id` is `None` only for the client that spelled its own id `null`, and that is what it gets
    // back: an answer echoes the id it was given rather than improving on it.
    Some(match outcome {
        Ok(result) => Response::success(id, result),
        Err(failure) => Response::failure(id, failure.into_rpc()),
    })
}

/// Run one method, containing anything it does to itself.
///
/// See [`contained`], which is the whole of it; this only names the work.
async fn call_method(
    api: Arc<Api>,
    method: String,
    params: Option<Value>,
) -> Result<Value, Failure> {
    let background = api.shutdown().background().clone();
    let named = method.clone();

    contained(
        &background,
        &named,
        async move { table::route(api, &method, params).await }
            // The span belongs to the request, and the task the work runs in is not the task the span
            // was opened on, so it is attached explicitly rather than inherited.
            .in_current_span(),
    )
    .await
}

/// Run `work` in the daemon's `Background`, and turn whatever it does to itself into an answer.
///
/// **A task of its own, so that a panic becomes a value.** A handler that panics must not take the
/// daemon with it — `docs/standards/rust.md` is explicit that a panic here kills every managed
/// service — and the connection alone is not enough to sacrifice either: the client would see a
/// dropped socket and have no idea whether its request had been carried out. `panic = "abort"` in
/// the release profile would defeat this, which is why the workspace manifest says so out loud.
///
/// **In `Background` rather than a bare `tokio::spawn`**, because the work outlives the
/// connection waiting on it: a connection still open after the client grace is dropped, and a
/// detached handler ran on past it into `Store::close`. In `Background` the shutdown waits for it
/// beside the clients and stops what is left. A `JoinSet` hands no result back to the caller, so
/// the panic is caught inside the task with `catch_unwind` and the outcome sent over a oneshot.
async fn contained<F>(background: &Background, method: &str, work: F) -> Result<Value, Failure>
where
    F: Future<Output = Result<Value, Failure>> + Send + 'static,
{
    let (answer, answered) = tokio::sync::oneshot::channel();

    background.spawn("rpc handler", async move {
        // `AssertUnwindSafe` because nothing observes the work's state after a panic: the future is
        // dropped here, and the only thing that leaves this task is the outcome.
        let outcome = std::panic::AssertUnwindSafe(work).catch_unwind().await;
        let _ = answer.send(outcome);
    });

    let stopped = match answered.await {
        Ok(Ok(Ok(result))) => {
            tracing::debug!("answered");
            return Ok(result);
        }

        Ok(Ok(Err(failure))) => {
            // `warn` and not `error`: the caller has been told, in a message written for them, and
            // most of what lands here is a request that was simply wrong. The line exists so the
            // daemon's log can answer "what was this client doing" later.
            tracing::warn!(code = %failure.error.code, error = %failure.error.message, "refused");
            return Err(failure);
        }

        // The panic message itself has already gone to the log through the panic hook — which is
        // `crate::crash`'s, installed at `main`, and which also left a report in `logs/crashes/`.
        // It is not repeated to the client — a backtrace is not something a user can act on, and
        // the daemon's log is where it belongs.
        Ok(Err(_panic)) => false,

        // No answer at all: the task was stopped by the shutdown, or was never started because the
        // shutdown had already closed the set. Answered all the same, so nothing waits on it.
        Err(_) => true,
    };

    tracing::error!(
        panicked = !stopped,
        stopped,
        "a request handler did not finish; answering `internal` and staying up"
    );

    Err(Failure {
        code: RpcCode::INTERNAL_ERROR,
        error: Error::new(
            ErrorCode::Internal,
            format!("`{method}` failed in a way it does not account for"),
        )
        .with_hint("this is a bug in MixEngine; `logs/daemon.log` has the detail a report needs"),
    })
}

/// Run a handler's blocking half where blocking work belongs.
///
/// `docs/standards/rust.md` keeps anything that waits on a disk off the runtime threads that
/// have connections to serve, and `path.*` is the first *method* here with such a half: nineteen
/// file copies, a directory walk, and on Windows a registry write that broadcasts to every window
/// on the desktop. Every other handler answers from memory or through `sqlx`, which does its own.
///
/// **A panic inside is re-raised rather than turned into an error**, so that the containment in
/// [`call_method`] is the one place a panicking handler is described — a second rendering of the
/// same accident here would be a second sentence for the same bug, differing only in which thread
/// it happened on.
pub(crate) async fn on_a_blocking_thread<T, F>(work: F) -> Result<T, Error>
where
    F: FnOnce() -> Result<T, Error> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(answer) => answer,
        Err(join) => match join.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            // Cancellation, which only happens when the runtime is going down — and a blocking task
            // is not cancellable, so this is unreachable while the daemon is the only thing
            // spawning them. Reported as itself rather than unwrapped, because nothing here panics.
            Err(_) => Err(Error::new(
                ErrorCode::Internal,
                "the work behind this call did not finish".to_owned(),
            )),
        },
    }
}

/// A method that takes no arguments, checking that it was called that way.
///
/// Lenient about how "nothing" is spelled: absent, `null`, `[]` and `{}` all mean the same thing,
/// and clients differ on which they send. Anything else is refused, because a client that passed
/// arguments believes they did something.
fn no_params(params: Option<&Value>) -> Result<(), Failure> {
    let empty = match params {
        None | Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(fields)) => fields.is_empty(),
        Some(_) => false,
    };

    if empty {
        Ok(())
    } else {
        Err(Failure {
            code: RpcCode::INVALID_PARAMS,
            error: Error::new(
                ErrorCode::InvalidArgument,
                "this method takes no parameters",
            ),
        })
    }
}

/// A method's arguments, decoded into the shape that method documents.
///
/// Lenient about "nothing" in the same three spellings [`no_params`] accepts, because a method whose
/// every parameter has a default — `service.start` with no service named is every service — should
/// answer a client that sent `{}`, `null` or nothing at all identically. A type with a required
/// field still refuses all three, which is the point: `service.status` with no subject is a
/// `service.list` that was typed wrongly, and reporting it is better than answering it.
fn arguments<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, Failure> {
    let given = match params {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(Value::Array(items)) if items.is_empty() => Value::Object(serde_json::Map::new()),
        Some(value) => value,
    };

    serde_json::from_value(given).map_err(|error| Failure {
        code: RpcCode::INVALID_PARAMS,
        error: Error::new(
            ErrorCode::InvalidArgument,
            format!("these are not this method's parameters: {error}"),
        ),
    })
}

/// A failure MixEngine itself produced: the method ran, and the work did not.
///
/// Everything that reaches this has already been through [`crate::error::ToWire`], where the code
/// and the hint are chosen; all that is added here is the JSON-RPC integer that says the call got as
/// far as running.
fn refused(error: Error) -> Failure {
    Failure {
        code: RpcCode::APPLICATION_ERROR,
        error,
    }
}

/// A handler's return value, as JSON.
///
/// Serialising a type we defined can only fail on something like a map with non-string keys, which
/// no `mixengine-proto` type has — but it is a `Result` all the same rather than an `unwrap`, on
/// the same principle as the panic containment above: nothing in the RPC layer panics.
fn encode_result<T: serde::Serialize>(value: &T) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| Failure {
        code: RpcCode::INTERNAL_ERROR,
        error: Error::new(
            ErrorCode::Internal,
            format!("the answer could not be encoded: {error}"),
        ),
    })
}

/// A failed call, before it is either dropped (a notification) or written into a [`Response`].
///
/// Carries the JSON-RPC integer *and* the MixEngine error because the two are chosen at different
/// moments: the integer says at which stage the call stopped, the error says what a person should
/// do about it.
#[derive(Debug)]
struct Failure {
    code: RpcCode,
    error: Error,
}

impl Failure {
    fn into_rpc(self) -> RpcError {
        RpcError::at(self.code, self.error)
    }
}

/// Serialise an answer.
///
/// The failure case is unreachable — every type in it is one of ours — and is answered with a
/// hand-written JSON-RPC error rather than an `unwrap`, so that the invariant being wrong costs one
/// bad response instead of the process.
fn encode<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| {
        br#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"the answer could not be encoded","data":{"code":"internal"}},"id":null}"#.to_vec()
    })
}

#[cfg(test)]
mod tests {
    use mixengine_proto::rpc::ResponseOutcome;

    use super::*;
    use crate::api::test_daemon::{answer_json, call, undeclared};

    /// Whether a response says the call succeeded.
    fn succeeded(response: &Response) -> bool {
        matches!(response.outcome, ResponseOutcome::Success { .. })
    }

    /// A handler that panics is answered `internal`, and the daemon stays up to say so.
    #[tokio::test]
    async fn a_handler_that_panics_is_answered_internal() {
        let failure = contained(&Background::new(), "test.panics", async {
            panic!("on purpose")
        })
        .await
        .expect_err("a panic is not an answer");

        assert_eq!(failure.code, RpcCode::INTERNAL_ERROR);
    }

    #[tokio::test]
    async fn a_handler_that_answers_is_passed_through() {
        let value = contained(&Background::new(), "test.answers", async {
            Ok(serde_json::json!(7))
        })
        .await
        .expect("the handler's own answer");

        assert_eq!(value, serde_json::json!(7));
    }

    /// A request that lands after the shutdown closed the set is answered, not left waiting.
    #[tokio::test]
    async fn a_request_during_shutdown_is_answered_rather_than_left_waiting() {
        let background = Background::new();
        background.shut_down(std::time::Duration::ZERO).await;

        let answered = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            contained(&background, "test.late", async { Ok(serde_json::json!(1)) }),
        )
        .await
        .expect("an answer rather than a wait");

        assert_eq!(
            answered.expect_err("nothing ran").code,
            RpcCode::INTERNAL_ERROR
        );
    }

    #[tokio::test]
    async fn a_string_id_comes_back_as_the_same_string() {
        let answer = call(r#"{"jsonrpc":"2.0","method":"daemon.version","id":"handshake"}"#).await;

        assert_eq!(answer["id"], "handshake");
    }

    #[tokio::test]
    async fn an_unknown_method_is_method_not_found_and_says_which_one() {
        // A namespace this build has not reached — `site.create` used to stand here, then
        // `blueprint.apply` and then `extension.install`. Each of them becoming a real method is
        // exactly the drift this test is worth keeping past; what stands here now is a method
        // nobody has written.
        let answer = call(r#"{"jsonrpc":"2.0","method":"extension.open","id":1}"#).await;

        assert_eq!(answer["error"]["code"], -32601);
        assert_eq!(answer["error"]["data"]["code"], "not_found");
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("extension.open")),
            "{answer}"
        );
    }

    #[tokio::test]
    async fn parameters_to_a_method_that_takes_none_are_refused() {
        let answer =
            call(r#"{"jsonrpc":"2.0","method":"daemon.status","params":{"verbose":true},"id":1}"#)
                .await;

        assert_eq!(answer["error"]["code"], -32602);
        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn the_four_ways_of_spelling_no_parameters_are_all_accepted() {
        for params in [
            r#","params":null"#,
            r#","params":[]"#,
            r#","params":{}"#,
            "",
        ] {
            let body = format!(r#"{{"jsonrpc":"2.0","method":"daemon.version"{params},"id":1}}"#);
            let answer = call(&body).await;

            assert!(answer.get("result").is_some(), "{params}: {answer}");
        }
    }

    #[tokio::test]
    async fn a_body_that_is_not_json_is_a_parse_error_with_a_null_id() {
        let answer = call("not json at all").await;

        assert_eq!(answer["error"]["code"], -32700);
        assert!(answer["id"].is_null(), "{answer}");
    }

    #[tokio::test]
    async fn a_request_that_is_not_one_keeps_the_id_it_claimed() {
        // No `method`, so it never becomes a `Request` — but a client waiting on id 9 still has to
        // be told that id 9 is the one that was wrong.
        let answer = call(r#"{"jsonrpc":"2.0","id":9}"#).await;

        assert_eq!(answer["error"]["code"], -32600);
        assert_eq!(answer["id"], 9);
    }

    #[tokio::test]
    async fn a_notification_is_answered_with_silence() {
        assert!(
            answer(
                &undeclared().await.api,
                br#"{"jsonrpc":"2.0","method":"daemon.status"}"#
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn a_null_id_is_answered_rather_than_mistaken_for_a_notification() {
        // The spec discourages a null id and nowhere lets it mean silence: a request carrying one
        // is still a request, and a client that sent it is still waiting. Answered to the id it
        // gave, which is `null`.
        let answer = call(r#"{"jsonrpc":"2.0","method":"daemon.version","id":null}"#).await;

        assert_eq!(answer["result"]["protocol"], 1);
        assert!(answer["id"].is_null(), "{answer}");
    }

    #[tokio::test]
    async fn a_notification_that_fails_is_still_answered_with_silence() {
        assert!(
            answer(
                &undeclared().await.api,
                br#"{"jsonrpc":"2.0","method":"nope.nope"}"#
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn a_batch_answers_every_call_that_asked_for_one() {
        let answered = answer(
            &undeclared().await.api,
            br#"[
                {"jsonrpc":"2.0","method":"daemon.version","id":1},
                {"jsonrpc":"2.0","method":"daemon.status"},
                {"jsonrpc":"2.0","method":"nope.nope","id":3}
            ]"#,
        )
        .await
        .expect("two of the three asked for an answer");

        let answers: Vec<Response> = serde_json::from_slice(&answered).expect("an array of them");

        assert_eq!(answers.len(), 2, "the notification is not answered");
        assert!(succeeded(&answers[0]));
        assert!(!succeeded(&answers[1]));
        assert_eq!(answers[1].id, Some(Id::Number(3)));
    }

    #[tokio::test]
    async fn a_batch_of_nothing_but_notifications_answers_nothing_at_all() {
        assert!(
            answer(
                &undeclared().await.api,
                br#"[{"jsonrpc":"2.0","method":"daemon.status"},{"jsonrpc":"2.0","method":"daemon.version"}]"#
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn an_empty_batch_is_an_invalid_request_rather_than_an_empty_array() {
        let answer = call("[]").await;

        assert_eq!(answer["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn a_batch_element_that_is_not_an_object_fails_on_its_own() {
        let answered = answer(
            &undeclared().await.api,
            br#"[1, {"jsonrpc":"2.0","method":"daemon.version","id":2}]"#,
        )
        .await
        .expect("both elements produce an answer");

        let answers: Vec<Response> = serde_json::from_slice(&answered).unwrap();

        assert_eq!(answers.len(), 2);
        assert!(!succeeded(&answers[0]));
        assert_eq!(answers[0].id, None, "a bare number has no id to echo");
        assert!(succeeded(&answers[1]), "the sound call is still answered");
    }

    #[tokio::test]
    async fn a_handler_that_panics_answers_internal_and_leaves_the_daemon_up() {
        let api = undeclared().await.api;

        let answered = answer(
            &api,
            br#"{"jsonrpc":"2.0","method":"daemon.__panic","id":1}"#,
        )
        .await
        .expect("a panic is still an answer");
        let answer: Value = serde_json::from_slice(&answered).unwrap();

        assert_eq!(answer["error"]["code"], -32603);
        assert_eq!(answer["error"]["data"]["code"], "internal");
        // The panic message is not handed to the client — a backtrace is not something a user can
        // act on, and `logs/daemon.log` already has it.
        assert!(
            !answer["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("on purpose"),
            "{answer}"
        );

        // The whole point: the next request is answered normally.
        let after = answer_json(
            &api,
            r#"{"jsonrpc":"2.0","method":"daemon.version","id":2}"#,
        )
        .await;
        assert_eq!(after["result"]["version"], "0.1.0");
    }

    #[tokio::test]
    async fn another_protocol_version_is_an_invalid_request() {
        let answer = call(r#"{"jsonrpc":"1.0","method":"daemon.status","id":1}"#).await;

        assert_eq!(answer["error"]["code"], -32600);
    }
}
