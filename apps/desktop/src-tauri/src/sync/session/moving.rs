//! Moving the account to another server, and giving a move up (D4b).

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

use super::{decode, derive, Address, Arrived, Moved, Moving, Status, SyncState};
use crate::error::AppError;
use crate::sync::account::{Account, Argon, Registration};
use crate::sync::copy;
use crate::sync::crypto;
use crate::sync::saved::Saved;
use crate::sync::transport::Transport;

impl SyncState {
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
}
