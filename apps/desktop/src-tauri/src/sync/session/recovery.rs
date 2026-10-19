//! Starting again without the password: a reset kept by the recovery key, or a start over.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use zeroize::Zeroize;

use super::{decode, rewrap, Address, OpenedReset, StartingOver, Status, SyncState};
use crate::error::AppError;
use crate::sync::account::{Account, NewKeys};
use crate::sync::crypto;

impl SyncState {
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
}
