//! `service.found` and `service.adopt` — service data an earlier home left under `data/`, roadmap
//! task **T182g**.
//!
//! Adopting is two things this daemon already does: `service.create` with the id whose derived
//! data directory is the one found — a directory carrying `.mixengine-ready` skips the first run —
//! and, for a database, `service.reset_credential`, which writes a new admin password into it and
//! generates that password when this home's credential store has none. Nothing here writes to the
//! directory itself.

use mixengine_core::adopt::instances::{self, FoundInstance, Opens};
use mixengine_proto::{
    Error, ErrorCode, ResetCredential, ServiceAdopt, ServiceCreate, ServiceFound, ServiceFoundList,
    ServiceId, ServiceSummary, ServiceTarget,
};

use super::Api;
use crate::error::ToWire as _;

impl Api {
    /// `service.found` — every data directory with no service row, and what would open each.
    ///
    /// # Errors
    ///
    /// The wire error of a table that could not be read.
    pub(crate) async fn service_found(&self) -> Result<ServiceFoundList, Error> {
        // **A restore comes first** (T182h). While a copy of the earlier home can be restored, its
        // data is not offered one directory at a time: adopting a database first would give this
        // home a service of its own, and the restore that brings back the projects and sites would
        // then be refused. A copy this build cannot restore does not hold this back.
        if self
            .home_previous()
            .await?
            .copy
            .is_some_and(|copy| !copy.newer)
        {
            return Ok(ServiceFoundList { found: Vec::new() });
        }

        let found = self.found_with_openers().await?;

        Ok(ServiceFoundList {
            found: found
                .into_iter()
                .filter_map(|(instance, opens)| {
                    let service = service_id(&instance)?;
                    let (opens_with, why_not) = match opens {
                        Opens::With(version) => (Some(version), None),
                        Opens::NotReady => (None, Some(not_ready())),
                        Opens::Needs(what) => (None, Some(needs(&what))),
                    };

                    Some(ServiceFound {
                        service,
                        path: instance.path.display().to_string(),
                        made_by: instance.made_by,
                        opens_with,
                        why_not,
                    })
                })
                .collect(),
        })
    }

    /// `service.adopt` — turn one found data directory back into a stopped service.
    ///
    /// An id that already has a row answers that row, so asking twice is not an error.
    ///
    /// # Errors
    ///
    /// `not_found` for an id `service.found` does not list; `precondition_failed` for one it lists
    /// as not adoptable, with the reason; the errors of creating the service; and, for a database,
    /// those of re-setting its password, after which the service stays and the hint says how to try
    /// the password again.
    pub(crate) async fn service_adopt(
        &self,
        asked: &ServiceAdopt,
    ) -> Result<ServiceSummary, Error> {
        if let Some(summary) = self.summary_of(&asked.service).await? {
            return Ok(summary);
        }

        let found = self.found_with_openers().await?;
        let Some((instance, opens)) = found
            .into_iter()
            .find(|(instance, _)| service_id(instance).as_ref() == Some(&asked.service))
        else {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("no data for {} was left under data/", asked.service),
            )
            .with_hint("`mix service found` lists what can be adopted"));
        };

        let version = match opens {
            Opens::With(version) => version,
            Opens::NotReady => {
                return Err(Error::new(ErrorCode::PreconditionFailed, not_ready()));
            }
            Opens::Needs(what) => {
                return Err(Error::new(ErrorCode::PreconditionFailed, needs(&what)));
            }
        };

        let created = self
            .service_create(&ServiceCreate {
                id: asked.service.clone(),
                version,
                port: None,
                bind_addr: None,
                data_dir: None,
                autostart: None,
                overrides: None,
            })
            .await?;

        tracing::info!(
            service = %asked.service,
            path = %instance.path.display(),
            "adopted service data an earlier home left"
        );

        self.new_admin_password(&asked.service).await?;

        Ok(self
            .summary_of(&asked.service)
            .await?
            .unwrap_or(created.service))
    }

    /// For a service that keeps an admin password, set a new one in its data and leave it stopped —
    /// what an adopt (T182g) and a restore (T182h) both need, since the password the data holds
    /// went with the earlier home's credential store. Nothing for any other service.
    ///
    /// # Errors
    ///
    /// The reset's own, with a hint saying the service and its data stand and how to try again;
    /// and a stop that fails.
    pub(super) async fn new_admin_password(&self, service: &ServiceId) -> Result<(), Error> {
        if !self.services.has_credential_reset(service) {
            return Ok(());
        }

        self.service_reset_credential(&ResetCredential {
            service: service.clone(),
            wait: true,
        })
        .await
        .map_err(|error| {
            error.with_hint(format!(
                "{service} is a service again and its data is untouched; `mix service \
                 reset-credential {service}` sets its admin password"
            ))
        })?;

        // **Left stopped.** A reset starts back the service it repaired — the right answer for a
        // repair of something that was running, and not for one that has only just become a
        // service again. Starting it is a person's call.
        self.service_stop(&ServiceTarget {
            service: Some(service.clone()),
            ..ServiceTarget::default()
        })
        .await?;

        Ok(())
    }

    /// Every found directory, with what would open it.
    async fn found_with_openers(&self) -> Result<Vec<(FoundInstance, Opens)>, Error> {
        let catalogue = crate::services::catalogue();
        let found = instances::found(&self.store, &self.paths, &catalogue)
            .await
            .map_err(|error| error.to_wire())?;
        let installed = mixengine_core::packages::records(&self.store, None)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(found
            .into_iter()
            .map(|instance| {
                let opens = instances::opens(&instance, &installed);
                (instance, opens)
            })
            .collect())
    }

    /// This service's summary, when it has a row.
    async fn summary_of(&self, id: &ServiceId) -> Result<Option<ServiceSummary>, Error> {
        Ok(self
            .service_list()
            .await?
            .services
            .into_iter()
            .find(|summary| &summary.id == id))
    }
}

/// The id a found directory would be declared under: `mariadb@main`, or `caddy` for a server that
/// exists once.
fn service_id(instance: &FoundInstance) -> Option<ServiceId> {
    let id = match instance.package == instance.instance {
        true => instance.package.clone(),
        false => format!("{}@{}", instance.package, instance.instance),
    };

    ServiceId::parse(&id).ok()
}

fn not_ready() -> String {
    "its first run never finished, so there is no database in it to keep".to_owned()
}

fn needs(what: &str) -> String {
    format!("install {what} to open it: `mix package install` with that version")
}
