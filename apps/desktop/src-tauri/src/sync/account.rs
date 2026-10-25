//! The account routes of `/v1`: registering, the letter's code, signing in, refreshing, and the
//! device list (`docs/features/sync-protocol.md` — Accounts, Devices, Tokens).
//!
//! **Nothing here derives or keeps a key.** A caller hands in what it derived and gets back what
//! the server said; what to keep is `session`'s. A refusal becomes a sentence of this
//! application's, as in `transport`, with the account's own codes first — `invalid-token` on
//! `verify` is a wrong code in a letter, not a session that ended.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Method, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::crypto::{ARGON_M_COST, ARGON_P_COST, ARGON_T_COST};
use super::transport::{self, unreachable};
use super::wire::{Capabilities, ErrorBody};
use crate::error::AppError;

/// Argon2id's cost, as the server stores it for an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon {
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

impl Argon {
    /// The one set MixLab derives with (D2). An account made with another is one this client says
    /// it cannot sign in to, rather than deriving a key that will not match.
    pub fn ours() -> Self {
        Self {
            m: ARGON_M_COST,
            t: ARGON_T_COST,
            p: ARGON_P_COST,
        }
    }
}

/// `GET /v1/auth/params`: what a fresh install needs to derive `A`. The same shape for an address
/// with no account, by design.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    pub salt_account: String,
    pub argon: Argon,
}

/// `POST /v1/auth/register`. Every byte field is base64.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub email: String,
    pub a: String,
    pub salt_account: String,
    pub argon: Argon,
    pub wrapped_mk_password: String,
    pub wrapped_mk_recovery: String,
}

/// `POST /v1/auth/login`. No `Debug`: most of it is a secret.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedIn {
    pub access_token: String,
    pub refresh_token: String,
    pub device_id: String,
    pub expires_in: i64,
    pub wrapped_mk_password: String,
    pub wrapped_mk_recovery: String,
    /// The account's own id: random at registration, never reused, so an address deleted and
    /// registered again is another account.
    pub account_id: String,
}

/// `POST /v1/auth/refresh`. The refresh token that asked for it is spent the moment this arrives.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refreshed {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

/// `POST /v1/auth/password` (D6 case 1): the current verifier and the new keys. Every other
/// machine is signed out by it; this one is not.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordChange {
    pub a: String,
    pub new_a: String,
    pub new_salt_account: String,
    pub new_wrapped_mk_password: String,
}

/// The keys a completed reset installs — after a code (case 3) or a ticket (case 2). Every field
/// is base64.
#[derive(Clone)]
pub struct NewKeys {
    pub a: String,
    pub salt_account: String,
    pub wrapped_mk_password: String,
    pub wrapped_mk_recovery: String,
}

/// Case 2's first answer: the code is spent, and this is what it earned.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub wrapped_mk_recovery: String,
    pub ticket: String,
    pub expires_in: i64,
}

/// How many records a reset or a deletion removed — every one in case 3 and on deletion, none in
/// case 2.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub records_deleted: u64,
}

/// `GET` and `POST /v1/account/freeze`: whether the account holds still for a copy (D4b), and since
/// when. Answered in both states, so a machine that meets `423` can find out why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Freeze {
    pub state: String,
    pub frozen_at: Option<i64>,
}

/// One row of `GET /v1/devices`, handed to the account screen as it came.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub current: bool,
}

#[derive(Deserialize)]
struct Devices {
    devices: Vec<Device>,
}

/// The `{}` most account routes answer with.
#[derive(Deserialize)]
struct Empty {}

pub struct Account {
    http: Client,
    base: String,
    /// `X-MixLab-Access`, for a server somebody closed to their own people.
    access: Option<String>,
}

impl Account {
    pub fn new(base: &str, access: Option<&str>) -> Result<Self, AppError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(unreachable)?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            access: access.map(str::to_owned),
        })
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let builder = self.http.request(method, format!("{}{path}", self.base));
        match &self.access {
            Some(access) => builder.header("X-MixLab-Access", access),
            None => builder,
        }
    }

    fn post(&self, path: &str, body: &impl Serialize) -> Result<RequestBuilder, AppError> {
        let body = serde_json::to_vec(body).map_err(|_| err!("error.syncCannotEncodeRequest"))?;
        Ok(self
            .request(Method::POST, path)
            .header(CONTENT_TYPE, "application/json")
            .body(body))
    }

    pub async fn params(&self, email: &str) -> Result<Params, AppError> {
        let email: String = url::form_urlencoded::byte_serialize(email.as_bytes()).collect();
        let path = format!("/v1/auth/params?email={email}");
        answer(send(self.request(Method::GET, &path)).await?, refusal).await
    }

    pub async fn register(&self, registration: &Registration) -> Result<(), AppError> {
        let request = self.post("/v1/auth/register", registration)?;
        answer::<Empty>(send(request).await?, refusal)
            .await
            .map(drop)
    }

    /// Spend the letter's code. **`invalid-token` here is the code** — wrong, spent or expired —
    /// and not a session, which this route does not have.
    pub async fn verify(&self, email: &str, code: &str) -> Result<(), AppError> {
        let request = self.post("/v1/auth/verify", &json!({ "email": email, "token": code }))?;
        answer::<Empty>(send(request).await?, |body| {
            match body.error.code.as_str() {
                "invalid-token" | "invalid-code" => err!("error.syncWrongCode"),
                _ => refusal(body),
            }
        })
        .await
        .map(drop)
    }

    pub async fn login(
        &self,
        email: &str,
        a: &[u8; 32],
        device_name: &str,
    ) -> Result<SignedIn, AppError> {
        let body = json!({ "email": email, "a": STANDARD.encode(a), "deviceName": device_name });
        answer(send(self.post("/v1/auth/login", &body)?).await?, refusal).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<Refreshed, AppError> {
        let body = json!({ "refreshToken": refresh_token });
        answer(send(self.post("/v1/auth/refresh", &body)?).await?, refusal).await
    }

    pub async fn devices(&self, access_token: &str) -> Result<Vec<Device>, AppError> {
        let request = self
            .request(Method::GET, "/v1/devices")
            .bearer_auth(access_token);
        answer::<Devices>(send(request).await?, refusal)
            .await
            .map(|list| list.devices)
    }

    /// Cut a device off — both its tokens, at once. Revoking this machine's own is signing out.
    pub async fn revoke(&self, access_token: &str, id: &str) -> Result<(), AppError> {
        let id: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        let request = self
            .request(Method::DELETE, &format!("/v1/devices/{id}"))
            .bearer_auth(access_token);
        answer::<Empty>(send(request).await?, refusal)
            .await
            .map(drop)
    }

    pub async fn change_password(
        &self,
        access_token: &str,
        change: &PasswordChange,
    ) -> Result<(), AppError> {
        let request = self
            .post("/v1/auth/password", change)?
            .bearer_auth(access_token);
        answer::<Empty>(send(request).await?, refusal)
            .await
            .map(drop)
    }

    /// Ask for the letter. Answers the same whether or not the address has an account.
    pub async fn ask_reset(&self, email: &str) -> Result<(), AppError> {
        let request = self.post("/v1/auth/reset", &json!({ "email": email }))?;
        answer::<Empty>(send(request).await?, reset_refusal)
            .await
            .map(drop)
    }

    /// Case 2, first request: spend the code for the wrapped recovery copy and a ticket.
    pub async fn open_reset(&self, email: &str, code: &str) -> Result<Opened, AppError> {
        let request = self.post("/v1/auth/reset", &json!({ "email": email, "token": code }))?;
        answer(send(request).await?, reset_refusal).await
    }

    /// Case 2, second request: the ticket and the new keys. The records stay.
    pub async fn reset_with_ticket(
        &self,
        email: &str,
        ticket: &str,
        keys: &NewKeys,
    ) -> Result<Deleted, AppError> {
        let body = json!({
            "email": email, "ticket": ticket, "a": keys.a, "saltAccount": keys.salt_account,
            "wrappedMkPassword": keys.wrapped_mk_password,
            "wrappedMkRecovery": keys.wrapped_mk_recovery,
        });
        answer(
            send(self.post("/v1/auth/reset", &body)?).await?,
            reset_refusal,
        )
        .await
    }

    /// Case 3: the code and the new keys in one request. **Every record is deleted.**
    pub async fn reset_with_code(
        &self,
        email: &str,
        code: &str,
        keys: &NewKeys,
    ) -> Result<Deleted, AppError> {
        let body = json!({
            "email": email, "token": code, "a": keys.a, "saltAccount": keys.salt_account,
            "wrappedMkPassword": keys.wrapped_mk_password,
            "wrappedMkRecovery": keys.wrapped_mk_recovery,
        });
        answer(
            send(self.post("/v1/auth/reset", &body)?).await?,
            reset_refusal,
        )
        .await
    }

    pub async fn freeze_state(&self, access_token: &str) -> Result<Freeze, AppError> {
        let request = self
            .request(Method::GET, "/v1/account/freeze")
            .bearer_auth(access_token);
        answer(send(request).await?, refusal).await
    }

    /// Hold the account still for a copy, or let it go. Nothing else ends a freeze (D4b).
    pub async fn set_freeze(&self, access_token: &str, frozen: bool) -> Result<Freeze, AppError> {
        let state = if frozen { "frozen" } else { "active" };
        let request = self
            .post("/v1/account/freeze", &json!({ "state": state }))?
            .bearer_auth(access_token);
        answer(send(request).await?, refusal).await
    }

    /// `GET /v1/capabilities`, before anybody signs in: what the sign-in form asks a server so it
    /// can say, before a registration, that the server has announced its end. Needs no session;
    /// a closed server still wants its access token.
    pub async fn capabilities(&self) -> Result<Capabilities, AppError> {
        answer(
            send(self.request(Method::GET, "/v1/capabilities")).await?,
            refusal,
        )
        .await
    }

    /// `POST /v1/account/check`: whether `a` is this account's password, asked before a move.
    pub async fn check(&self, access_token: &str, a: &[u8; 32]) -> Result<(), AppError> {
        let request = self
            .post("/v1/account/check", &json!({ "a": STANDARD.encode(a) }))?
            .bearer_auth(access_token);
        answer::<Empty>(send(request).await?, refusal)
            .await
            .map(drop)
    }

    /// Delete the account and every record in it. **Re-proves the password**: a session alone is
    /// what a borrowed, unlocked machine already has (D4b).
    pub async fn delete_account(
        &self,
        access_token: &str,
        a: &[u8; 32],
    ) -> Result<Deleted, AppError> {
        let request = self
            .post("/v1/account/delete", &json!({ "a": STANDARD.encode(a) }))?
            .bearer_auth(access_token);
        answer(send(request).await?, refusal).await
    }
}

async fn send(request: RequestBuilder) -> Result<Response, AppError> {
    request.send().await.map_err(unreachable)
}

async fn answer<T: DeserializeOwned>(
    response: Response,
    refuse: impl Fn(&ErrorBody) -> AppError,
) -> Result<T, AppError> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(unreachable)?;
    if status.is_success() {
        return success(&bytes);
    }
    Err(match serde_json::from_slice::<ErrorBody>(&bytes) {
        Ok(body) => refuse(&body),
        Err(_) => err!("error.syncServerRefused", code = status.as_u16()),
    })
}

/// A success body. `204` has none, and reads as `{}`.
fn success<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, AppError> {
    let bytes = if bytes.is_empty() {
        b"{}".as_slice()
    } else {
        bytes
    };
    serde_json::from_slice(bytes).map_err(|_| err!("error.syncServerAnswerUnreadable"))
}

/// `reset` has no session: `invalid-code` is the letter's code, `invalid-token` the ticket.
fn reset_refusal(body: &ErrorBody) -> AppError {
    match body.error.code.as_str() {
        "invalid-code" => err!("error.syncWrongCode"),
        "invalid-token" => err!("error.syncResetExpired"),
        _ => refusal(body),
    }
}

/// The account's own refusals, then everything `transport` already has a sentence for.
fn refusal(body: &ErrorBody) -> AppError {
    match body.error.code.as_str() {
        "invalid-credentials" => err!("error.syncWrongPassword"),
        "email-not-verified" => err!("error.syncEmailNotVerified"),
        "email-taken" => err!("error.syncEmailTaken"),
        "invalid-email" => err!("error.syncInvalidEmail"),
        "invalid-device-name" => err!("error.syncInvalidDeviceName"),
        "letter-not-sent" => err!("error.syncLetterNotSent"),
        _ => transport::refusal(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::wire::ErrorDetail;

    fn body(code: &str) -> ErrorBody {
        ErrorBody {
            error: ErrorDetail {
                code: code.into(),
                retry_after: None,
            },
        }
    }

    #[test]
    fn a_registration_names_what_the_protocol_names() {
        let registration = Registration {
            email: "e".into(),
            a: "a".into(),
            salt_account: "s".into(),
            argon: Argon::ours(),
            wrapped_mk_password: "p".into(),
            wrapped_mk_recovery: "r".into(),
        };
        assert_eq!(
            serde_json::to_value(&registration).unwrap(),
            json!({ "email": "e", "a": "a", "saltAccount": "s",
                    "argon": { "m": 65536, "t": 3, "p": 4 },
                    "wrappedMkPassword": "p", "wrappedMkRecovery": "r" })
        );
    }

    #[test]
    fn the_account_s_refusals_have_sentences_of_their_own() {
        assert_eq!(
            refusal(&body("invalid-credentials")).code,
            "error.syncWrongPassword"
        );
        assert_eq!(
            refusal(&body("email-not-verified")).code,
            "error.syncEmailNotVerified"
        );
        assert_eq!(refusal(&body("email-taken")).code, "error.syncEmailTaken");
        assert_eq!(
            refusal(&body("letter-not-sent")).code,
            "error.syncLetterNotSent"
        );
    }

    /// Everything else is the transport's sentence, so a session that ended says so in one voice.
    #[test]
    fn the_rest_fall_through_to_the_transport() {
        assert_eq!(refusal(&body("invalid-token")).code, "error.syncSignedOut");
        assert_eq!(
            refusal(&body("something-new")).code,
            "error.syncServerRefused"
        );
    }

    #[test]
    fn a_body_less_success_reads_as_nothing() {
        let _: Empty = success(b"").unwrap();
        let _: Empty = success(b"{}").unwrap();
    }

    #[test]
    fn a_device_reads_as_the_list_sends_it() {
        let device: Device = serde_json::from_value(json!({
            "current": true, "id": "d1", "name": "laptop", "createdAt": 1, "lastSeenAt": 2
        }))
        .unwrap();
        assert!(device.current);
        assert_eq!(device.last_seen_at, 2);
    }

    #[test]
    fn a_password_change_names_what_the_protocol_names() {
        let change = PasswordChange {
            a: "a".into(),
            new_a: "n".into(),
            new_salt_account: "s".into(),
            new_wrapped_mk_password: "w".into(),
        };
        assert_eq!(
            serde_json::to_value(&change).unwrap(),
            json!({ "a": "a", "newA": "n", "newSaltAccount": "s", "newWrappedMkPassword": "w" })
        );
    }

    /// On `reset`, `invalid-code` is the letter's code and `invalid-token` is the ticket: neither
    /// is a session, which this route does not have.
    #[test]
    fn a_reset_s_refusals_are_about_the_code_and_the_ticket() {
        assert_eq!(
            reset_refusal(&body("invalid-code")).code,
            "error.syncWrongCode"
        );
        assert_eq!(
            reset_refusal(&body("invalid-token")).code,
            "error.syncResetExpired"
        );
        assert_eq!(
            reset_refusal(&body("too-many-attempts")).code,
            "error.syncTooManyRequests"
        );
    }

    #[test]
    fn an_opened_reset_reads_as_the_server_sends_it() {
        let opened: Opened = serde_json::from_value(json!({
            "wrappedMkRecovery": "w", "ticket": "t", "expiresIn": 600
        }))
        .unwrap();
        assert_eq!(opened.ticket, "t");
        let reset: Deleted = serde_json::from_value(json!({ "recordsDeleted": 214 })).unwrap();
        assert_eq!(reset.records_deleted, 214);
    }

    #[test]
    fn a_freeze_reads_as_the_server_sends_it() {
        let frozen: Freeze =
            serde_json::from_value(json!({ "state": "frozen", "frozenAt": 1758300000 })).unwrap();
        assert_eq!(frozen.frozen_at, Some(1_758_300_000));
        let active: Freeze =
            serde_json::from_value(json!({ "state": "active", "frozenAt": null })).unwrap();
        assert_eq!(active.state, "active");
        assert_eq!(active.frozen_at, None);
    }

    #[test]
    fn a_deletion_says_how_many_went() {
        let deleted: Deleted = serde_json::from_value(json!({ "recordsDeleted": 3 })).unwrap();
        assert_eq!(deleted.records_deleted, 3);
    }
}
