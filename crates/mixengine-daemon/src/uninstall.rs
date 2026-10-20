//! `daemon.uninstall_plan` and `daemon.uninstall` — roadmap task **T87**.
//!
//! **Two methods on `daemon.doctor`/`daemon.doctor_repair`'s split, and not one flag.** The plan is
//! a read in the strict sense — no row written, nothing enqueued, no prompt possible — which is what
//! makes it safe to put in front of the one command that cannot be undone. The act is a job, because
//! it can raise the elevation prompt and what that waits on is a person reading a dialog.
//!
//! Both build their list from [`inventory::take`], which is the one enumeration: two of them, one
//! for the dry run and one for the real run, is the second inventory the roadmap sentence refuses.

mod inventory;
mod outcome;

use outcome::{aligned, forgotten, keep_credentials, merge, needs_the_helper, planned_row, settle};
pub(crate) use outcome::{drop_helper_installations, remove_completed};

use std::sync::Arc;

use mixengine_proto::privileged::PrivilegedOp;
use mixengine_proto::{Error, Removal, Residue, ResidueId, UninstallQuery, UninstallReport};

use crate::api::Armed;

/// The doors an uninstall reaches this daemon's own subsystems through.
///
/// One argument rather than seven, on [`Supervision`](crate::api::Supervision)'s rule: a constructor
/// whose arguments have to be *counted* is one a caller gets wrong silently, and every one of these
/// is the only door into the thing it holds.
pub(crate) struct Doors {
    /// Where the DNS server is listening, which is what a resolver would have been pointed at.
    pub(crate) dns: Arc<crate::dns::Dns>,

    /// The front end's program path, which is what the port-access reading is about.
    pub(crate) services: Arc<crate::services::Registry>,

    /// `<root>/bin` and this user's PATH.
    pub(crate) shims: Arc<crate::shims::Shims>,

    /// The login entry.
    pub(crate) autostart: Arc<crate::autostart::Autostart>,

    /// The queue, and the one prompt an uninstall raises.
    pub(crate) elevation: Arc<crate::elevation::Elevation>,

    /// This home's authority, for the browsers it was put into.
    pub(crate) certificates: crate::certs::Certificates,

    /// Where the home's own directories are left for `main` to remove on the way out.
    pub(crate) armed: Arc<Armed>,

    /// Where this copy's binaries are, and whether MixEngine updates them itself — T185a.
    pub(crate) updates: Arc<crate::updates::Updates>,
}

/// What an uninstall put in the queue — the T182b design, D6.
#[derive(Debug, Default)]
struct Asked {
    /// The operations it enqueued, in the order it enqueued them.
    ops: Vec<PrivilegedOp>,

    /// The queue rows that hold them, which are what its prompt is raised over.
    ids: Vec<mixengine_proto::PendingOpId>,

    /// What they replaced: a whole-state operation somebody else had waiting under the same key,
    /// put back if the uninstall does not finish.
    displaced: Vec<PrivilegedOp>,
}

/// Both halves of the uninstall.
///
/// **Holding one set of readers**, so the plan and the act cannot disagree about what is on this
/// machine — and the same readers `Doctor` is given, reached through the door that already owns
/// each. A second `Host` here would be a second answer to *what does this machine's hosts file
/// hold*.
#[derive(Debug)]
pub(crate) struct Uninstall {
    /// The rows, for the one question this asks of them: is anything shared?
    store: mixengine_core::Store,

    /// This machine: its hosts file, its resolver, its trust store, its port access, its browsers.
    host: Arc<dyn mixengine_platform::Host>,

    /// Where the DNS server is listening, which is what a resolver would have been pointed at.
    dns: Arc<crate::dns::Dns>,

    /// The front end's program path, which is what the port-access reading is about.
    services: Arc<crate::services::Registry>,

    /// `<root>/bin` and this user's PATH — the first of the two things outside the home that need no
    /// token.
    shims: Arc<crate::shims::Shims>,

    /// The login entry — the second of them.
    autostart: Arc<crate::autostart::Autostart>,

    /// The queue, and the one prompt this raises.
    elevation: Arc<crate::elevation::Elevation>,

    /// This home's authority, for the browsers it was put into — the third unprivileged thing.
    certificates: crate::certs::Certificates,

    /// Where the home's own directories are left for `main` to remove on the way out.
    armed: Arc<Armed>,

    /// Where this copy's binaries are, and whether MixEngine updates them itself — T185a.
    updates: Arc<crate::updates::Updates>,

    /// The home's own layout: where its authority is, and every directory it owns.
    paths: mixengine_core::Paths,
}

impl Uninstall {
    /// The one of these the API holds.
    ///
    /// **The home's layout rather than its root and its certificate directory separately**, on
    /// `Doctor::new`'s rule: both are derived from it, and passing them apart lets a caller hand
    /// this a certificate directory from one home and a root from another.
    pub(crate) fn new(
        store: &mixengine_core::Store,
        host: Arc<dyn mixengine_platform::Host>,
        doors: Doors,
        paths: &mixengine_core::Paths,
    ) -> Arc<Self> {
        Arc::new(Self {
            store: store.clone(),
            host,
            dns: doors.dns,
            services: doors.services,
            shims: doors.shims,
            autostart: doors.autostart,
            elevation: doors.elevation,
            certificates: doors.certificates,
            armed: doors.armed,
            updates: doors.updates,
            paths: paths.clone(),
        })
    }

    /// `daemon.uninstall_plan` — everything an uninstall would take off this machine.
    ///
    /// **A read, and every branch of it.** Nothing here writes a row, enqueues an operation or can
    /// raise a prompt, which is what `daemon.doctor` established the shape for and what makes this
    /// safe to call from a client that is only showing somebody what would happen.
    ///
    /// # Errors
    ///
    /// The wire error of a home whose layout could not be read. A *machine* that could not be read
    /// is never an error: it is [`Removal::Failed`] on the row it
    /// is about, because an uninstall that reported "nothing there" for a question nobody could
    /// answer is the one failure this feature exists to prevent.
    pub(crate) async fn plan(&self, query: &UninstallQuery) -> Result<UninstallReport, Error> {
        Ok(UninstallReport {
            items: self.rows(query).await?,
        })
    }

    /// `daemon.uninstall` — the job that takes MixEngine off this machine.
    ///
    /// **Refuse when blocked, privileged first, then this user's own things, home last** (the T182
    /// design, D4 and D5). A process running from a directory that is going refuses the whole run
    /// before anything is enqueued. Everything needing the helper goes into one batch, so there is
    /// one dialog — and it goes *first*, so a declined prompt has changed nothing at all: `PATH`, the
    /// login entry and the browsers are touched only once the machine is clear. That reverses T87's
    /// D7, which cleaned them before the prompt so a declined grant still left them clean; an
    /// uninstall that must either finish or change nothing cannot leave a half-clean machine behind.
    /// The home goes last because the daemon is running out of it.
    ///
    /// **Nothing stops a service here, and nothing in the home is removed here.** Both happen on the
    /// way out: the home's directories are *armed*, and `mixengined` removes them after its own
    /// shutdown has stopped every service in dependency order within the configured grace and
    /// `Store::close` has checkpointed the write-ahead log. A stop walk of this method's own would be
    /// a second answer to a question `daemon.shutdown` already answers better.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::PreconditionFailed`](mixengine_proto::ErrorCode::PreconditionFailed) when a
    /// process is in the way, with nothing changed; the wire error of a home whose layout could not
    /// be read, or of a queue that could not be written. **A declined prompt is not an error** — ADR
    /// 0005 — and neither is a machine that could not be read: both are rows.
    pub(crate) async fn run(
        &self,
        query: &UninstallQuery,
        handle: &crate::jobs::JobHandle,
    ) -> Result<UninstallReport, Error> {
        handle.progress(5, "reading what this machine holds").await;
        let planned = self.rows(query).await?;

        // T182, D4: nothing is touched while a process runs from a directory that is going — or,
        // since T182e, holds something in one that cannot be moved. The row's own sentence says
        // which, and what to close.
        if let Some(row) = planned
            .iter()
            .find(|row| matches!(row.outcome, Removal::Blocked { .. }))
        {
            let by = match &row.outcome {
                Removal::Blocked { by } => by.as_str(),
                _ => "it is in the way",
            };
            return Err(Error::new(
                mixengine_proto::ErrorCode::PreconditionFailed,
                format!("{}: {by}; nothing was changed", row.what),
            ));
        }

        handle.progress(20, "asking for permission").await;
        let asked = self.ask_for_the_rest(&planned).await?;

        // **Only when something is waiting.** `elevation.grant` refuses an empty queue outright, and
        // raising a dialog to discover that would be a prompt for nothing.
        let granted = match query.grant && !asked.ids.is_empty() {
            true => Some(self.elevation.grant_only(handle, &asked.ids).await),
            false => None,
        };

        if let Some(Err(error)) = &granted {
            // Logged and carried, never returned: what the machine holds now is read below, and a
            // grant that failed against a store that turns out not to hold the authority anyway is
            // still a removal — `cert.ca_uninstall`'s rule, and the reason this method measures.
            tracing::warn!(%error, "the grant an uninstall raised did not finish");
        }

        handle.progress(50, "reading this machine back").await;
        let measured = aligned(&planned, self.rows(query).await?)?;
        let waiting = self.elevation.status().await?.pending;

        let privileged: Vec<Residue> = planned
            .iter()
            .zip(&measured)
            .filter(|(before, _)| needs_the_helper(before.id))
            .map(|(before, after)| {
                settle(before.clone(), after.clone(), granted.is_some(), &waiting)
            })
            .collect();

        // `Kept` is a helper a package placed (T88e), which is the package's to remove.
        let machine_clear = privileged.iter().all(|row| {
            matches!(
                row.outcome,
                Removal::Removed { .. }
                    | Removal::Absent {}
                    | Removal::OnRestart { .. }
                    | Removal::Kept { .. }
            )
        });

        if !machine_clear {
            // A prompt that was raised and did not clear the machine leaves nothing of this run
            // waiting in the queue: the next run enqueues again. `grant: false` keeps them — that
            // caller asked for exactly a queue.
            if granted.is_some() {
                self.restore_queue(&asked).await?;
            }

            let mut items = merge(measured, privileged);
            self.forget_credentials(&mut items, false).await;
            self.arm_the_home(&mut items, true);

            return Ok(UninstallReport { items });
        }

        handle
            .progress(
                70,
                "taking this home out of your PATH, your login and your browsers",
            )
            .await;
        let mut done = self.undo_what_needs_no_token(&planned).await;

        // **And every installed JDK lets this home's authority go** — roadmap task T27e, D9.
        // Logged rather than a residue row of its own: `cacerts` is inside the home, not a store on
        // this machine a person could be shown. It matters with `keep_home`, where a kept JDK would
        // otherwise go on trusting an authority the machine has just been told to forget.
        if let Ok(mixengine_proto::CaState::Present { ca }) = self.certificates.authority().await {
            self.certificates
                .release_from_jdks(&ca.key_id)
                .await
                .log("during an uninstall");
        }

        handle.progress(85, "reading this machine back").await;
        let measured = aligned(&planned, self.rows(query).await?)?;

        let mut items = Vec::with_capacity(planned.len());
        for (before, after) in planned.into_iter().zip(measured) {
            items.push(match done.remove(&before.id) {
                // One of the three that needed no token, acted on above.
                Some(outcome) => Residue { outcome, ..after },

                // One of the seven the helper answers for, settled against the second reading.
                None if needs_the_helper(before.id) => {
                    settle(before, after, granted.is_some(), &waiting)
                }

                // **The home, a relocated directory, or a row that had nothing to do.** `settle` may
                // not be let near these: it reads a row that is still `Planned` as one the helper was
                // asked about and did not manage, and the home is `Planned` right up until
                // `arm_the_home` rewrites it two statements below — so settling it turned every
                // complete uninstall into one that reported the home as waiting for a prompt, and
                // then kept the home because of it. Found by CI on 2026-09-04.
                //
                // Except for one of the three that needed no token which was absent and is now
                // back: that is `settle`'s T182b rule, and it holds for them too.
                None if !matches!(
                    before.id,
                    ResidueId::Home
                        | ResidueId::RelocatedDirectory
                        | ResidueId::WindowData
                        | ResidueId::WindowCache
                        | ResidueId::Credentials
                        | ResidueId::WindowCredentials
                        | ResidueId::InUse
                ) =>
                {
                    settle(before, after, granted.is_some(), &waiting)
                }

                None => after,
            });
        }

        // T182d: the credential store goes only with a home that is going, so ask the same
        // question `unfinished` asks, and let a failure here keep the home.
        let clear = !items.iter().any(|item| {
            matches!(
                item.outcome,
                Removal::Failed { .. } | Removal::Enqueued { .. }
            )
        });
        self.forget_credentials(&mut items, clear).await;

        // **The home is kept when anything outside it is still there, whatever was asked for.** A
        // home removed while this machine still routes `.test` to a daemon that no longer exists is
        // the worst of the states available, and one nobody could repair without the home that
        // knew how.
        let unfinished = items.iter().any(|item| {
            matches!(
                item.outcome,
                Removal::Failed { .. } | Removal::Enqueued { .. }
            )
        });

        // T182h: the home's state goes with the home, so a folder that stays gets a copy of it —
        // the next home pointed at it can bring the projects and sites back.
        if !unfinished && !query.keep_home {
            self.leave_a_copy(&mut items).await;
        }

        handle
            .progress(95, "arming this home's own directories")
            .await;
        self.arm_the_home(&mut items, unfinished);

        // T182b, D6: a finished uninstall leaves nothing waiting. The daemon is about to exit, and
        // nothing in the queue belongs to a machine this home has just been taken off.
        if !unfinished {
            self.elevation
                .drop_pending(&mixengine_proto::ElevationDrop { op: None })
                .await?;
        }

        Ok(UninstallReport { items })
    }

    /// Put the queue back as this uninstall found it — the T182b design, D6.
    ///
    /// Drops the rows this run enqueued, then enqueues again what they displaced: a whole-state
    /// operation (`hosts-apply`, `firewall-apply`) dedupes on its name, so the uninstall's empty
    /// block *replaced* a site's pending one rather than waiting beside it. A declined uninstall has
    /// then changed nothing at all, the queue included.
    async fn restore_queue(&self, asked: &Asked) -> Result<(), Error> {
        for id in &asked.ids {
            self.elevation
                .drop_pending(&mixengine_proto::ElevationDrop { op: Some(*id) })
                .await?;
        }

        for op in &asked.displaced {
            self.elevation.enqueue(op).await?;
        }

        Ok(())
    }

    /// The three things outside the home that belong to this account rather than to the machine.
    ///
    /// **After the prompt, and only once the machine is clear** (the T182 design, D5). None needs a
    /// token, which is exactly why they wait: a person who declines the dialog has then changed
    /// nothing at all, and the next run finds all three where they were.
    ///
    /// Answers only the rows it acted on; everything else is settled from the second reading.
    async fn undo_what_needs_no_token(
        &self,
        planned: &[Residue],
    ) -> std::collections::HashMap<ResidueId, Removal> {
        let mut done = std::collections::HashMap::new();

        if planned_row(planned, ResidueId::PathEntry) {
            done.insert(
                ResidueId::PathEntry,
                match self.shims.uninstall().await {
                    Ok(report) => Removal::Removed {
                        what: format!("{} is no longer on your PATH", report.directory),
                    },
                    Err(error) => Removal::Failed {
                        because: format!("your PATH could not be written: {error}"),
                    },
                },
            );
        }

        // T185a: the program files this install added to itself. The record is cleared once they
        // are gone, so a kept home reused by a fresh install does not remove that install's file.
        if planned_row(planned, ResidueId::CompletedBinary)
            && let mixengine_core::updates::Placement::SelfUpdatable { directory } =
                self.updates.placement()
        {
            let recorded: Vec<String> = mixengine_core::updates::records::get(
                &self.store,
                mixengine_core::updates::records::COMPLETED,
            )
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

            let outcome = remove_completed(directory, &recorded);

            if matches!(outcome, Removal::Removed { .. }) {
                let _ = mixengine_core::updates::records::clear(
                    &self.store,
                    mixengine_core::updates::records::COMPLETED,
                )
                .await;
            }

            done.insert(ResidueId::CompletedBinary, outcome);
        }

        if planned_row(planned, ResidueId::AutostartEntry) {
            done.insert(
                ResidueId::AutostartEntry,
                match self.autostart.disable().await {
                    Ok(report) => Removal::Removed {
                        what: format!("{} no longer starts this home at login", report.location),
                    },
                    Err(error) => Removal::Failed {
                        because: format!("the login entry could not be removed: {error}"),
                    },
                },
            );
        }

        if planned_row(planned, ResidueId::BrowserTrust)
            && let Ok(mixengine_proto::CaState::Present { ca }) =
                self.certificates.authority().await
        {
            let change = self.certificates.remove_from_browsers(&ca.key_id).await;

            done.insert(
                ResidueId::BrowserTrust,
                match change.refused.is_empty() {
                    true => Removal::Removed {
                        what: format!(
                            "{} browser database(s) no longer hold it",
                            change.written.len()
                        ),
                    },
                    false => Removal::Failed {
                        because: change.refused.join("; "),
                    },
                },
            );
        }

        done
    }

    /// Put everything that needs the helper in the queue, and say which operations those were.
    ///
    /// **In the order the batch is applied**, which the queue preserves: the log's own removal goes
    /// in last, so the last thing it ever records is the removal of the binary that writes it.
    ///
    /// **Only the rows that said `Planned`.** A machine with no resolver mechanism, or one holding
    /// none of this home's wiring, asks for nothing — a row whose only possible outcome is
    /// `AlreadyDone` is a row that makes a dialog longer for no reason.
    async fn ask_for_the_rest(&self, planned: &[Residue]) -> Result<Asked, Error> {
        // T88d, and before anything else this asks for.
        drop_helper_installations(&self.elevation).await?;

        let mut asked = Asked::default();

        for id in [
            ResidueId::HostsBlock,
            ResidueId::ResolverWiring,
            ResidueId::PortAccess,
            ResidueId::FirewallRules,
            ResidueId::TrustStore,
            // T182a, D2: after every row the program's own files serve, before the helper and its
            // log, so a failure part-way leaves the program in place to finish the job.
            ResidueId::Package,
            ResidueId::PrivilegedHelper,
            ResidueId::AuditLog,
        ] {
            if !planned_row(planned, id) {
                continue;
            }

            let Some(op) = self.operation_for(id).await else {
                continue;
            };

            // T182b, D6: what this row is about to replace in the queue, kept to be put back.
            let key = op.dedupe_key();
            asked.displaced.extend(
                self.elevation
                    .status()
                    .await?
                    .pending
                    .into_iter()
                    .filter(|waiting| waiting.op.dedupe_key() == key && waiting.op != op)
                    .map(|waiting| waiting.op),
            );

            self.elevation.enqueue(&op).await?;
            asked.ops.push(op);
        }

        // **Every batch writes the audit log, so every batch removes it** (T182b). The helper records
        // each operation it applies, so a run whose inventory found no log — an earlier uninstall
        // removed it — would create one with its own first line and then report that as left
        // behind. Measured on the first real uninstall after T182b's D7. The helper applies this
        // last and records nothing for it, whatever position it has in the batch.
        let remove_the_log = PrivilegedOp::AuditLogRemove {};
        if !asked.ops.is_empty() && !asked.ops.contains(&remove_the_log) {
            self.elevation.enqueue(&remove_the_log).await?;
            asked.ops.push(remove_the_log);
        }

        // The rows that now hold what this run asked for, read back rather than assumed: an
        // operation that was already waiting is the same row, and its id is the one to grant.
        asked.ids = self
            .elevation
            .status()
            .await?
            .pending
            .into_iter()
            .filter(|waiting| asked.ops.contains(&waiting.op))
            .map(|waiting| waiting.id)
            .collect();

        Ok(asked)
    }

    /// The operation that undoes one row, rebuilt from the same probe the inventory read.
    ///
    /// **Rebuilt rather than carried on the row.** A `Residue` is a wire type a client renders, and
    /// putting a `PrivilegedOp` on it would put the elevation protocol into the uninstall report —
    /// where a client could read it, and where two clients could disagree about what a row means.
    async fn operation_for(&self, id: ResidueId) -> Option<PrivilegedOp> {
        match id {
            // Whole state: the empty block *is* the removal.
            ResidueId::HostsBlock => Some(PrivilegedOp::hosts_apply(Vec::new())),

            ResidueId::ResolverWiring => {
                let port = self.dns.wirable_port()?;
                let want: Vec<&str> = mixengine_proto::domains::WIRED_TLDS.to_vec();

                self.host
                    .resolver()
                    .probe(&want, port)
                    .ok()?
                    .target()
                    .map(|target| PrivilegedOp::ResolverRevoke { target })
            }

            ResidueId::PortAccess => {
                let binary = self.services.front_end_program().await?;

                self.host
                    .port_access()
                    .probe(&binary, &crate::elevation::Elevation::ANSWERING)
                    .ok()?
                    .target(&binary)
                    .map(|target| PrivilegedOp::PortAccessRevoke { target })
            }

            // Whole state again: the empty plan is the revoke, which is why T74 shipped no
            // `FirewallRevoke` beside it.
            ResidueId::FirewallRules => Some(PrivilegedOp::FirewallApply {
                plan: mixengine_proto::privileged::FirewallPlan {
                    ports: Vec::new(),
                    label: inventory::firewall_label(),
                },
            }),

            ResidueId::TrustStore => {
                let mixengine_proto::CaState::Present { ca } =
                    self.certificates.authority().await.ok()?
                else {
                    return None;
                };

                let der = mixengine_core::certs::ca::der(&ca.certificate_pem)?;

                self.host
                    .trust_store()
                    .probe(&der)
                    .ok()?
                    .target(&ca.key_id)
                    .map(|target| PrivilegedOp::TrustCaRemove { target })
            }

            ResidueId::PrivilegedHelper => Some(PrivilegedOp::HelperRemove {}),

            // Last, always: `mixengine-elevate` applies it after everything else in the batch and
            // records nothing for it, because the line would recreate the file.
            ResidueId::AuditLog => Some(PrivilegedOp::AuditLogRemove {}),

            // T182a: the program the `.pkg` placed, after everything else it serves is undone.
            ResidueId::Package => Some(PrivilegedOp::PackageRemove {}),

            // The four that need no token, and the directories the daemon removes as it exits.
            ResidueId::BrowserTrust
            | ResidueId::AutostartEntry
            | ResidueId::PathEntry
            | ResidueId::CompletedBinary
            | ResidueId::Home
            | ResidueId::RelocatedDirectory
            | ResidueId::WindowData
            | ResidueId::WindowCache
            | ResidueId::Credentials
            | ResidueId::WindowCredentials
            | ResidueId::InUse => None,
        }
    }

    /// Write a copy of this home's state into each relocated folder that stays — roadmap task
    /// **T182h**, spec D5 — and say so on its row.
    ///
    /// Never fatal: a copy that cannot be written costs only the restore it would have offered, and
    /// is logged; the uninstall goes on exactly as it would have.
    async fn leave_a_copy(&self, items: &mut [Residue]) {
        for index in kept_relocated(items) {
            let dir = std::path::PathBuf::from(&items[index].location);
            match mixengine_core::adopt::snapshot::write(&self.store, &dir).await {
                Ok(written) => {
                    tracing::info!(path = %written.display(), "left a copy of this home's state");
                    items[index].outcome = Removal::Kept {
                        because: "you asked for it to stay, and it now holds a copy of your \
                                  projects and sites so the next install can bring them back"
                            .to_owned(),
                    };
                }
                Err(error) => tracing::warn!(
                    path = %dir.display(),
                    %error,
                    "a kept folder could not be given a copy of this home's state"
                ),
            }
        }
    }

    /// Remove what can go now, and leave the rest for `mixengined` to remove as it exits.
    ///
    /// **Nothing is removed here at all, in fact**, and that is deliberate rather than an economy:
    /// this daemon still has services running, a database open and a socket in `run/`. What this does
    /// is record the directories, and the process removes them on the way out — after its own
    /// shutdown has stopped every service and after `Store::close` has checkpointed the log.
    ///
    /// **Called whatever is kept, and it is what marks the run finished** (the T182 design, D1): a
    /// directory the caller asked to keep stays exactly as the inventory said, and a finished run
    /// ends the daemon even when nothing at all was armed.
    fn arm_the_home(&self, items: &mut [Residue], unfinished: bool) {
        let mut paths = Vec::new();

        for item in items.iter_mut() {
            // The window's folders go with the home and by the same all-or-nothing removal (T182b):
            // the webview's cache and a half-removed sync database are the same kind of leftover.
            if !matches!(
                item.id,
                ResidueId::Home
                    | ResidueId::RelocatedDirectory
                    | ResidueId::WindowData
                    | ResidueId::WindowCache
            ) {
                continue;
            }

            if !matches!(item.outcome, Removal::Planned { .. }) {
                continue;
            }

            item.outcome = match unfinished {
                true => Removal::Kept {
                    because: "something outside this home is still there, and a home removed \
                              while this machine is still wired for it is one nothing could \
                              repair; run the uninstall again once the rows above are clear"
                        .to_owned(),
                },
                false => {
                    paths.push(std::path::PathBuf::from(&item.location));

                    Removal::OnExit {
                        what: "this daemon removes it as it exits, which it is about to do"
                            .to_owned(),
                    }
                }
            };
        }

        if !paths.is_empty() {
            self.armed.arm(paths);
        }

        if !unfinished {
            self.armed.finish();
        }
    }

    /// Forget this home's credentials and the window's, when the home is about to go — T182d.
    ///
    /// **Before `arm_the_home`, and only when everything else is clear**, so a failure here keeps
    /// the home: a home removed with its passwords still in the store is one nothing can clean up,
    /// and passwords removed from a home that stays are databases nobody can open.
    async fn forget_credentials(&self, items: &mut [Residue], clear: bool) {
        let planned = |id| {
            items
                .iter()
                .any(|item| item.id == id && matches!(item.outcome, Removal::Planned { .. }))
        };
        let (daemon, window) = (
            planned(ResidueId::Credentials),
            planned(ResidueId::WindowCredentials),
        );
        if !daemon && !window {
            return;
        }
        if !clear {
            keep_credentials(items);
            return;
        }

        let home = mixengine_core::home::id(&self.store)
            .await
            .ok()
            .map(|id| id.as_str().to_owned());
        let host = Arc::clone(&self.host);

        let outcomes = tokio::task::spawn_blocking(move || {
            let keyring = host.keyring();
            let forget = |service: &str, pick: &dyn Fn(&[String]) -> Vec<String>| {
                let keys = keyring.keys(service).map(|keys| pick(&keys));
                let before = keys.as_ref().map_or(0, Vec::len);
                let forgot = keys.and_then(|keys| {
                    keys.iter()
                        .try_for_each(|key| keyring.forget_secret(service, key))
                });
                let left = forgot
                    .and_then(|()| keyring.keys(service))
                    .map(|keys| pick(&keys).len());
                forgotten(before, left)
            };

            let daemon = (daemon && home.is_some()).then(|| {
                let home = home.clone().unwrap_or_default();
                forget(mixengine_platform::KEYRING_SERVICE, &move |keys| {
                    inventory::ours(&home, keys)
                })
            });
            let window = window.then(|| {
                forget(mixengine_core::window::KEYRING_SERVICE, &|keys| {
                    keys.to_vec()
                })
            });
            (daemon, window)
        })
        .await;

        let (daemon, window) = outcomes.unwrap_or_else(|error| {
            let failed = || {
                Some(Removal::Failed {
                    because: format!("the credential store could not be reached: {error}"),
                })
            };
            (failed(), failed())
        });

        for item in items.iter_mut() {
            let outcome = match item.id {
                ResidueId::Credentials => daemon.clone(),
                ResidueId::WindowCredentials => window.clone(),
                _ => None,
            };
            if let (Some(outcome), Removal::Planned { .. }) = (outcome, &item.outcome) {
                item.outcome = outcome;
            }
        }
    }

    /// The inventory, taken once.
    async fn rows(&self, query: &UninstallQuery) -> Result<Vec<Residue>, Error> {
        inventory::take(self, query).await
    }
}

/// The rows of relocated folders this uninstall keeps — where a copy of the state goes (T182h).
fn kept_relocated(items: &[Residue]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.id == ResidueId::RelocatedDirectory && matches!(item.outcome, Removal::Kept { .. })
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests;
