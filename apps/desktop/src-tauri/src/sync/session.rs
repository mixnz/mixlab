//! Signed in or not: the account on this machine, held for the run and kept between runs.
//!
//! **One lock over everything that changes the session, held across a refresh on purpose**: a
//! refresh token rotates on use, and two refreshes racing with one token revoke the device's whole
//! chain (`docs/features/sync-protocol.md`, Tokens). Everything else takes a snapshot and lets go.
//!
//! **What a module writes is held here until it has written it.** A pulled page, or a lost
//! conflict's winners, leaves as a token and plain items; only when the shell hands the token back
//! are versions and hashes recorded and the cursor moved (D4, and `lend`'s module comment).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::Serialize;
use tokio::sync::Mutex;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::account::{Account, Argon, Device, Freeze, NewKeys, PasswordChange, Registration};
use super::copy;
use super::crypto;
use super::engine::{self, Fetched};
use super::lend::{self, Agreement, Incoming, Item, Keys};
use super::saved::{Keeping, Saved};
use super::store::Store;
use super::transport::Transport;
use super::wire::{Capabilities, WireRecord};
use crate::error::AppError;
use crate::platform::in_background;

/// Refresh this long before the access token lapses, so no request leaves with a token that dies
/// on the way.
const RENEW_BEFORE: i64 = 60;

/// The running app. A record skipped under one version is asked for again under the next (T178d).
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the account screen draws from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub signed_in: bool,
    pub server: Option<String>,
    pub email: Option<String>,
    pub device_id: Option<String>,
    /// The day the server said it closes (`/v1/capabilities`), once this run has opened a session.
    /// Advisory: nothing stops working on it (D4b).
    pub closing_on: Option<i64>,
    /// The address a code was sent to, while registration waits for it.
    pub verifying: Option<String>,
}

/// A page for a module to write. `token` comes back with `commit_pull` once it has.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PulledPage {
    pub token: String,
    pub changes: Incoming,
    pub more: bool,
}

/// What a push did. `replaced` is this machine's edits that lost to newer ones: the module writes
/// them and hands `token` back with `commit_push`. No token, nothing to write.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushedChanges {
    pub accepted: usize,
    pub replaced: Incoming,
    pub token: Option<String>,
    /// The first refusal, of one entry or of a whole request. What landed is already agreed; the
    /// shell reports this once it has written `replaced` (T178c, C2).
    pub error: Option<AppError>,
    /// This account's collection was never pulled, so nothing was sent: a push trusts that a
    /// record it never saw is one the server does not have, which holds only after a pull. The
    /// shell syncs the collection in full instead.
    pub needs_pull: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Pull,
    Push,
}

/// Records handed to a module, waiting for it to say it wrote them.
struct Held {
    token: String,
    records: Vec<WireRecord>,
    agreements: Vec<Agreement>,
    /// A pulled page's cursor; `None` for a lost conflict's winners, which move no cursor.
    fetched: Option<Fetched>,
    /// On the page that ends a resync: what it never met, forgotten on commit (T178b, M2).
    unmet: Vec<String>,
}

/// Registration between its first step and its last: `A` to sign in with, and `MK`, which was
/// made here and needs no unwrapping.
#[derive(Zeroize, ZeroizeOnDrop)]
struct Registering {
    server: String,
    access: Option<String>,
    email: String,
    a: [u8; 32],
    master: [u8; 32],
}

/// Where a reset is happening: the server and the address it concerns.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct Address {
    server: String,
    access: Option<String>,
    email: String,
}

/// Case 2 after the code was spent: the ticket, and the wrapped copy it earned. Held so that a
/// mistyped recovery key is typed again rather than costing a second letter.
#[derive(Zeroize, ZeroizeOnDrop)]
struct OpenedReset {
    address: Address,
    ticket: String,
    wrapped_mk_recovery: String,
}

/// Case 3 before the code is spent: the new `MK` and every key made from it, held until the new
/// recovery key has been shown and typed back — because spending the code is what deletes.
#[derive(Zeroize, ZeroizeOnDrop)]
struct StartingOver {
    address: Address,
    a: [u8; 32],
    master: [u8; 32],
    salt_account: String,
    wrapped_mk_password: String,
    wrapped_mk_recovery: String,
}

/// The new server, once registration there has been sent.
#[derive(Zeroize, ZeroizeOnDrop)]
struct Moving {
    to: Address,
    /// `A`, the same on both servers: same password, same salt (D4b).
    a: [u8; 32],
    /// Filled once the address is confirmed and this machine has signed in there.
    arrived: Option<Arrived>,
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct Arrived {
    access_token: String,
    refresh_token: String,
    device_id: String,
    expires_in: i64,
    account_id: String,
}

/// What a copy did, for the screen that asks what to do with the old account.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    pub copied: usize,
}

/// One signed-in session: a snapshot, replaced whole when the tokens are refreshed.
struct Session {
    keys: Keys,
    device_id: String,
    access_token: String,
    transport: Transport,
    account: Account,
    store: Arc<Store>,
    limits: Capabilities,
    expires_at: i64,
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    saved: Option<Saved>,
    session: Option<Arc<Session>>,
    store: Option<Arc<Store>>,
    registering: Option<Registering>,
    opened: Option<OpenedReset>,
    starting_over: Option<StartingOver>,
    moving: Option<Moving>,
    held: HashMap<(Kind, String), Held>,
}

pub struct SyncState {
    keeping: Arc<dyn Keeping>,
    store_path: Result<PathBuf, AppError>,
    inner: Mutex<Inner>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn decode(text: &str) -> Result<Vec<u8>, AppError> {
    STANDARD
        .decode(text)
        .map_err(|_| err!("error.syncServerAnswerUnreadable"))
}

/// Argon2id and the two expansions, off the async thread. The password is wiped once used.
async fn derive(mut password: String, salt: Vec<u8>) -> Result<crypto::PasswordKeys, AppError> {
    in_background(move || {
        let keys = crypto::derive_password_keys(&password, &salt);
        password.zeroize();
        keys
    })
    .await
}

/// What a new password needs: a fresh salt, `A`, and `MK` wrapped under it.
struct Rewrapped {
    a: [u8; 32],
    salt: [u8; 16],
    wrapped: Vec<u8>,
}

async fn rewrap(password: String, master: [u8; 32]) -> Result<Rewrapped, AppError> {
    let salt: [u8; 16] = rand::random();
    let keys = derive(password, salt.to_vec()).await?;
    let wrapped = crypto::wrap_master_key(&keys.wrap, &master)?;
    Ok(Rewrapped {
        a: keys.auth,
        salt,
        wrapped,
    })
}

impl SyncState {
    pub fn new(keeping: Arc<dyn Keeping>, store_path: Result<PathBuf, AppError>) -> Self {
        Self {
            keeping,
            store_path,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// What the credential store kept, read once per run.
    async fn load(&self, inner: &mut Inner) -> Result<(), AppError> {
        if !inner.loaded {
            let keeping = self.keeping.clone();
            inner.saved = in_background(move || keeping.load()).await?;
            inner.loaded = true;
        }
        Ok(())
    }

    async fn keep(&self, saved: &Saved) -> Result<(), AppError> {
        let (keeping, saved) = (self.keeping.clone(), saved.clone());
        in_background(move || keeping.keep(&saved)).await
    }

    pub async fn status(&self) -> Result<Status, AppError> {
        let mut inner = self.inner.lock().await;
        self.load(&mut inner).await?;
        let saved = inner.saved.as_ref();
        Ok(Status {
            signed_in: saved.is_some(),
            server: saved.map(|saved| saved.server.clone()),
            email: saved.map(|saved| saved.email.clone()),
            device_id: saved.map(|saved| saved.device_id.clone()),
            closing_on: inner
                .session
                .as_ref()
                .and_then(|session| session.limits.closing_on),
            verifying: inner
                .registering
                .as_ref()
                .map(|registering| registering.email.clone()),
        })
    }

    /// Make the account and send the letter. Returns the recovery key, formatted, to be shown once.
    pub async fn register(
        &self,
        server: &str,
        access: Option<&str>,
        email: &str,
        mut password: String,
    ) -> Result<String, AppError> {
        let salt: [u8; 16] = rand::random();
        let keys = in_background(move || {
            let keys = crypto::derive_password_keys(&password, &salt);
            password.zeroize();
            keys
        })
        .await?;
        let master = crypto::new_master_key();
        let mut recovery = crypto::new_recovery_key();
        let registration = Registration {
            email: email.to_owned(),
            a: STANDARD.encode(keys.auth),
            salt_account: STANDARD.encode(salt),
            argon: Argon::ours(),
            wrapped_mk_password: STANDARD.encode(crypto::wrap_master_key(&keys.wrap, &master)?),
            wrapped_mk_recovery: STANDARD.encode(crypto::wrap_master_key(
                &crypto::recovery_wrapping_key(&recovery),
                &master,
            )?),
        };
        Account::new(server, access)?
            .register(&registration)
            .await?;
        let shown = crypto::format_recovery_key(&recovery);
        recovery.zeroize();
        self.inner.lock().await.registering = Some(Registering {
            server: server.to_owned(),
            access: access.map(str::to_owned),
            email: email.to_owned(),
            a: keys.auth,
            master,
        });
        Ok(shown)
    }

    /// Spend the letter's code, then sign in as the account's first device. A wrong code leaves
    /// the registration waiting, so the person can type it again.
    pub async fn verify(&self, code: &str, device_name: &str) -> Result<Status, AppError> {
        {
            let mut inner = self.inner.lock().await;
            let registering = inner
                .registering
                .as_ref()
                .ok_or_else(|| err!("error.syncNothingToVerify"))?;
            let account = Account::new(&registering.server, registering.access.as_deref())?;
            account.verify(&registering.email, code).await?;
            let signed_in = account
                .login(&registering.email, &registering.a, device_name)
                .await?;
            let saved = Saved {
                server: registering.server.clone(),
                access: registering.access.clone(),
                email: registering.email.clone(),
                device_id: signed_in.device_id.clone(),
                refresh_token: signed_in.refresh_token.clone(),
                master_key: STANDARD.encode(registering.master),
                wrapped_mk_recovery: Some(signed_in.wrapped_mk_recovery.clone()),
                account_id: Some(signed_in.account_id.clone()),
            };
            inner.registering = None;
            self.begin(
                &mut inner,
                saved,
                signed_in.access_token.clone(),
                signed_in.expires_in,
            )
            .await?;
        }
        self.status().await
    }

    /// Sign in on a machine that has only the address and the password (D2): ask for the salt,
    /// derive `A`, and unwrap `MK` from what signing in hands back.
    pub async fn login(
        &self,
        server: &str,
        access: Option<&str>,
        email: &str,
        mut password: String,
        device_name: &str,
    ) -> Result<Status, AppError> {
        let account = Account::new(server, access)?;
        let params = account.params(email).await?;
        if params.argon != Argon::ours() {
            return Err(err!("error.syncArgonUnsupported"));
        }
        let salt = decode(&params.salt_account)?;
        let keys = in_background(move || {
            let keys = crypto::derive_password_keys(&password, &salt);
            password.zeroize();
            keys
        })
        .await?;
        let signed_in = account.login(email, &keys.auth, device_name).await?;
        let mut master =
            crypto::unwrap_master_key(&keys.wrap, &decode(&signed_in.wrapped_mk_password)?)?;
        let saved = Saved {
            server: server.to_owned(),
            access: access.map(str::to_owned),
            email: email.to_owned(),
            device_id: signed_in.device_id.clone(),
            refresh_token: signed_in.refresh_token.clone(),
            master_key: STANDARD.encode(master),
            wrapped_mk_recovery: Some(signed_in.wrapped_mk_recovery.clone()),
            account_id: Some(signed_in.account_id.clone()),
        };
        master.zeroize();
        {
            let mut inner = self.inner.lock().await;
            self.begin(
                &mut inner,
                saved,
                signed_in.access_token.clone(),
                signed_in.expires_in,
            )
            .await?;
        }
        self.status().await
    }

    /// Keep what a sign-in produced, and make it the session. Anything held from before is
    /// dropped: it was read under another session.
    async fn begin(
        &self,
        inner: &mut Inner,
        saved: Saved,
        access_token: String,
        expires_in: i64,
    ) -> Result<(), AppError> {
        self.keep(&saved).await?;
        inner.loaded = true;
        inner.held.clear();
        inner.store = None;
        let session = self.open(inner, &saved, access_token, expires_in).await?;
        inner.session = Some(Arc::new(session));
        inner.saved = Some(saved);
        Ok(())
    }

    /// A session over `saved` with a fresh access token, and what the server allows today.
    async fn open(
        &self,
        inner: &mut Inner,
        saved: &Saved,
        access_token: String,
        expires_in: i64,
    ) -> Result<Session, AppError> {
        let store = match &inner.store {
            Some(store) => store.clone(),
            None => {
                let path = self.store_path.clone()?;
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| err!("error.syncStoreFailed", message = e))?;
                }
                let store = Arc::new(Store::open(&path, &saved.store_scope()).await?);
                inner.store = Some(store.clone());
                store
            }
        };
        let mut master = saved.master_key_bytes()?;
        let keys = Keys {
            id: crypto::id_key(&master),
            data: crypto::data_key(&master),
        };
        master.zeroize();
        let transport = Transport::new(&saved.server, &access_token, saved.access.as_deref())?;
        let limits = transport.capabilities().await?;
        Ok(Session {
            keys,
            device_id: saved.device_id.clone(),
            access_token,
            transport,
            account: Account::new(&saved.server, saved.access.as_deref())?,
            store,
            limits,
            expires_at: now() + expires_in,
        })
    }

    /// Everything forgotten, here and in the credential store.
    async fn end(&self, inner: &mut Inner) -> Result<(), AppError> {
        inner.saved = None;
        inner.session = None;
        inner.store = None;
        inner.held.clear();
        let keeping = self.keeping.clone();
        in_background(move || keeping.forget()).await
    }

    /// The session, refreshed first when its access token is about to lapse — or, given `stale`,
    /// because a request just said that token had already died. A refresh the server refuses
    /// means this machine was signed out elsewhere, and it forgets everything.
    async fn session(&self, stale: Option<&Arc<Session>>) -> Result<Arc<Session>, AppError> {
        let mut inner = self.inner.lock().await;
        self.load(&mut inner).await?;
        if let Some(current) = &inner.session {
            let renewed_meanwhile = stale.is_some_and(|stale| !Arc::ptr_eq(stale, current));
            let fresh = stale.is_none() && now() + RENEW_BEFORE < current.expires_at;
            if renewed_meanwhile || fresh {
                return Ok(current.clone());
            }
        }
        let Some(mut saved) = inner.saved.clone() else {
            return Err(err!("error.syncNotSignedIn"));
        };
        let account = Account::new(&saved.server, saved.access.as_deref())?;
        let refreshed = match account.refresh(&saved.refresh_token).await {
            Ok(refreshed) => refreshed,
            Err(error) => {
                if error.code == "error.syncSignedOut" {
                    self.end(&mut inner).await?;
                }
                return Err(error);
            }
        };
        // The old token is spent the moment the answer arrives: memory first, then the store.
        saved.refresh_token = refreshed.refresh_token.clone();
        inner.saved = Some(saved.clone());
        self.keep(&saved).await?;
        let session = Arc::new(
            self.open(
                &mut inner,
                &saved,
                refreshed.access_token.clone(),
                refreshed.expires_in,
            )
            .await?,
        );
        inner.session = Some(session.clone());
        Ok(session)
    }

    /// `call` against the session, and once more after a refresh if the token died on the way.
    async fn with_session<T, F, Fut>(&self, call: F) -> Result<T, AppError>
    where
        F: Fn(Arc<Session>) -> Fut,
        Fut: Future<Output = Result<T, AppError>>,
    {
        let session = self.session(None).await?;
        match call(session.clone()).await {
            Err(error) if error.code == "error.syncSignedOut" => {
                call(self.session(Some(&session)).await?).await
            }
            other => other,
        }
    }

    /// Sign out: this device revoked on the server if it can be reached, and everything here
    /// forgotten whether or not it could — the device list on another machine is where a leftover
    /// is cut off.
    pub async fn logout(&self) -> Result<(), AppError> {
        let mut inner = self.inner.lock().await;
        self.load(&mut inner).await?;
        if let Some(session) = inner.session.clone() {
            let _ = session
                .account
                .revoke(&session.access_token, &session.device_id)
                .await;
        }
        self.end(&mut inner).await
    }

    pub async fn devices(&self) -> Result<Vec<Device>, AppError> {
        self.with_session(
            |session| async move { session.account.devices(&session.access_token).await },
        )
        .await
    }

    /// Cut a device off. This machine's own is signing out.
    pub async fn revoke(&self, id: &str) -> Result<(), AppError> {
        if self.session(None).await?.device_id == id {
            return self.logout().await;
        }
        let id = id.to_owned();
        self.with_session(|session| {
            let id = id.clone();
            async move { session.account.revoke(&session.access_token, &id).await }
        })
        .await
    }

    /// D6 case 1: prove the current password, then re-wrap `MK` under the new one. Nothing is
    /// re-encrypted; every other machine is signed out by the server and this one is not.
    pub async fn change_password(&self, current: String, next: String) -> Result<(), AppError> {
        let session = self.session(None).await?;
        let saved = self
            .inner
            .lock()
            .await
            .saved
            .clone()
            .ok_or_else(|| err!("error.syncNotSignedIn"))?;
        let params = session.account.params(&saved.email).await?;
        let proven = derive(current, decode(&params.salt_account)?).await?;
        let fresh = rewrap(next, saved.master_key_bytes()?).await?;
        let change = PasswordChange {
            a: STANDARD.encode(proven.auth),
            new_a: STANDARD.encode(fresh.a),
            new_salt_account: STANDARD.encode(fresh.salt),
            new_wrapped_mk_password: STANDARD.encode(&fresh.wrapped),
        };
        self.with_session(|session| {
            let change = change.clone();
            async move {
                session
                    .account
                    .change_password(&session.access_token, &change)
                    .await
            }
        })
        .await
    }

    /// Ask for the letter. The same answer whether or not the address has an account.
    pub async fn ask_reset(
        &self,
        server: &str,
        access: Option<&str>,
        email: &str,
    ) -> Result<(), AppError> {
        Account::new(server, access)?.ask_reset(email).await
    }

    /// Case 2, first request: spend the code and hold the ticket.
    pub async fn open_reset(
        &self,
        server: &str,
        access: Option<&str>,
        email: &str,
        code: &str,
    ) -> Result<(), AppError> {
        let opened = Account::new(server, access)?
            .open_reset(email, code)
            .await?;
        self.inner.lock().await.opened = Some(OpenedReset {
            address: Address {
                server: server.to_owned(),
                access: access.map(str::to_owned),
                email: email.to_owned(),
            },
            ticket: opened.ticket,
            wrapped_mk_recovery: opened.wrapped_mk_recovery,
        });
        Ok(())
    }

    /// Case 2, second request: the recovery key unwraps `MK` here, the new password wraps it, and
    /// the records stay. **The recovery key does not change**: its wrapped copy goes back as it
    /// came. A key that does not open this account leaves the ticket held for another try.
    pub async fn reset_keeping(
        &self,
        recovery_key: &str,
        password: String,
        device_name: &str,
    ) -> Result<Status, AppError> {
        let (address, ticket, wrapped_recovery) = {
            let inner = self.inner.lock().await;
            let opened = inner
                .opened
                .as_ref()
                .ok_or_else(|| err!("error.syncNothingToReset"))?;
            (
                opened.address.clone(),
                opened.ticket.clone(),
                opened.wrapped_mk_recovery.clone(),
            )
        };
        let mut key = crypto::parse_recovery_key(recovery_key)?;
        let unwrapped = crypto::unwrap_master_key(
            &crypto::recovery_wrapping_key(&key),
            &decode(&wrapped_recovery)?,
        )
        .map_err(|_| err!("error.syncRecoveryKeyWrong"));
        key.zeroize();
        let mut master = unwrapped?;
        let fresh = rewrap(password, master).await?;
        let account = Account::new(&address.server, address.access.as_deref())?;
        let reset = account
            .reset_with_ticket(
                &address.email,
                &ticket,
                &NewKeys {
                    a: STANDARD.encode(fresh.a),
                    salt_account: STANDARD.encode(fresh.salt),
                    wrapped_mk_password: STANDARD.encode(&fresh.wrapped),
                    wrapped_mk_recovery: wrapped_recovery,
                },
            )
            .await;
        // A spent ticket is spent whether it worked or expired; anything else may be retried.
        if reset
            .as_ref()
            .map_or_else(|error| error.code == "error.syncResetExpired", |_| true)
        {
            self.inner.lock().await.opened = None;
        }
        reset?;
        let status = self
            .finish(&account, address, &fresh.a, &master, device_name)
            .await;
        master.zeroize();
        status
    }

    /// Case 3, before anything is deleted: a new `MK`, a new recovery key, and the new password's
    /// keys, held. Returns the recovery key, for the same ceremony as registration.
    pub async fn prepare_start_over(
        &self,
        server: &str,
        access: Option<&str>,
        email: &str,
        password: String,
    ) -> Result<String, AppError> {
        let master = crypto::new_master_key();
        let mut recovery = crypto::new_recovery_key();
        let fresh = rewrap(password, master).await?;
        let wrapped_recovery =
            crypto::wrap_master_key(&crypto::recovery_wrapping_key(&recovery), &master)?;
        let shown = crypto::format_recovery_key(&recovery);
        recovery.zeroize();
        self.inner.lock().await.starting_over = Some(StartingOver {
            address: Address {
                server: server.to_owned(),
                access: access.map(str::to_owned),
                email: email.to_owned(),
            },
            a: fresh.a,
            master,
            salt_account: STANDARD.encode(fresh.salt),
            wrapped_mk_password: STANDARD.encode(&fresh.wrapped),
            wrapped_mk_recovery: STANDARD.encode(&wrapped_recovery),
        });
        Ok(shown)
    }

    /// Case 3: spend the code. **Every record on the server is deleted**, every machine signed
    /// out, and this one signs in under the new `MK`. A wrong code leaves everything held.
    pub async fn start_over(&self, code: &str, device_name: &str) -> Result<Status, AppError> {
        let (address, a, master, keys) = {
            let inner = self.inner.lock().await;
            let held = inner
                .starting_over
                .as_ref()
                .ok_or_else(|| err!("error.syncNothingToReset"))?;
            (
                held.address.clone(),
                held.a,
                held.master,
                NewKeys {
                    a: STANDARD.encode(held.a),
                    salt_account: held.salt_account.clone(),
                    wrapped_mk_password: held.wrapped_mk_password.clone(),
                    wrapped_mk_recovery: held.wrapped_mk_recovery.clone(),
                },
            )
        };
        let account = Account::new(&address.server, address.access.as_deref())?;
        account.reset_with_code(&address.email, code, &keys).await?;
        self.inner.lock().await.starting_over = None;
        self.finish(&account, address, &a, &master, device_name)
            .await
    }

    /// Delete this account and everything in it (D4b). **Re-proves the password**: the session
    /// alone is what a borrowed, unlocked machine already has. Every machine is signed out, this
    /// one included, and what is on each of them stays there. Returns how many records went.
    pub async fn delete_account(&self, password: String) -> Result<u64, AppError> {
        let session = self.session(None).await?;
        let email = self
            .inner
            .lock()
            .await
            .saved
            .as_ref()
            .map(|saved| saved.email.clone())
            .ok_or_else(|| err!("error.syncNotSignedIn"))?;
        let params = session.account.params(&email).await?;
        let proven = derive(password, decode(&params.salt_account)?).await?;
        let a = proven.auth;
        let deleted = self
            .with_session(|session| async move {
                session
                    .account
                    .delete_account(&session.access_token, &a)
                    .await
            })
            .await?;
        let mut inner = self.inner.lock().await;
        self.end(&mut inner).await?;
        Ok(deleted.records_deleted)
    }

    /// Whether the account is holding still for a copy, and since when.
    pub async fn freeze_state(&self) -> Result<Freeze, AppError> {
        self.with_session(|session| async move {
            session.account.freeze_state(&session.access_token).await
        })
        .await
    }

    /// End a freeze — a finished copy's, or one nobody is going to finish. Any signed-in machine
    /// may, and nothing else ever will (D4b).
    pub async fn thaw(&self) -> Result<Freeze, AppError> {
        self.with_session(|session| async move {
            session
                .account
                .set_freeze(&session.access_token, false)
                .await
        })
        .await
    }

    /// The signed-in server's closing date (D4b), opening the session if this run has not — so the
    /// window can warn at launch rather than after the first sync. `None` when signed out, and then
    /// nothing is asked.
    pub async fn closing_on(&self) -> Result<Option<i64>, AppError> {
        let signed_in = {
            let mut inner = self.inner.lock().await;
            self.load(&mut inner).await?;
            inner.saved.is_some()
        };
        if !signed_in {
            return Ok(None);
        }
        Ok(self.session(None).await?.limits.closing_on)
    }

    /// Freeze without a move, for `tests/sync_live.rs` alone: in the product a freeze is only ever
    /// step 2 of one.
    #[doc(hidden)]
    pub async fn freeze_for_test(&self) -> Result<Freeze, AppError> {
        self.with_session(|session| async move {
            session
                .account
                .set_freeze(&session.access_token, true)
                .await
        })
        .await
    }

    /// D4b step 1: register on the new server with the same salt and the same two wrapped copies,
    /// so the data crosses unchanged. The new server sends its own letter.
    pub async fn move_begin(
        &self,
        server: &str,
        access: Option<&str>,
        password: String,
    ) -> Result<(), AppError> {
        let session = self.session(None).await?;
        let saved = self
            .inner
            .lock()
            .await
            .saved
            .clone()
            .ok_or_else(|| err!("error.syncNotSignedIn"))?;
        let recovery = saved
            .wrapped_mk_recovery
            .clone()
            .ok_or_else(|| err!("error.syncSignInAgainToMove"))?;
        let params = session.account.params(&saved.email).await?;
        let keys = derive(password, decode(&params.salt_account)?).await?;
        // The password becomes the new server's: checked here, where a typo is still a typo, and
        // not discovered at the last step, when the old account refuses to be deleted (C3).
        session
            .account
            .check(&session.access_token, &keys.auth)
            .await?;
        let wrapped = crypto::wrap_master_key(&keys.wrap, &saved.master_key_bytes()?)?;
        Account::new(server, access)?
            .register(&Registration {
                email: saved.email.clone(),
                a: STANDARD.encode(keys.auth),
                salt_account: params.salt_account,
                argon: Argon::ours(),
                wrapped_mk_password: STANDARD.encode(&wrapped),
                wrapped_mk_recovery: recovery,
            })
            .await?;
        self.inner.lock().await.moving = Some(Moving {
            to: Address {
                server: server.to_owned(),
                access: access.map(str::to_owned),
                email: saved.email.clone(),
            },
            a: keys.auth,
            arrived: None,
        });
        Ok(())
    }

    /// D4b steps 2 to 5's comparison: confirm the new address, sign in there, freeze the old
    /// account, copy every live record, and check that each one arrived. Run again after a failure,
    /// it picks up where it stopped — the copy is resumable, and a spent code is not asked for twice.
    pub async fn move_confirm(&self, code: &str, device_name: &str) -> Result<Moved, AppError> {
        let (to, a, arrived) = {
            let inner = self.inner.lock().await;
            let moving = inner
                .moving
                .as_ref()
                .ok_or_else(|| err!("error.syncNothingToMove"))?;
            (moving.to.clone(), moving.a, moving.arrived.clone())
        };
        let account = Account::new(&to.server, to.access.as_deref())?;
        let arrived = match arrived {
            Some(arrived) => arrived,
            None => {
                account.verify(&to.email, code).await?;
                let signed_in = account.login(&to.email, &a, device_name).await?;
                let arrived = Arrived {
                    access_token: signed_in.access_token.clone(),
                    refresh_token: signed_in.refresh_token.clone(),
                    device_id: signed_in.device_id.clone(),
                    expires_in: signed_in.expires_in,
                    account_id: signed_in.account_id.clone(),
                };
                if let Some(moving) = self.inner.lock().await.moving.as_mut() {
                    moving.arrived = Some(arrived.clone());
                }
                arrived
            }
        };
        let there = Transport::new(&to.server, &arrived.access_token, to.access.as_deref())?;
        let limits = there.capabilities().await?;
        let carried = self
            .with_session(|session| {
                let (there, limits) = (&there, &limits);
                async move {
                    session
                        .account
                        .set_freeze(&session.access_token, true)
                        .await?;
                    copy::copy_account(&session.transport, there, limits).await
                }
            })
            .await?;
        let missing = copy::missing(&there, &carried).await?;
        if missing > 0 {
            return Err(err!("error.syncMoveIncomplete", missing = missing));
        }
        Ok(Moved {
            copied: carried.len(),
        })
    }

    /// D4b step 5's choice: delete the old account (offered first) or thaw it. Either way this
    /// machine now syncs with the new server, under the same `MK`.
    pub async fn move_finish(&self, delete_old: bool) -> Result<Status, AppError> {
        let (to, a, arrived) = {
            let inner = self.inner.lock().await;
            let moving = inner
                .moving
                .as_ref()
                .ok_or_else(|| err!("error.syncNothingToMove"))?;
            let arrived = moving
                .arrived
                .clone()
                .ok_or_else(|| err!("error.syncNothingToMove"))?;
            (moving.to.clone(), moving.a, arrived)
        };
        self.with_session(|session| async move {
            if delete_old {
                session
                    .account
                    .delete_account(&session.access_token, &a)
                    .await
                    .map(drop)
            } else {
                session
                    .account
                    .set_freeze(&session.access_token, false)
                    .await
                    .map(drop)
            }
        })
        .await?;
        let saved = {
            let inner = self.inner.lock().await;
            let old = inner
                .saved
                .as_ref()
                .ok_or_else(|| err!("error.syncNotSignedIn"))?;
            Saved {
                server: to.server.clone(),
                access: to.access.clone(),
                email: to.email.clone(),
                device_id: arrived.device_id.clone(),
                refresh_token: arrived.refresh_token.clone(),
                master_key: old.master_key.clone(),
                wrapped_mk_recovery: old.wrapped_mk_recovery.clone(),
                account_id: Some(arrived.account_id.clone()),
            }
        };
        {
            let mut inner = self.inner.lock().await;
            inner.moving = None;
            self.begin(
                &mut inner,
                saved,
                arrived.access_token.clone(),
                arrived.expires_in,
            )
            .await?;
        }
        self.status().await
    }

    /// Give up: thaw the old account and forget the move. An account already registered on the new
    /// server stays there, with whatever was copied; signing in to it is how to delete it.
    pub async fn move_abandon(&self) -> Result<(), AppError> {
        let had_one = self.inner.lock().await.moving.take().is_some();
        if !had_one {
            return Err(err!("error.syncNothingToMove"));
        }
        self.thaw().await.map(drop)
    }

    /// Sign in with keys this machine already has, and make it the session.
    async fn finish(
        &self,
        account: &Account,
        address: Address,
        a: &[u8; 32],
        master: &[u8; 32],
        device_name: &str,
    ) -> Result<Status, AppError> {
        let signed_in = account.login(&address.email, a, device_name).await?;
        let saved = Saved {
            server: address.server.clone(),
            access: address.access.clone(),
            email: address.email.clone(),
            device_id: signed_in.device_id.clone(),
            refresh_token: signed_in.refresh_token.clone(),
            master_key: STANDARD.encode(master),
            wrapped_mk_recovery: Some(signed_in.wrapped_mk_recovery.clone()),
            account_id: Some(signed_in.account_id.clone()),
        };
        {
            let mut inner = self.inner.lock().await;
            self.begin(
                &mut inner,
                saved,
                signed_in.access_token.clone(),
                signed_in.expires_in,
            )
            .await?;
        }
        self.status().await
    }

    /// Stamp this machine's changes to `collection` before its pages are pulled, so the pull can
    /// weigh them against what it brings (D4). Nothing is sent.
    pub async fn notice(&self, collection: &str, items: Vec<Item>) -> Result<(), AppError> {
        let name = collection.to_owned();
        let items = Arc::new(items);
        self.with_session(|session| {
            let (name, items) = (name.clone(), items.clone());
            async move { lend::notice(&session.store, &session.keys, &name, &items, now()).await }
        })
        .await
    }

    /// Which of `collections` a full run must pull, in one request; every other one's cursor is
    /// moved past this machine's own writes (T189). Plain names in, plain names out.
    pub async fn heads(&self, collections: Vec<String>) -> Result<Vec<String>, AppError> {
        let collections = Arc::new(collections);
        self.with_session(|session| {
            let collections = collections.clone();
            async move {
                let opaque: Vec<String> = collections
                    .iter()
                    .map(|name| crypto::opaque_id(&session.keys.id, name))
                    .collect();
                let stale: HashSet<String> = engine::stale(
                    &session.transport,
                    &session.store,
                    &session.limits,
                    &opaque,
                    APP_VERSION,
                )
                .await?
                .into_iter()
                .collect();
                Ok::<_, AppError>(
                    collections
                        .iter()
                        .zip(&opaque)
                        .filter(|(_, id)| stale.contains(*id))
                        .map(|(name, _)| name.clone())
                        .collect(),
                )
            }
        })
        .await
    }

    /// The next page of `collection`, opened, for its module to write.
    pub async fn pull_page(&self, collection: &str) -> Result<PulledPage, AppError> {
        let name = collection.to_owned();
        let (fetched, opened) = self
            .with_session(|session| {
                let name = name.clone();
                async move {
                    let opaque = crypto::opaque_id(&session.keys.id, &name);
                    let fetched =
                        engine::fetch(&session.transport, &session.store, &opaque, APP_VERSION)
                            .await?;
                    let opened = lend::incoming(
                        &session.store,
                        &session.keys,
                        &name,
                        &session.device_id,
                        &fetched.records,
                        fetched.resync && !fetched.more,
                    )
                    .await?;
                    // Named: an async block that uses `?` cannot infer its error type.
                    Ok::<_, AppError>((fetched, opened))
                }
            })
            .await?;
        let token = token();
        let more = fetched.more;
        self.inner.lock().await.held.insert(
            (Kind::Pull, name),
            Held {
                token: token.clone(),
                records: fetched.records.clone(),
                agreements: opened.agreements,
                fetched: Some(fetched),
                unmet: opened.unmet,
            },
        );
        Ok(PulledPage {
            token,
            changes: opened.changes,
            more,
        })
    }

    /// This machine's items for `collection`, pushed as whatever changed since they were last
    /// agreed. A lost conflict comes back as `replaced`, held for the module to write.
    pub async fn push(
        &self,
        collection: &str,
        items: Vec<Item>,
    ) -> Result<PushedChanges, AppError> {
        self.push_reporting(collection, items, || {}).await
    }

    /// [`push`](Self::push), calling `on_sending` just before anything leaves for the server — and
    /// never when nothing changed, which is most pushes. It is what the window's upload icon waits
    /// for: a push that sends nothing must not look like one that does.
    pub async fn push_reporting(
        &self,
        collection: &str,
        items: Vec<Item>,
        on_sending: impl Fn() + Sync,
    ) -> Result<PushedChanges, AppError> {
        let on_sending = &on_sending;
        let name = collection.to_owned();
        let items = Arc::new(items);
        let (accepted, replaced, records, agreements, error, needs_pull) = self
            .with_session(|session| {
                let (name, items) = (name.clone(), items.clone());
                async move {
                    // After a move the new account's store starts empty; pushed first, every record
                    // the move had copied came back as a conflict this machine won, and was
                    // written again.
                    let opaque = crypto::opaque_id(&session.keys.id, &name);
                    if !session.store.has_pulled(&opaque).await? {
                        return Ok::<_, AppError>((
                            0,
                            Incoming::default(),
                            Vec::new(),
                            Vec::new(),
                            None,
                            true,
                        ));
                    }
                    let (changes, agreed) =
                        lend::outgoing(&session.store, &session.keys, &name, &items, now()).await?;
                    if changes.is_empty() {
                        return Ok((0, Incoming::default(), Vec::new(), Vec::new(), None, false));
                    }
                    on_sending();
                    let pushed = engine::push(
                        &session.transport,
                        &session.store,
                        &session.limits,
                        &session.device_id,
                        changes,
                    )
                    .await?;
                    lend::settle_pushed(&session.store, &session.keys, &name, agreed, &pushed)
                        .await?;
                    // A dead token is refreshed and the push run again (`with_session`); what
                    // landed is agreed above, so the second run does not send it twice.
                    if let Some(error) = pushed
                        .error
                        .as_ref()
                        .filter(|error| error.code == "error.syncSignedOut")
                    {
                        return Err(error.clone());
                    }
                    let opened = lend::incoming(
                        &session.store,
                        &session.keys,
                        &name,
                        &session.device_id,
                        &pushed.superseded,
                        false,
                    )
                    .await?;
                    Ok((
                        pushed.accepted,
                        opened.changes,
                        pushed.superseded,
                        opened.agreements,
                        pushed.error,
                        false,
                    ))
                }
            })
            .await?;
        let token = if records.is_empty() {
            None
        } else {
            let token = token();
            self.inner.lock().await.held.insert(
                (Kind::Push, name),
                Held {
                    token: token.clone(),
                    records,
                    agreements,
                    fetched: None,
                    unmet: Vec::new(),
                },
            );
            Some(token)
        };
        Ok(PushedChanges {
            accepted,
            replaced,
            token,
            error,
            needs_pull,
        })
    }

    pub async fn commit_pull(
        &self,
        collection: &str,
        token: &str,
        skipped: Vec<String>,
    ) -> Result<(), AppError> {
        self.commit(Kind::Pull, collection, token, skipped).await
    }

    pub async fn commit_push(
        &self,
        collection: &str,
        token: &str,
        skipped: Vec<String>,
    ) -> Result<(), AppError> {
        self.commit(Kind::Push, collection, token, skipped).await
    }

    /// The module has written what `token` handed out, except the ids in `skipped`: record it, and
    /// for a page, move the cursor. No request is made.
    async fn commit(
        &self,
        kind: Kind,
        collection: &str,
        token: &str,
        skipped: Vec<String>,
    ) -> Result<(), AppError> {
        let (held, session) = {
            let mut inner = self.inner.lock().await;
            let key = (kind, collection.to_owned());
            let held = match inner.held.remove(&key) {
                Some(held) if held.token == token => held,
                // Handed out before a sign-in, or already recorded: recording it would agree on
                // something this session never read.
                Some(other) => {
                    inner.held.insert(key, other);
                    return Err(err!("error.syncPageStale"));
                }
                None => return Err(err!("error.syncPageStale")),
            };
            let session = inner
                .session
                .clone()
                .ok_or_else(|| err!("error.syncNotSignedIn"))?;
            (held, session)
        };
        lend::land(
            &session.store,
            &session.keys,
            collection,
            &held.records,
            held.agreements,
            &skipped,
            APP_VERSION,
        )
        .await?;
        if let Some(fetched) = &held.fetched {
            let opaque = crypto::opaque_id(&session.keys.id, collection);
            engine::commit(&session.store, &opaque, fetched, &held.unmet).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::saved::InMemory;

    fn state() -> SyncState {
        let path = std::env::temp_dir().join(format!("sync-{}.db", uuid::Uuid::new_v4()));
        SyncState::new(Arc::new(InMemory::default()), Ok(path))
    }

    #[tokio::test]
    async fn nothing_kept_is_signed_out() {
        assert_eq!(state().status().await.unwrap(), Status::default());
    }

    /// Signed out, the loop's thirty-second check costs nothing: no request, a quiet error.
    #[tokio::test]
    async fn signed_out_asks_nobody() {
        let state = state();
        let pulled = state.pull_page("c").await.err().map(|error| error.code);
        let pushed = state.push("c", vec![]).await.err().map(|error| error.code);
        assert_eq!(pulled, Some("error.syncNotSignedIn"));
        assert_eq!(pushed, Some("error.syncNotSignedIn"));
    }

    /// A token nobody handed out — or one from before a sign-in — records nothing.
    #[tokio::test]
    async fn a_token_nobody_handed_out_records_nothing() {
        let error = state()
            .commit_pull("c", "nope", Vec::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "error.syncPageStale");
    }

    #[tokio::test]
    async fn a_code_with_no_registration_waiting_is_refused() {
        let error = state().verify("AAAA-AAAA", "desktop").await.unwrap_err();
        assert_eq!(error.code, "error.syncNothingToVerify");
    }

    #[tokio::test]
    async fn keeping_with_no_ticket_held_is_refused() {
        let error = state()
            .reset_keeping("AAAA", "new".into(), "desktop")
            .await
            .unwrap_err();
        assert_eq!(error.code, "error.syncNothingToReset");
    }

    #[tokio::test]
    async fn starting_over_with_nothing_prepared_is_refused() {
        let error = state()
            .start_over("AAAA-AAAA", "desktop")
            .await
            .unwrap_err();
        assert_eq!(error.code, "error.syncNothingToReset");
    }

    /// Signed out, there is no server to ask about, and nobody is asked.
    #[tokio::test]
    async fn signed_out_has_no_closing_date() {
        assert_eq!(state().closing_on().await.unwrap(), None);
    }
}
