//! The socket: `/v1` over HTTPS, and nothing about what a record means.
//!
//! **Every refusal becomes one of this application's own errors**, because the server's `code` is
//! a key the server chose and the person reads a sentence MixLab chose (D4a). And `reqwest` here
//! has no `json` feature, so bodies are encoded and decoded with `serde_json` by hand.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::wire::{BatchResponse, BatchResult, Capabilities, ErrorBody, Heads, Operation, Page};
use crate::error::AppError;

/// What a pull can be told about its cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageOutcome {
    Page(Page),
    /// `410 cursor-expired`: the server has forgotten enough that this machine must start over.
    CursorExpired,
}

/// The two calls the engine makes, as a trait so the engine is tested without a server.
pub trait Remote {
    /// `resync`: `since` came from a read that began at 0, so the server does not expire it
    /// (T178b, M4).
    fn page(
        &self,
        collection: &str,
        since: i64,
        resync: bool,
    ) -> impl Future<Output = Result<PageOutcome, AppError>> + Send;

    fn batch(
        &self,
        operations: &[Operation],
    ) -> impl Future<Output = Result<Vec<BatchResult>, AppError>> + Send;
}

/// `POST /v1/records/heads` (T189). A trait of its own rather than a third method of [`Remote`]:
/// every fake that pages or pushes would have to answer a question it is never asked.
pub trait AskHeads {
    fn heads(
        &self,
        cursors: &BTreeMap<String, i64>,
    ) -> impl Future<Output = Result<Heads, AppError>> + Send;
}

pub struct Transport {
    http: Client,
    base: String,
    token: String,
    /// `X-MixLab-Access`, for a server somebody closed to their own people (D4a).
    access: Option<String>,
}

impl Transport {
    pub fn new(base: &str, token: &str, access: Option<&str>) -> Result<Self, AppError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(unreachable)?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            access: access.map(str::to_owned),
        })
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let builder = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token);
        match &self.access {
            Some(access) => builder.header("X-MixLab-Access", access),
            None => builder,
        }
    }

    pub async fn capabilities(&self) -> Result<Capabilities, AppError> {
        let response = self
            .request(Method::GET, "/v1/capabilities")
            .send()
            .await
            .map_err(unreachable)?;
        read(response).await
    }

    /// Every record the account holds after `since`, across collections — what a copy reads
    /// (D4b). Not part of [`Remote`]: nothing but a copy asks for the whole account at once.
    pub async fn page_all(&self, since: i64, resync: bool) -> Result<Page, AppError> {
        let response = self
            .request(
                Method::GET,
                &format!("/v1/records?since={since}{}", resync_flag(resync)),
            )
            .send()
            .await
            .map_err(unreachable)?;
        read(response).await
    }
}

impl Remote for Transport {
    async fn page(
        &self,
        collection: &str,
        since: i64,
        resync: bool,
    ) -> Result<PageOutcome, AppError> {
        // Both halves are safe in a query string: an opaque id is 64 hex characters and a cursor
        // is a number.
        let path = format!(
            "/v1/records?collection={collection}&since={since}{}",
            resync_flag(resync)
        );
        let response = self
            .request(Method::GET, &path)
            .send()
            .await
            .map_err(unreachable)?;
        if response.status() == StatusCode::GONE {
            let body: ErrorBody = decode(&response.bytes().await.map_err(unreachable)?)?;
            return if body.error.code == "cursor-expired" {
                Ok(PageOutcome::CursorExpired)
            } else {
                Err(refusal(&body))
            };
        }
        read(response).await.map(PageOutcome::Page)
    }

    async fn batch(&self, operations: &[Operation]) -> Result<Vec<BatchResult>, AppError> {
        #[derive(Serialize)]
        struct Batch<'a> {
            operations: &'a [Operation],
        }
        let body = serde_json::to_vec(&Batch { operations })
            .map_err(|_| err!("error.syncCannotEncodeRequest"))?;
        let response = self
            .request(Method::POST, "/v1/records/batch")
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(unreachable)?;
        read::<BatchResponse>(response)
            .await
            .map(|batch| batch.results)
    }
}

impl AskHeads for Transport {
    async fn heads(&self, cursors: &BTreeMap<String, i64>) -> Result<Heads, AppError> {
        #[derive(Serialize)]
        struct Body<'a> {
            cursors: &'a BTreeMap<String, i64>,
        }
        let body = serde_json::to_vec(&Body { cursors })
            .map_err(|_| err!("error.syncCannotEncodeRequest"))?;
        let response = self
            .request(Method::POST, "/v1/records/heads")
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(unreachable)?;
        read(response).await
    }
}

/// `resync=1`: this cursor came from a read that began at 0 (T178b, M4).
fn resync_flag(resync: bool) -> &'static str {
    if resync {
        "&resync=1"
    } else {
        ""
    }
}

async fn read<T: DeserializeOwned>(response: Response) -> Result<T, AppError> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(unreachable)?;
    if status.is_success() {
        return decode(&bytes);
    }
    Err(match serde_json::from_slice::<ErrorBody>(&bytes) {
        Ok(body) => refusal(&body),
        Err(_) => err!("error.syncServerRefused", code = status.as_u16()),
    })
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, AppError> {
    serde_json::from_slice(bytes).map_err(|_| err!("error.syncServerAnswerUnreadable"))
}

pub(super) fn unreachable(error: reqwest::Error) -> AppError {
    err!("error.syncServerUnreachable", message = error)
}

/// A server's refusal, in this application's words. Codes with no sentence of their own fall
/// through to one that names the code, rather than to silence.
pub fn refusal(body: &ErrorBody) -> AppError {
    match body.error.code.as_str() {
        "invalid-token" => err!("error.syncSignedOut"),
        "invalid-access-token" => err!("error.syncAccessTokenRejected"),
        "account-frozen" => err!("error.syncAccountFrozen"),
        "too-many-requests" | "too-many-attempts" => err!(
            "error.syncTooManyRequests",
            seconds = body.error.retry_after.unwrap_or(60)
        ),
        "quota-exceeded" => err!("error.syncQuotaExceeded"),
        "request-too-large" => err!("error.syncRequestTooLarge"),
        "record-too-large" => err!("error.syncRecordTooLarge"),
        other => err!("error.syncServerRefused", code = other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::wire::ErrorDetail;

    fn body(code: &str, retry_after: Option<u64>) -> ErrorBody {
        ErrorBody {
            error: ErrorDetail {
                code: code.into(),
                retry_after,
            },
        }
    }

    #[test]
    fn a_known_refusal_has_a_sentence_of_its_own() {
        assert_eq!(
            refusal(&body("invalid-token", None)).code,
            "error.syncSignedOut"
        );
        assert_eq!(
            refusal(&body("account-frozen", None)).code,
            "error.syncAccountFrozen"
        );
        assert_eq!(
            refusal(&body("invalid-access-token", None)).code,
            "error.syncAccessTokenRejected"
        );
    }

    #[test]
    fn a_throttle_says_how_long() {
        let error = refusal(&body("too-many-requests", Some(42)));
        assert_eq!(error.code, "error.syncTooManyRequests");
        assert_eq!(error.params.get("seconds"), Some(&"42".to_string()));
    }

    #[test]
    fn an_unknown_code_is_named_rather_than_swallowed() {
        let error = refusal(&body("something-new", None));
        assert_eq!(error.code, "error.syncServerRefused");
        assert_eq!(error.params.get("code"), Some(&"something-new".to_string()));
    }
}
