//! Installing a runtime: the job, the download and unpacking, and what is recorded after it.

use std::sync::Arc;
use std::time::SystemTime;

use mixengine_core::{RuntimeError, paths, runtimes};
use mixengine_proto::{
    Error, ErrorCode, JobKind, JobSummary, PackageVersion, RuntimeInstall, RuntimeKind,
    RuntimeSummary, RuntimeTarget, Timestamp, rpc,
};

use crate::error::ToWire as _;
use crate::jobs::JobHandle;
use crate::requirements;

use super::{Runtimes, emulation_notice, offered, subject_of};

impl Runtimes {
    /// `runtime.install` — start the download, and answer with the job doing it.
    ///
    /// Three things are decided before a job exists, because each is an answer a caller should have
    /// immediately rather than through a job that fails a moment later: a version that is already
    /// installed, a version this daemon is already installing, and — since T149 — a version this
    /// machine certainly lacks something for.
    ///
    /// # Errors
    ///
    /// `already_exists` when it is installed, `dependency_missing` when this machine lacks what it
    /// needs, and the wire error of a row that could not be read or a job that could not be started.
    pub(crate) async fn install(
        self: &Arc<Self>,
        asked: &RuntimeInstall,
    ) -> Result<JobSummary, Error> {
        let target = &asked.target;

        // Held across the whole of this, so that "is one running" and "start one" are one decision.
        let mut running = self.running.lock().await;

        let key = (target.kind, target.version.clone());
        if let Some(job) = running.get(&key).copied() {
            tracing::debug!(
                kind = target.kind.as_str(),
                version = target.version.as_str(),
                %job,
                "an install of this version is already running; answering with its job"
            );
            return self.jobs.status(job).await;
        }

        match runtimes::record(&self.store, target.kind, &target.version).await {
            // **A recorded version whose folder is gone is installed again** — roadmap task
            // **T206d**, D3: the install restores the row it already has.
            Ok(found) if found.is_missing() => {}
            Ok(_) => {
                return Err(
                    mixengine_core::Error::Runtime(RuntimeError::AlreadyRecorded {
                        kind: target.kind,
                        version: target.version.clone(),
                    })
                    .to_wire(),
                );
            }
            Err(mixengine_core::Error::NotFound { .. }) => {}
            Err(error) => return Err(error.to_wire()),
        }

        if !asked.ignore_requirements {
            requirements::gate(
                &self.fetcher,
                target.kind.as_str(),
                target.version.as_str(),
                &subject_of(target),
                asked.install_prerequisites,
            )
            .await?;
        }

        let kind = JobKind::parse(rpc::method::RUNTIME_INSTALL)
            .expect("`runtime.install` is a method name, which is what a job kind is");

        let runtimes = Arc::clone(self);
        let asked = asked.clone();
        let started = self
            .jobs
            .begin(&kind, move |handle| async move {
                // **What the person agreed to happens first** — roadmap task **T150**. Inside the job,
                // because it is a download and an installer; before `perform`, because nothing of
                // the runtime may be fetched for a machine that still cannot run it.
                let prepared = match asked.install_prerequisites && !asked.ignore_requirements {
                    true => {
                        requirements::prepare(
                            &runtimes.fetcher,
                            asked.target.kind.as_str(),
                            asked.target.version.as_str(),
                            &subject_of(&asked.target),
                            &handle,
                        )
                        .await
                    }
                    false => Ok(()),
                };

                let outcome = match prepared {
                    Ok(()) => runtimes.perform(&asked.target, &handle).await,
                    Err(refused) => Err(refused),
                };

                // Released here rather than by the caller: this future is what owns the install, and
                // it ends by being cancelled as well as by returning. It cannot run ahead of the
                // insert below — the caller holds this same lock until after it — so a job that
                // finishes in the instant it is spawned still leaves the map empty rather than
                // removing a key that has not been added.
                runtimes
                    .running
                    .lock()
                    .await
                    .remove(&(asked.target.kind, asked.target.version));

                outcome
            })
            .await?;

        running.insert(key, started.id);

        Ok(started)
    }

    /// The work behind an install: look it up, fetch it, write it down.
    ///
    /// The three steps are in this order for a reason each: the lookup is refused before a byte is
    /// downloaded, the download is a transaction whose commit is a rename
    /// ([`mixengine_core::install`]), and the row is written **after** that rename — so a failure
    /// anywhere leaves either nothing or a directory with no row, and never a row describing a
    /// runtime that is not there.
    pub(crate) async fn perform(
        &self,
        target: &RuntimeTarget,
        handle: &JobHandle,
    ) -> Result<serde_json::Value, Error> {
        let (kind, version) = (target.kind, &target.version);
        tracing::info!(job = %handle.id(), kind = kind.as_str(), version = version.as_str(), "installing a runtime");

        handle.progress(0, "reading the package index").await;
        let catalogue = self
            .fetcher
            .index
            .kinds(&[kind.as_str()])
            .await
            .map_err(|error| error.to_wire())?;
        let (package, selection) = offered(
            &catalogue.index,
            kind.as_str(),
            version.as_str(),
            &format!("mix runtime available --kind {kind}"),
        )?;

        if let Some(notice) = emulation_notice(kind.as_str(), version.as_str(), selection) {
            handle.progress(0, &notice).await;
        }

        // **What this machine may lack, said and not refused** — roadmap task T27e, D16. The gate
        // in front of this job already let it through; what is left is to name the sonames, in the
        // job's own progress so every client sees them and in the log for a support conversation.
        let advisories = mixengine_core::requirements::advisories(&requirements::of(
            &catalogue.index,
            kind.as_str(),
            version.as_str(),
            &requirements::facts(),
        ));
        if !advisories.is_empty() {
            let named: Vec<String> = advisories
                .iter()
                .map(|requirement| requirement.need.label())
                .collect();
            let notice = format!(
                "this machine's loader does not list {}; install them with this distribution's \
                 package manager; the install goes on",
                named.join(", ")
            );

            tracing::warn!(kind = kind.as_str(), version = version.as_str(), "{notice}");
            handle.progress(0, &notice).await;
        }

        let into = runtimes::directory(&self.paths, kind, version);
        if let Some(parent) = into.parent() {
            paths::create_dir(parent).map_err(|error| error.to_wire())?;
        }

        // Assembled before the install rather than after it, because the marker the install leaves
        // in the directory is this row (T182f, D1).
        let installation = runtimes::Installation {
            kind,
            version: version.clone(),
            channel: package.channel.into(),
            path: into.clone(),
            bytes: selection.artifact.size,
            url: selection.artifact.url.clone(),
            sha256: selection.artifact.sha256.clone(),
            // Recorded because the shim reads it, months later and with nothing to ask: which file
            // inside the directory is `php` is the publisher's layout, not ours.
            provides: selection.artifact.provides.clone(),
            // The other half of what the index knows and the daemon would otherwise consult once and
            // forget. See `migrations-archive/0005_runtime_extensions.sql`.
            extension_dir: selection.artifact.extension_dir.clone(),
            extensions: selection.artifact.extensions.clone(),
        };
        let marker = mixengine_core::adopt::marker::Marker::runtime(&installation).encode();

        // **Restore or record** — roadmap task **T206d**, D3: decided before the download, because an
        // install whose row already exists writes over it rather than beside it.
        let restoring = matches!(
            runtimes::record(&self.store, kind, version).await,
            Ok(found) if found.is_missing()
        );

        let smoke = runtimes::smoke_test(kind);
        let installed = match self
            .fetcher
            .installer
            .install_marked(
                selection.artifact,
                &into,
                smoke.as_ref(),
                mixengine_core::install::NotAnArchive::Refuse,
                Some(&marker),
                handle,
            )
            .await
        {
            Ok(installed) => installed,

            // The folder came back between the check and the rename. There is nothing to adopt:
            // the row is already there, and adopting would try to write a second one.
            Err(mixengine_core::Error::Runtime(RuntimeError::AlreadyInstalled { .. }))
                if restoring =>
            {
                return Err(
                    mixengine_core::Error::Runtime(RuntimeError::AlreadyRecorded {
                        kind,
                        version: version.clone(),
                    })
                    .to_wire(),
                );
            }

            // **T182f, D2.** The directory is there and has no row: an earlier home installed it.
            Err(mixengine_core::Error::Runtime(RuntimeError::AlreadyInstalled { path })) => {
                let found = mixengine_core::adopt::walk::Found {
                    subject: mixengine_core::adopt::Subject::Runtime {
                        kind,
                        version: version.clone(),
                    },
                    path,
                };

                return match crate::adopt::claim_offered(
                    &self.store,
                    &found,
                    &selection.artifact.sha256,
                    handle,
                )
                .await?
                {
                    mixengine_core::adopt::walk::Claimed::Runtime(summary) => {
                        self.after_recorded(kind, version, summary).await
                    }
                    mixengine_core::adopt::walk::Claimed::Package(_) => Err(Error::new(
                        ErrorCode::Internal,
                        "a runtime directory was recorded as a package",
                    )),
                };
            }

            Err(error) => return Err(error.to_wire()),
        };

        let installation = runtimes::Installation {
            path: installed.path.clone(),
            bytes: installed.bytes,
            ..installation
        };
        let at = Timestamp::from_system_time(SystemTime::now());
        let record = match restoring {
            true => runtimes::restore(&self.store, &installation, at).await,
            false => runtimes::remember(&self.store, &installation, at).await,
        };

        let summary = match record {
            Ok(summary) => summary,

            // **The one place the ordering is undone rather than kept.** A directory with no row is
            // survivable in general — it is invisible and costs disk — but this is the moment we
            // know one exists, and leaving it would make the retry that fixes everything else fail
            // with `already installed` instead. Best-effort: if it cannot be removed either, the
            // error the caller gets is still the one that explains why nothing was installed.
            Err(error) => {
                if let Err(cleanup) = runtimes::discard(&installed.path).await {
                    tracing::warn!(
                        path = %installed.path.display(),
                        %cleanup,
                        "a runtime whose row could not be written could not be removed either"
                    );
                }
                return Err(error.to_wire());
            }
        };

        self.after_recorded(kind, version, summary).await
    }

    /// What every newly recorded runtime needs: for a JDK this home's authority, a pool and its
    /// activation port, and its `conf.d`. Shared by an install, an adopt and the start walk — roadmap
    /// task **T182f**.
    ///
    /// # Errors
    ///
    /// Only when the summary cannot be encoded: everything else here is reported and left for the
    /// next daemon start, on the reasoning each step gives.
    pub(crate) async fn after_recorded(
        &self,
        kind: RuntimeKind,
        version: &PackageVersion,
        summary: RuntimeSummary,
    ) -> Result<serde_json::Value, Error> {
        // **After the row and never before it**, because the pool points at that row: this is the
        // post-install hook `docs/features/runtime-versions.md` describes, and it is the same
        // idempotent call the daemon makes at boot. A failure here is reported and does not undo the
        // install — a PHP with no pool is a PHP the next boot gives one to, where an install rolled
        // back for it would be eighty megabytes thrown away over a row.
        // **A JDK verifies this home's own sites from its first run** — roadmap task T27e, D9.
        // After the row for the pool hook's reason below, and because what this walks is rows; it
        // never undoes the install, on that hook's rule — a JDK that cannot verify a local site is
        // more use than a hundred and forty megabytes thrown away over a certificate store.
        if kind == RuntimeKind::Java {
            crate::certs::jdks::hold(&self.store, self.paths.certs())
                .await
                .log("after a JDK was installed");
        }

        match mixengine_core::services::pools::ensure(
            &self.store,
            mixengine_platform::host().as_ref(),
            &crate::services::catalogue(),
        )
        .await
        {
            Ok(created) if created.is_empty() => {}
            Ok(created) => {
                tracing::info!(pools = ?created, "the new runtime was given its service")
            }
            Err(error) => tracing::warn!(
                kind = kind.as_str(),
                version = version.as_str(),
                %error,
                "this runtime was installed but could not be given its service; the next daemon \
                 start will try again"
            ),
        }

        // **And the address that lets a request start that pool again** — roadmap task **T72a**.
        //
        // The same two idempotent repairs the daemon makes at boot, in the same order, because a
        // pool that has just been created has neither: `activation::ensure` gives it the port a
        // Windows activator listens on, and `hold_all` binds the address a site file is about to
        // name. Without them the activator arrives only at the next daemon start — and until then a
        // pool that idle-stops leaves its site answering 502, which is a failure a user would have
        // to guess a restart out of.
        //
        // **Found by measurement rather than by reading**: this hole was T70's, and it stayed
        // invisible until T72a's cold path put a real request through a pool the sweeper had
        // stopped.
        //
        // Reported and not fatal, on the pool hook's reasoning above.
        let host = mixengine_platform::host();

        if let Err(error) = mixengine_core::services::activation::ensure(
            &self.store,
            host.as_ref(),
            &crate::services::catalogue(),
        )
        .await
        {
            tracing::warn!(
                %error,
                "a new pool could not be given an activation port; the next daemon start will try \
                 again"
            );
        }

        match crate::services::activate::hold_all(
            Arc::clone(&self.services),
            &self.paths,
            &self.store,
            host.as_ref(),
        )
        .await
        {
            Ok(held) if held.is_empty() => {}
            Ok(held) => tracing::info!(services = ?held, "a request can now start these services"),
            Err(error) => tracing::warn!(
                %error,
                "a new pool cannot be started by a request until this daemon is restarted"
            ),
        }

        // The ini set this build can load, rendered before anything runs out of it — reported rather
        // than fatal, on the pool hook's reasoning above: a PHP with no `conf.d` is one the next
        // daemon start gives one to, where an install rolled back for it would be eighty megabytes
        // thrown away over a file.
        match mixengine_core::runtimes::extensions::state(&self.store, kind, version).await {
            Ok(state) => {
                if let Err(error) =
                    mixengine_core::runtimes::extensions::render(&self.paths, &state).await
                {
                    tracing::warn!(
                        %error,
                        "this runtime was installed but its conf.d could not be written"
                    );
                }
            }
            Err(error) => tracing::warn!(
                %error,
                "this runtime was installed but its extensions could not be read back"
            ),
        }

        self.refresh_bin().await;

        serde_json::to_value(&summary).map_err(|error| {
            Error::new(
                ErrorCode::Internal,
                format!("what the install produced could not be encoded: {error}"),
            )
        })
    }
}
