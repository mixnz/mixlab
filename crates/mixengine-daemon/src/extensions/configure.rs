//! Configuring an installed extension: its settings, the pools it needs, what it runs, and the
//! secrets it keeps.

use std::sync::Arc;

use mixengine_core::extensions::{store as extension_store, uninstall};
use mixengine_proto::{Error, ErrorCode, ExtensionId, ServiceId};

use super::{CONFIG_SECRET_KEY, EXTENSION_SECRET_PREFIX};
use super::{CONFIG_SECRET_LENGTH, Extensions, needs_a_database};
use crate::error::ToWire as _;

impl Extensions {
    /// Write every installed `web-app`'s generated configuration — roadmap task **T82**, the
    /// design's D2.
    ///
    /// **Called at boot as well as after an install**, which is what makes the file disposable
    /// rather than something written once and hoped about: a database that was re-provisioned, a
    /// port that moved and a MixEngine that changed its mind about the generated half all take
    /// effect on the next start, with no repair anybody has to know to run.
    ///
    /// **One extension's failure costs that extension and nothing else.** A boot that stopped at the
    /// first unwritable directory would leave every later `web-app` unconfigured for a reason
    /// belonging to a different one — the shape `mix extension available` already takes for an entry
    /// it cannot read.
    ///
    /// # Errors
    ///
    /// None: everything that goes wrong is logged against the extension it belongs to. The install
    /// path uses [`configure_one`](Self::configure_one), which does report.
    pub(crate) async fn configure(&self) {
        let installed = match extension_store::all(&self.store).await {
            Ok(installed) => installed,
            Err(error) => {
                tracing::warn!(%error, "the installed extensions could not be read");
                return;
            }
        };

        for one in &installed {
            if let Err(error) = self.configure_one(one).await {
                tracing::warn!(
                    extension = %one.id,
                    error = %error.message,
                    "an extension's configuration could not be written"
                );
            }
        }
    }

    /// Give every `web-app` a pool of its own, and regenerate if that changed anything — roadmap
    /// task **T82a**, its design's D10.
    ///
    /// **Called at boot, before [`configure`](Self::configure)**, so the configuration written there
    /// belongs to the pool the site is actually served on. Idempotent, which is what lets it run at
    /// every boot: it is one query on a home with no extension sites.
    ///
    /// **Reported and never fatal**, on the rule `configure` follows: a `web-app` that could not be
    /// moved onto its own pool is one command away from being reinstalled, where refusing to start
    /// would leave the user with no daemon at all.
    pub(crate) async fn ensure_pools(&self) {
        let made = match mixengine_core::extensions::pools::ensure(&self.store, self.host.as_ref())
            .await
        {
            Ok(made) => made,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "could not give every web-app extension a pool of its own"
                );
                return;
            }
        };

        if made.is_empty() {
            return;
        }

        // The site rows moved, so what the front end is serving names a pool that is no longer
        // theirs until this runs.
        if let Err(error) = self.sites.now_serves_what_it_declares().await {
            tracing::warn!(
                error = %error.message,
                "web-app extensions were moved onto pools of their own and the front end was not \
                 told; the next `mix site` call renders it"
            );
        }
    }

    /// Give a pool this install just created the address a request wakes it through — roadmap task
    /// **T200**, its design's D1.
    ///
    /// **`runtime.install`'s two repairs, in its order** (T72a): `activation::ensure` allocates the
    /// activation port a TCP pool needs, and `hold_all` binds every activator not yet held. Without
    /// them the pool gets both only at the next daemon start, and until then its site answers 502
    /// behind MixEngine's starting page — which is how a person met it.
    ///
    /// **Reported and never fatal**, on T72a's reasoning: the next daemon start makes both repairs
    /// again, where failing the install would leave a downloaded extension half-written.
    pub(super) async fn make_pool_reachable(&self) {
        if let Err(error) = mixengine_core::services::activation::ensure(
            &self.store,
            self.host.as_ref(),
            &crate::services::catalogue(),
        )
        .await
        {
            tracing::warn!(
                %error,
                "a web-app's pool could not be given an activation port; the next daemon start \
                 will try again"
            );
        }

        match crate::services::activate::hold_all(
            Arc::clone(&self.services),
            &self.paths,
            &self.store,
            self.host.as_ref(),
        )
        .await
        {
            Ok(held) if held.is_empty() => {}
            Ok(held) => tracing::info!(services = ?held, "a request can now start these services"),
            Err(error) => tracing::warn!(
                %error,
                "a web-app's pool cannot be started by a request until this daemon is restarted"
            ),
        }
    }

    /// Stop everything an extension runs before its rows are deleted: a `service`'s own process,
    /// and a `web-app`'s pool — roadmap tasks **T82a** (the pool, its design's D11) and **T200**
    /// (the service).
    ///
    /// **The service is T200's, found by uninstalling a running Mailpit.** Only the pool used to be
    /// stopped, so a running service had its row deleted from under it, went on holding its own
    /// executable, and the install directory could not be removed — on Windows `Access is denied`,
    /// and the next install refused because the directory was still there.
    ///
    /// # Errors
    ///
    /// [`Self::stop_one`]'s, and the wire error of a row that cannot be read.
    pub(super) async fn stop_what_it_runs(&self, id: &ExtensionId) -> Result<(), Error> {
        let service = extension_store::get(&self.store, id)
            .await
            .map_err(|error| error.to_wire())?
            .as_ref()
            .and_then(uninstall::service_of);

        let pool = mixengine_core::extensions::pools::of(&self.store, id)
            .await
            .map_err(|error| error.to_wire())?;

        for running in service.iter().chain(pool.iter()) {
            self.stop_one(running).await?;
        }

        Ok(())
    }

    /// Stop one service an extension runs, and refuse if it did not stop.
    ///
    /// **A refusal and not a best effort.** `services::delete` does not look at a process, so a row
    /// removed from under a live process would leave it with no configuration and nothing that
    /// knows about it — which is exactly what `service.delete`'s own first refusal exists to
    /// prevent, said here for the caller that does not go through it.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::PreconditionFailed`] naming the service and the command that stops it, when the
    /// stop did not take.
    async fn stop_one(&self, pool: &ServiceId) -> Result<(), Error> {
        let pool = pool.clone();

        if let Ok(graph) = self.services.graph().await
            && let Ok(plan) = graph.stop_plan(std::slice::from_ref(&pool))
        {
            self.services.stop(&plan).await;
        }

        let record = mixengine_core::services::record(&self.store, &pool)
            .await
            .map_err(|error| error.to_wire())?;

        if self.services.supervised().contains(&pool)
            || !matches!(
                record.state,
                mixengine_proto::ServiceState::Stopped | mixengine_proto::ServiceState::Failed
            )
        {
            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                format!("{pool} is {}", record.state.as_str()),
            )
            .with_hint(format!("`mix service stop {pool}` first")));
        }

        Ok(())
    }

    /// Rewrite every installed runtime's generated `conf.d` — roadmap task **T82**.
    ///
    /// The call `main.rs` makes at boot, made again where an extension changes what belongs in it.
    /// **Reported and never fatal**, on the rule that call follows: an install that landed and then
    /// could not write one ini file is not an install to undo, and the next start rewrites it.
    pub(super) async fn refresh_ini_sets(&self) {
        match mixengine_core::runtimes::extensions::refresh_all(&self.store, &self.paths).await {
            Ok(moved) if moved.is_empty() => {
                tracing::debug!("every installed runtime's conf.d was already up to date");
            }
            Ok(moved) => tracing::info!(runtimes = ?moved, "rewrote the generated conf.d"),
            Err(error) => {
                tracing::warn!(%error, "could not rebuild every installed runtime's conf.d");
            }
        }
    }

    /// Write one, or say why it was skipped.
    ///
    /// **A `web-app` whose declared database is gone is skipped and its file left alone** — the
    /// design's D4. That state is reachable only through `mix service delete <db> --force`, a person
    /// overruling the refusal the link armed; rewriting the configuration to point nowhere would
    /// make a forced delete worse in silence, and reading the old value back out of the file would
    /// be parsing a generated file into state.
    pub(super) async fn configure_one(
        &self,
        installed: &extension_store::Installed,
    ) -> Result<(), Error> {
        let secret = self.config_secret(&installed.id).await?;

        let rendered = mixengine_core::extensions::config::of(&self.store, installed, &secret)
            .await
            .map_err(|error| error.to_wire())?;

        let Some(rendered) = rendered else {
            if needs_a_database(installed) {
                tracing::warn!(
                    extension = %installed.id,
                    "its database is gone, so its configuration was left as it is; \
                     `mix service create` puts one back"
                );
            }

            return Ok(());
        };

        mixengine_core::extensions::config::write(&rendered).map_err(|error| error.to_wire())
    }

    /// Where this extension's `{secret}` lives — roadmap task **T126**.
    ///
    /// One composition, three callers (read, write, forget), because an address spelled twice is an
    /// address that drifts — and a `forget` that drifted would leave the entry behind for ever.
    async fn secret_address(&self, id: &ExtensionId) -> Result<String, Error> {
        let home = mixengine_core::home::id(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(format!(
            "{home}/{EXTENSION_SECRET_PREFIX}{id}/{CONFIG_SECRET_KEY}"
        ))
    }

    /// The stable random value behind `{secret}`, created on first use — the design's D7.
    ///
    /// **It has to be stable**: phpMyAdmin's `blowfish_secret` is what its session cookie is signed
    /// with, so a value that changed on every render would log everybody out on every regeneration.
    /// It cannot be recovered by reading the last generated file either — *never parse a generated
    /// file back into state* is the rule the whole `etc/` layout rests on — so it lives where this
    /// system keeps a secret, and `mixengine-core` never sees the keyring
    /// (`generate::databases`' D1).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Internal`] when this machine has a credential store and it refused. A machine
    /// with **no** store answers an empty secret rather than failing: a `web-app` that uses no
    /// `{secret}` would otherwise be unconfigurable on a headless Linux for a value it never asked
    /// for, and one that *does* use it is refused by the renderer, naming the field.
    async fn config_secret(&self, id: &ExtensionId) -> Result<String, Error> {
        let host = Arc::clone(&self.host);
        let key = self.secret_address(id).await?;

        // The keyring blocks, and on Linux it blocks on a D-Bus round trip to a daemon that may be
        // prompting somebody to unlock it. `docs/standards/rust.md`'s rule for anything that can
        // hang.
        let read = {
            let (host, key) = (Arc::clone(&host), key.clone());
            tokio::task::spawn_blocking(move || {
                // Through `crate::secrets` — roadmap task **T126** — so that an entry written
                // before addresses named their home is still found, and moved.
                crate::secrets::secret_blocking(
                    host.as_ref(),
                    mixengine_platform::KEYRING_SERVICE,
                    &key,
                )
            })
            .await
        };

        match read {
            Ok(Ok(Some(secret))) => return Ok(secret),
            Ok(Ok(None)) => {}
            // No credential store is not a failure to configure: see this function's own note.
            Ok(Err(reason)) => {
                tracing::debug!(%id, error = %reason, "no credential store answered");
                return Ok(String::new());
            }
            Err(_) => {
                return Err(Error::new(
                    ErrorCode::Internal,
                    format!("the task reading {id}'s configuration secret did not finish"),
                ));
            }
        }

        let made = mixengine_platform::generate_secret(CONFIG_SECRET_LENGTH).map_err(|reason| {
            Error::new(
                ErrorCode::Internal,
                format!("this machine would not produce a random value: {reason}"),
            )
        })?;

        let stored = {
            let (host, key, value) = (host, key, made.clone());
            tokio::task::spawn_blocking(move || {
                host.keyring()
                    .set_secret(mixengine_platform::KEYRING_SERVICE, &key, &value)
            })
            .await
        };

        match stored {
            Ok(Ok(())) => Ok(made),
            Ok(Err(reason)) => {
                tracing::debug!(%id, error = %reason, "no credential store took the secret");
                Ok(String::new())
            }
            Err(_) => Err(Error::new(
                ErrorCode::Internal,
                format!("the task storing {id}'s configuration secret did not finish"),
            )),
        }
    }

    /// Forget the `{secret}` of an extension that is gone.
    ///
    /// Otherwise a reinstall inherits a key from something that was removed, and the entry outlives
    /// everything it was for. Idempotent, and a machine with no credential store has nothing to
    /// remove — both of which are why nothing here is reported.
    pub(super) async fn forget_config_secret(&self, id: &ExtensionId) {
        let host = Arc::clone(&self.host);
        let Ok(key) = self.secret_address(id).await else {
            tracing::debug!(%id, "this home has no id, so an extension's secret cannot be named");
            return;
        };

        let removed = tokio::task::spawn_blocking(move || {
            host.keyring()
                .forget_secret(mixengine_platform::KEYRING_SERVICE, &key)
        })
        .await;

        if let Ok(Err(reason)) = removed {
            tracing::debug!(%id, error = %reason, "an extension's secret could not be removed");
        }
    }
}
