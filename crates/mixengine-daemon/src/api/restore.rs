//! `home.previous` and `home.restore` — a copy of an earlier home's state, roadmap task **T182h**.
//!
//! The merge is core's (`adopt::snapshot::restore`). What this adds is where to look for a copy and
//! what each restored thing needs afterwards: a new admin password for every database (left
//! stopped, as an adopt leaves one), its configuration rendered, the hosts file and certificates
//! asked for the way a site create asks for them, and `bin/`.

use std::path::PathBuf;

use mixengine_core::adopt::snapshot::{self, Previous};
use mixengine_proto::{Error, ErrorCode, HomePrevious, HomeRestoreReport, PreviousCopy, ServiceId};

use super::Api;
use crate::error::ToWire as _;

impl Api {
    /// `home.previous` — the newest copy in a kept folder, while this home has nothing of its own.
    ///
    /// # Errors
    ///
    /// The wire error of a table or a copy that could not be read.
    pub(crate) async fn home_previous(&self) -> Result<HomePrevious, Error> {
        if self.has_things_of_its_own().await? {
            return Ok(HomePrevious { copy: None });
        }

        Ok(HomePrevious {
            copy: self.newest_copy().await?.map(|previous| PreviousCopy {
                path: previous.path.display().to_string(),
                taken_at: previous.taken_at,
                projects: previous.projects,
                sites: previous.sites,
                services: previous.services,
                runtimes: previous.runtimes,
                packages: previous.packages,
                newer: previous.newer,
            }),
        })
    }

    /// `home.restore` — merge the newest copy, then give each restored thing what it needs.
    ///
    /// # Errors
    ///
    /// `not_found` when there is no copy; `precondition_failed` for one this home will not take
    /// (newer, or a home with things of its own); the wire error of a merge that failed, in which
    /// case nothing was kept. A step after the merge that fails is a line in `problems`.
    pub(crate) async fn home_restore(&self) -> Result<HomeRestoreReport, Error> {
        let Some(previous) = self.newest_copy().await? else {
            return Err(Error::new(
                ErrorCode::NotFound,
                "no copy of an earlier home is in the folders this home keeps",
            )
            .with_hint("`mix home previous` says what a restore would bring back"));
        };

        let restored = snapshot::restore(&self.store, &previous.path)
            .await
            .map_err(|error| match error {
                mixengine_core::Error::RestoreRefused { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, error.to_string())
                }
                other => other.to_wire(),
            })?;

        tracing::info!(copy = %previous.path.display(), ?restored, "restored an earlier home");

        let mut problems = Vec::new();

        // Rendered first: it is what tells the registry which restored services keep a password.
        if let Err(error) = self.sites.now_serves_what_it_declares().await {
            problems.push(format!(
                "the restored services could not be configured: {}",
                error.message
            ));
        }

        for id in &restored.services {
            let Ok(service) = ServiceId::parse(id) else {
                continue;
            };
            if let Err(error) = self.new_admin_password(&service).await {
                problems.push(format!(
                    "{service} has no new admin password yet: {}. `mix service reset-credential \
                     {service}` sets one",
                    error.message
                ));
            }
        }

        self.sites.wants_the_hosts_file().await;

        if !restored.https_sites.is_empty()
            && let Err(error) = self.certificates.issue(None).await
        {
            problems.push(format!(
                "the restored sites have no certificates yet: {}. `mix cert issue` tries again",
                error.message
            ));
        }

        if let Err(error) = self.shims.refresh().await {
            problems.push(format!("bin/ could not be refreshed: {}", error.message));
        }

        Ok(HomeRestoreReport {
            projects: restored.projects,
            sites: restored.sites,
            services: u64::try_from(restored.services.len()).unwrap_or(u64::MAX),
            runtimes: restored.runtimes,
            packages: restored.packages,
            skipped: restored.skipped,
            problems,
        })
    }

    /// Whether this home has a project, a site or a package's service of its own — what makes a
    /// restore the wrong thing to offer.
    async fn has_things_of_its_own(&self) -> Result<bool, Error> {
        snapshot::has_things_of_its_own(&self.store)
            .await
            .map_err(|error| error.to_wire())
    }

    /// The newest copy among the folders a copy is written to: `runtimes/`, `packages/`, `data/`.
    async fn newest_copy(&self) -> Result<Option<Previous>, Error> {
        let folders: [PathBuf; 3] = [
            self.paths.runtimes().to_path_buf(),
            self.paths.packages().to_path_buf(),
            self.paths.data().to_path_buf(),
        ];

        let mut newest: Option<Previous> = None;
        for folder in folders {
            let Some(found) = snapshot::read(&folder)
                .await
                .map_err(|error| error.to_wire())?
            else {
                continue;
            };
            if newest
                .as_ref()
                .is_none_or(|kept| found.taken_at > kept.taken_at)
            {
                newest = Some(found);
            }
        }

        Ok(newest)
    }
}
