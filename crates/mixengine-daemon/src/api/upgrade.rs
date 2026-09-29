//! `runtime.upgrade*` and `package.upgrade*` — roadmap tasks **T193b** and **T193c**.
//!
//! Beside [`super::front_end`] and built the same way: an update is a stop, a row change, a render
//! and a start, and its difficulty is what happens when one of them does not work. The rows are
//! [`mixengine_core::upgrade`]'s; the order, the processes and the plan are here.
//!
//! **Shared by both namespaces is only which release `to` is, and who is already moving what.**
//! Everything else differs enough that one walk for both would be two walks behind a `match`.

mod package;
mod runtime;

use std::future::Future;
use std::sync::Arc;

use mixengine_core::lines;
use mixengine_proto::{
    Error, ErrorCode, JobKind, JobSummary, PackageVersion, Requirement, ServiceId, ServiceState,
    UpgradeOutcome, UpgradePlan,
};

use super::Api;
use crate::error::ToWire as _;
use crate::jobs::JobHandle;
use crate::runtimes::Fetcher;

/// Which release an update goes to, and what the index says about it.
#[derive(Debug, Clone)]
pub(super) struct Resolved {
    pub(super) to: PackageVersion,
    pub(super) to_installed: bool,
    pub(super) bytes: u64,
    pub(super) stale: bool,
    pub(super) needs: Vec<Requirement>,
    /// What `to`'s build ships, for a runtime whose extension choices travel (PHP).
    pub(super) extensions: Option<mixengine_core::index::Extensions>,
}

/// A service as an update found it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Standing {
    pub(super) running: bool,
    pub(super) person_stopped: bool,
}

/// Decide `to`, or refuse — D4.
///
/// # Errors
///
/// `invalid_argument` when `from` is not installed, when `to` is in another line or not newer,
/// and when `from` is already its line's newest; `not_found` when nobody offers a named `to`; the
/// wire error of an index that could not be obtained at all.
pub(super) async fn resolve(
    fetcher: &Fetcher,
    name: &str,
    installed: &[PackageVersion],
    from: &PackageVersion,
    to: Option<&PackageVersion>,
) -> Result<Resolved, Error> {
    if !installed.contains(from) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{name} {from} is not installed, so there is nothing to update"),
        ));
    }

    let catalogue = fetcher
        .index
        .catalogue()
        .await
        .map_err(|error| error.to_wire())?;
    let offered: Vec<PackageVersion> = catalogue
        .index
        .installable(name)
        .filter_map(|package| PackageVersion::parse(package.version.clone()).ok())
        .collect();

    let to = match to {
        Some(to) => {
            if !lines::same_line(name, from, to) {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "{name} {to} is in line {} and {from} is in line {}; moving between lines \
                         is a switch, not an update",
                        lines::line_of(name, to),
                        lines::line_of(name, from)
                    ),
                ));
            }
            if !to.cmp_precedence(from).is_gt() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{name} {to} is not newer than {from}"),
                ));
            }
            if !offered.contains(to) && !installed.contains(to) {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    format!("the package index offers no {name} {to} for this machine"),
                ));
            }
            to.clone()
        }
        None => lines::update_for(name, from, offered.iter()).ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "{name} {from} is already the newest release of its line, {}",
                    lines::line_of(name, from)
                ),
            )
        })?,
    };

    let to_installed = installed.contains(&to);
    let chosen = catalogue.index.artifact(name, to.as_str());

    Ok(Resolved {
        bytes: match to_installed {
            true => 0,
            false => chosen.as_ref().map_or(0, |chosen| chosen.artifact.size),
        },
        extensions: chosen.map(|chosen| chosen.artifact.extensions.clone()),
        needs: match to_installed {
            true => Vec::new(),
            false => crate::requirements::of(
                &catalogue.index,
                name,
                to.as_str(),
                &crate::requirements::facts(),
            ),
        },
        stale: catalogue.freshness.is_stale(),
        to_installed,
        to,
    })
}

/// Every entry still `planned` becomes `outcome`.
pub(super) fn settle(plan: &mut UpgradePlan, outcome: &UpgradeOutcome) {
    for entry in &mut plan.entries {
        if entry.outcome == (UpgradeOutcome::Planned {}) {
            entry.outcome = outcome.clone();
        }
    }
}

/// A plan as a job's result.
fn encoded(plan: &UpgradePlan) -> Result<serde_json::Value, Error> {
    serde_json::to_value(plan).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!("an upgrade plan could not be encoded: {error}"),
        )
    })
}

impl Api {
    /// `conflict` while an upgrade job is moving from or to `name version`.
    ///
    /// # Errors
    ///
    /// `conflict`, naming the job.
    pub(super) async fn not_being_upgraded(
        &self,
        name: &str,
        version: &PackageVersion,
    ) -> Result<(), Error> {
        match self
            .upgrading
            .lock()
            .await
            .get(&(name.to_owned(), version.clone()))
        {
            None => Ok(()),
            Some(job) => Err(Error::new(
                ErrorCode::Conflict,
                format!("job {job} is updating {name} {version}"),
            )
            .with_hint(format!("`mix job wait {job}`, then ask again"))),
        }
    }

    /// Whether a service is running now, and whether a person stopped it.
    pub(super) async fn standing_of(&self, id: &ServiceId) -> Result<Standing, Error> {
        let record = mixengine_core::services::record(&self.store, id)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(Standing {
            running: self.services.supervised().contains(id)
                || matches!(
                    record.state,
                    ServiceState::Running
                        | ServiceState::Starting
                        | ServiceState::Restarting
                        | ServiceState::Degraded
                ),
            person_stopped: record.stopped_by == mixengine_core::services::StoppedBy::Person,
        })
    }

    /// Start an upgrade job, or answer with the one already moving `from`.
    ///
    /// **Held across the start**, for `Runtimes::install`'s reason: "is one running" and "start
    /// one" are one decision.
    async fn begin_upgrade<F, Fut>(
        self: &Arc<Self>,
        method: &str,
        name: &str,
        from: &PackageVersion,
        to: &PackageVersion,
        work: F,
    ) -> Result<JobSummary, Error>
    where
        F: FnOnce(JobHandle) -> Fut + Send + 'static,
        Fut: Future<Output = Result<UpgradePlan, Error>> + Send + 'static,
    {
        let mut upgrading = self.upgrading.lock().await;
        let from_key = (name.to_owned(), from.clone());
        let to_key = (name.to_owned(), to.clone());

        if let Some(job) = upgrading.get(&from_key).copied() {
            return self.jobs.status(job).await;
        }
        if let Some(job) = upgrading.get(&to_key).copied() {
            return Err(Error::new(
                ErrorCode::Conflict,
                format!("job {job} is already updating to or from {name} {to}"),
            ));
        }

        let api = Arc::clone(self);
        let keys = (from_key.clone(), to_key.clone());
        let started = self
            .jobs
            .begin(
                &JobKind::parse(method).expect("a method name is a job kind"),
                move |handle| async move {
                    let outcome = work(handle).await.and_then(|plan| encoded(&plan));

                    // Released here for `Runtimes::install`'s reason: this future owns the job, and
                    // the caller holds the lock until after the insert below.
                    let mut upgrading = api.upgrading.lock().await;
                    upgrading.remove(&keys.0);
                    upgrading.remove(&keys.1);

                    outcome
                },
            )
            .await?;

        upgrading.insert(from_key, started.id);
        upgrading.insert(to_key, started.id);

        Ok(started)
    }
}
