//! Removing an installed runtime, and refusing to while something still uses it.

use std::path::Path;

use mixengine_core::runtimes;
use mixengine_core::upgrade::kept::{self, Family};
use mixengine_proto::{Error, ErrorCode, RuntimeRemoval, RuntimeUninstall, ServiceState};

use crate::error::ToWire as _;

use super::Runtimes;

impl Runtimes {
    /// `runtime.uninstall` — remove the directory, then the row.
    ///
    /// **In that order**, which is [`mixengine_core::runtimes`]' rule read backwards: a directory
    /// that could not be removed leaves a row that still describes it, and asking again repeats
    /// exactly this. The reverse would leave a runtime on disk that nothing knows about.
    ///
    /// **Two refusals, and `force` crosses exactly one of them.** A project whose pin this removal
    /// would leave with no answer is a statement about the future — the next `cd` into that
    /// directory fails with a message naming the install that fixes it — so somebody who has been
    /// shown the projects and typed `--force` has made a decision they are entitled to make. A
    /// running php-fpm pool is a fact about the present, and no flag buys a live process with no
    /// files under it. That asymmetry is decided here rather than by the schema: a **stopped** pool
    /// is deleted along with the runtime, deliberately, so the `ON DELETE RESTRICT` on
    /// `services.runtime_install_id` is never reached.
    ///
    /// **Pools, plural.** A `web-app` extension owns a second pool on the same installed PHP as the
    /// shared one, so every sentence above holds for a set: any of them running refuses the whole
    /// removal, and all the stopped ones go with the runtime.
    ///
    /// # Errors
    ///
    /// `not_found` when it is not installed; `precondition_failed` when a registered project pins
    /// it and `force` was not asked for, and when the pool that runs out of it has not been
    /// stopped; and the wire error of a directory that could not be removed — on Windows, most
    /// often a process still running out of it.
    pub(crate) async fn uninstall(
        &self,
        asked: &RuntimeUninstall,
    ) -> Result<RuntimeRemoval, Error> {
        let target = &asked.target;

        let removed = runtimes::record(&self.store, target.kind, &target.version)
            .await
            .map_err(|error| error.to_wire())?;

        // **Every pool this runtime is running, not one of them.** A `web-app` extension owns a
        // second pool on the same install, and seeing one of two would delete the shared one, leave
        // the extension's, and then fail on the `ON DELETE RESTRICT` with a message about a foreign
        // key.
        let pools =
            mixengine_core::services::pools::of_runtime(&self.store, target.kind, &target.version)
                .await
                .map_err(|error| error.to_wire())?;

        // Cheaper than the pool check below and asked first for that reason: two reads of tables
        // this home owns, against a `services` row and the state of a process.
        if !asked.force {
            let broken =
                mixengine_core::projects::pins_broken_by(&self.store, target.kind, &target.version)
                    .await
                    .map_err(|error| error.to_wire())?;

            // **An extension's site frozen on this PHP joins the refusal.** A pin is a promise
            // about a project's future; a web-app's pool is a fact about what is served now, and
            // losing it silently was the state before this line.
            let mut frozen = Vec::new();

            for pool in &pools {
                frozen.extend(
                    mixengine_core::sites::frozen_on(&self.store, pool)
                        .await
                        .map_err(|error| error.to_wire())?,
                );
            }

            if !broken.is_empty() || !frozen.is_empty() {
                let named = broken
                    .iter()
                    .map(|pin| format!("{} ({})", pin.project, pin.constraint))
                    .chain(frozen.iter().map(|id| format!("{id} (extension)")))
                    .collect::<Vec<_>>()
                    .join(", ");

                return Err(Error::new(
                    ErrorCode::PreconditionFailed,
                    format!(
                        "removing {} {} would leave nothing for {named}",
                        target.kind, target.version
                    ),
                )
                .with_hint(
                    "install another version that answers the pin, change the pin, or `--force` \
                     to remove it anyway",
                ));
            }
        }

        // A PHP whose pool is running is a PHP something is serving sites out of, and removing the
        // directory under it would leave a process with no files and a row naming a runtime that is
        // gone.
        // **Every one of them is asked before any of them is deleted.** A walk that deleted as it
        // went would leave a home with the shared pool gone and the extension's still running, over
        // a refusal that was going to be raised anyway.
        for service in &pools {
            let record = mixengine_core::services::record(&self.store, service)
                .await
                .map_err(|error| error.to_wire())?;

            if !matches!(record.state, ServiceState::Stopped | ServiceState::Failed) {
                return Err(Error::new(
                    ErrorCode::PreconditionFailed,
                    format!("{service} is {}", record.state.as_str()),
                )
                .with_hint(format!("`mix service stop {service}` first")));
            }
        }

        for service in &pools {
            // The row goes before the directory, which is the reverse of the rule the directory
            // follows — and is right for the same reason: a `services` row whose runtime is gone is
            // a row every `service.*` call fails on, where a directory with no row is invisible.
            mixengine_core::services::delete(&self.store, service)
                .await
                .map_err(|error| error.to_wire())?;

            tracing::info!(%service, "a pool was removed with the runtime it ran out of");

            // And its activator with it — the same hole `extension.uninstall` had.
            crate::services::activate::release(service).await;
        }

        runtimes::discard(Path::new(&removed.path))
            .await
            .map_err(|error| error.to_wire())?;

        // **And the language's own folder with its last version.**
        if let Some(parent) = Path::new(&removed.path).parent() {
            mixengine_core::paths::remove_if_empty(parent);
        }

        // The second directory an uninstall owns, beside the pool's `etc/<service-id>/`.
        if let Err(error) =
            mixengine_core::runtimes::extensions::discard(&self.paths, target.kind, &target.version)
                .await
        {
            tracing::warn!(
                %error,
                "the runtime is gone and its generated conf.d could not be removed"
            );
        }

        let default_cleared = runtimes::forget(&self.store, target.kind, &target.version)
            .await
            .map_err(|error| error.to_wire())?;

        // A copy installed again later starts with no history of an update it was kept beside.
        // The runtime is gone either way, so a note that cannot be cleared is a warning.
        if let Err(error) = kept::forget(
            &self.store,
            Family::Runtime,
            target.kind.as_str(),
            target.version.as_str(),
        )
        .await
        {
            tracing::warn!(%error, "could not forget the update this runtime was kept beside");
        }

        tracing::info!(
            kind = target.kind.as_str(),
            version = target.version.as_str(),
            default_cleared,
            "a runtime was uninstalled"
        );

        self.refresh_bin().await;

        Ok(RuntimeRemoval {
            removed,
            default_cleared,
        })
    }
}
