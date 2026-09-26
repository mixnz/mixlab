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
                format!("{}: {by} — nothing was changed", row.what),
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
            let autostart = Arc::clone(&self.autostart);
            done.insert(
                ResidueId::AutostartEntry,
                match crate::api::on_a_blocking_thread(move || autostart.disable()).await {
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
                              repair — run the uninstall again once the rows above are clear"
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

/// Forget any waiting helper installation before an uninstall asks for the removal — roadmap task
/// **T88d**.
///
/// `Elevation::require_helper` puts `HelperInstall {}` in the queue at every daemon start, and
/// T88d's bootstrap adds one whenever a producer on a machine with no installed helper asks for
/// anything. Either can still be waiting when somebody presses Uninstall, and the batch would then
/// install the helper and remove it — the right end state, reached by doing and undoing work in
/// front of a person reading the list.
///
/// `HelperReplace {}` for the same reason: it exists to put a newer helper on a machine that is
/// about to have none.
///
/// **`pub(crate)` for its test**, which lives in [`crate::elevation`]'s test module because that is
/// where the fixture holding a queue and a machine is.
///
/// # Errors
///
/// The wire error of a queue that could not be read or written.
pub(crate) async fn drop_helper_installations(
    elevation: &crate::elevation::Elevation,
) -> Result<(), Error> {
    for waiting in elevation.status().await?.pending {
        if matches!(
            waiting.op,
            PrivilegedOp::HelperInstall {} | PrivilegedOp::HelperReplace {}
        ) {
            elevation
                .drop_pending(&mixengine_proto::ElevationDrop {
                    op: Some(waiting.id),
                })
                .await?;
        }
    }

    Ok(())
}

/// The second reading, paired with the first row by row — or a refusal when the two cannot be.
///
/// **Rows are paired by position**, as T87 always has. A process that started between the two
/// readings adds an in-use row the first did not have, and those rows are dropped here: the run
/// refused on any in-use row before it began, so a new one is somebody who arrived after the
/// check, and the rename on the way out refuses rather than half-deletes if they are still there.
/// Anything else that makes the readings disagree is a machine that changed underneath the
/// uninstall, and mis-pairing its rows would report one thing's outcome against another's name.
fn aligned(planned: &[Residue], mut measured: Vec<Residue>) -> Result<Vec<Residue>, Error> {
    measured.retain(|row| row.id != ResidueId::InUse);

    let agrees = planned.len() == measured.len()
        && planned
            .iter()
            .zip(&measured)
            .all(|(before, after)| before.id == after.id);

    match agrees {
        true => Ok(measured),
        false => Err(Error::new(
            mixengine_proto::ErrorCode::Internal,
            "this machine changed while it was being uninstalled; run the uninstall again",
        )),
    }
}

/// The second reading, with each privileged row replaced by its settled outcome.
fn merge(measured: Vec<Residue>, privileged: Vec<Residue>) -> Vec<Residue> {
    let mut settled = privileged.into_iter();

    measured
        .into_iter()
        .map(|row| match needs_the_helper(row.id) {
            true => settled.next().unwrap_or(row),
            false => row,
        })
        .collect()
}

/// What became of a credential row, from how many keys there were and how many the store still
/// lists — T182d. Measured, like every other row.
fn forgotten(before: usize, left: mixengine_platform::Result<usize>) -> Removal {
    match left {
        Ok(0) => Removal::Removed {
            what: format!("forgot {before} from this user's credential store"),
        },
        Ok(left) => Removal::Failed {
            because: format!("{left} of {before} are still in this user's credential store"),
        },
        Err(error) => Removal::Failed {
            because: format!("the credential store could not be read back: {error}"),
        },
    }
}

/// The home stays, so the passwords its data needs stay with it — T182d, D1.
fn keep_credentials(items: &mut [Residue]) {
    for item in items.iter_mut() {
        if matches!(
            item.id,
            ResidueId::Credentials | ResidueId::WindowCredentials
        ) && matches!(item.outcome, Removal::Planned { .. })
        {
            item.outcome = Removal::Kept {
                because: "this home is kept, and the passwords its data needs are kept with it"
                    .to_owned(),
            };
        }
    }
}

/// Is this one of the rows the elevated helper answers for?
///
/// **The whole of what [`settle`] may be applied to.** The three that need no token are answered
/// where they are acted on, and the home is answered by `arm_the_home` — which runs *after* the
/// fold, so the home is still `Planned` when the fold sees it.
fn needs_the_helper(id: ResidueId) -> bool {
    matches!(
        id,
        ResidueId::HostsBlock
            | ResidueId::ResolverWiring
            | ResidueId::PortAccess
            | ResidueId::FirewallRules
            | ResidueId::TrustStore
            | ResidueId::PrivilegedHelper
            | ResidueId::AuditLog
    )
}

/// Did the plan say there was something of ours here?
/// Remove the recorded files from the program's directory, and say what is left — read back off
/// the disk rather than taken from `remove_file`'s answer, on this module's rule.
pub(crate) fn remove_completed(directory: &std::path::Path, recorded: &[String]) -> Removal {
    let stuck: Vec<String> = recorded
        .iter()
        .map(|name| directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
        .filter(|file| {
            let _ = std::fs::remove_file(file);
            file.exists()
        })
        .map(|file| file.display().to_string())
        .collect();

    match stuck.is_empty() {
        true => Removal::Removed {
            what: "the files this install added to itself are gone".to_owned(),
        },
        false => Removal::Failed {
            because: format!("still there: {}", stuck.join(", ")),
        },
    }
}

fn planned_row(planned: &[Residue], id: ResidueId) -> bool {
    planned
        .iter()
        .any(|row| row.id == id && matches!(row.outcome, Removal::Planned { .. }))
}

/// What became of one privileged row, from the second reading rather than from what was attempted.
///
/// **`Removed` is a measurement.** The helper is honest about what it did, but it is a separate
/// process describing finished work; this is a fresh reading of the thing itself, and it costs no
/// privilege for any row on the list (the T87 design, D3).
///
/// **And `OnRestart` is settled by the queue and the disk**, not by reading the helper's sentence: an
/// operation that is no longer waiting has been applied, so a file that is nonetheless still there is
/// one the operating system accepted and has not got to yet. That is Windows and the privileged
/// helper — a running image cannot be unlinked there — and it is the only row that can answer it.
fn settle(
    before: Residue,
    after: Residue,
    granted: bool,
    waiting: &[mixengine_proto::PendingOp],
) -> Residue {
    let Removal::Planned { how } = &before.outcome else {
        // **Absent before and present after is a failure** (the T182b design, D7): the act left
        // this on the machine, and the second reading's `Planned` would have read as nothing
        // being wrong — which is how an uninstall that put the authority back reported success.
        if matches!(before.outcome, Removal::Absent {})
            && matches!(after.outcome, Removal::Planned { .. })
        {
            return Residue {
                outcome: Removal::Failed {
                    because: "this was not here before the uninstall and it is here now".to_owned(),
                },
                ..after
            };
        }

        // Kept or Failed before anything was attempted: the second reading is the answer.
        return after;
    };

    if matches!(after.outcome, Removal::Absent {}) {
        return Residue {
            outcome: Removal::Removed { what: how.clone() },
            ..after
        };
    }

    if !granted {
        return Residue {
            outcome: Removal::Enqueued { what: how.clone() },
            ..after
        };
    }

    let still_queued = waiting.iter().any(|pending| {
        matches!(
            (&pending.op, before.id),
            (PrivilegedOp::HostsApply { .. }, ResidueId::HostsBlock)
                | (
                    PrivilegedOp::ResolverRevoke { .. },
                    ResidueId::ResolverWiring
                )
                | (PrivilegedOp::PortAccessRevoke { .. }, ResidueId::PortAccess)
                | (PrivilegedOp::FirewallApply { .. }, ResidueId::FirewallRules)
                | (PrivilegedOp::TrustCaRemove { .. }, ResidueId::TrustStore)
                | (PrivilegedOp::HelperRemove {}, ResidueId::PrivilegedHelper)
                | (PrivilegedOp::AuditLogRemove {}, ResidueId::AuditLog)
        )
    });

    let outcome = match (still_queued, before.id) {
        (false, ResidueId::PrivilegedHelper) => Removal::OnRestart {
            what: format!(
                "this system accepted the removal and will carry it out {}, because a program \
                 cannot be deleted while it is running",
                mixengine_proto::privileged::AT_NEXT_RESTART
            ),
        },
        (false, _) => Removal::Failed {
            because: "the operation was applied and this is still here".to_owned(),
        },
        (true, _) => Removal::Failed {
            because: "this is still here, and the operation that removes it is still waiting for \
                      permission"
                .to_owned(),
        },
    };

    Residue { outcome, ..after }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_left_is_a_removal() {
        assert!(matches!(forgotten(3, Ok(0)), Removal::Removed { .. }));
    }

    #[test]
    fn something_left_is_a_failure_naming_how_many() {
        let Removal::Failed { because } = forgotten(3, Ok(1)) else {
            panic!("not a failure")
        };
        assert!(because.contains('1'), "{because}");
    }

    #[test]
    fn a_store_that_cannot_be_read_back_is_a_failure() {
        let refused = Err(mixengine_platform::Error::UnsupportedPlatform {
            capability: "Keyring",
            reason: "gone".to_owned(),
        });
        assert!(matches!(forgotten(3, refused), Removal::Failed { .. }));
    }

    /// Review focus 5: something outside the home failed, so the home stays and so do its
    /// passwords.
    #[test]
    fn a_home_that_stays_keeps_its_passwords() {
        let mut items = vec![Residue {
            id: ResidueId::Credentials,
            what: "1 password".to_owned(),
            location: "mixengine".to_owned(),
            outcome: Removal::Planned { how: String::new() },
        }];

        keep_credentials(&mut items);

        assert!(
            matches!(items[0].outcome, Removal::Kept { .. }),
            "{items:?}"
        );
    }

    /// T185a: the files are removed, and what is reported is read back off the disk.
    #[test]
    fn removing_what_the_install_added_reads_the_disk_back() {
        let directory = tempfile::tempdir().expect("a directory");
        let exe = std::env::consts::EXE_SUFFIX;
        let file = directory.path().join(format!("mixengine-trampoline{exe}"));
        std::fs::write(&file, b"x").expect("a file");

        let outcome = remove_completed(directory.path(), &["mixengine-trampoline".to_owned()]);

        assert!(matches!(outcome, Removal::Removed { .. }), "{outcome:?}");
        assert!(!file.exists());
    }

    /// **The home is not the helper's business, and this is the assertion that says so.**
    ///
    /// `settle` reads a row that is still `Planned` as one the helper was asked about and did not
    /// manage — which is right for the seven it answers for and wrong for everything else. The home
    /// is `Planned` for two statements longer than the fold, because `arm_the_home` rewrites it
    /// afterwards; settled in between, every complete uninstall reported the home as waiting for a
    /// prompt and then kept it for that reason. Found by CI on 2026-09-04.
    #[test]
    fn the_helper_answers_for_seven_rows_and_the_home_is_not_one_of_them() {
        for id in [
            ResidueId::HostsBlock,
            ResidueId::ResolverWiring,
            ResidueId::PortAccess,
            ResidueId::FirewallRules,
            ResidueId::TrustStore,
            ResidueId::PrivilegedHelper,
            ResidueId::AuditLog,
        ] {
            assert!(needs_the_helper(id), "{id:?}");
        }

        for id in [
            // The four that need no token, answered where they are acted on.
            ResidueId::BrowserTrust,
            ResidueId::AutostartEntry,
            ResidueId::PathEntry,
            ResidueId::CompletedBinary,
            // And the four the daemon removes itself, answered by `arm_the_home`.
            ResidueId::Home,
            ResidueId::RelocatedDirectory,
            ResidueId::WindowData,
            ResidueId::WindowCache,
            // And the two credential rows, answered by `forget_credentials`.
            ResidueId::Credentials,
            ResidueId::WindowCredentials,
        ] {
            assert!(!needs_the_helper(id), "{id:?}");
        }

        assert_eq!(
            ResidueId::ALL
                .iter()
                .filter(|id| needs_the_helper(**id))
                .count(),
            7,
            "every id is on exactly one side of this, and a new one has to choose"
        );
    }

    /// What `settle` does to a row that is still there and was never granted — the behaviour that is
    /// correct for a privileged row and catastrophic for the home.
    #[test]
    fn a_row_that_was_never_granted_is_still_waiting() {
        let settled = settle(planned(), planned(), false, &[]);

        assert!(
            matches!(settled.outcome, Removal::Enqueued { .. }),
            "{settled:?}"
        );
    }

    /// And one the machine no longer holds is a removal, whatever was attempted: the second reading
    /// decides, not the helper's own account of itself (the T87 design, D3).
    #[test]
    fn a_row_the_machine_no_longer_holds_is_a_removal() {
        let mut gone = planned();
        gone.outcome = Removal::Absent {};

        let settled = settle(planned(), gone, true, &[]);

        assert!(
            matches!(settled.outcome, Removal::Removed { .. }),
            "{settled:?}"
        );
    }

    /// T182b, D7. A row that was absent before the act and is present after it is something the
    /// act left on the machine — put back by a grant that carried more than it should, or by
    /// anything else — and it is a failure, never the `Planned` the second reading spells it as.
    #[test]
    fn a_row_absent_before_and_present_after_is_a_failure() {
        let mut absent = planned();
        absent.outcome = Removal::Absent {};

        for granted in [true, false] {
            let settled = settle(absent.clone(), planned(), granted, &[]);

            assert!(
                matches!(settled.outcome, Removal::Failed { .. }),
                "{settled:?}"
            );
        }
    }

    /// One row, planned, for the two above to work on.
    fn planned() -> Residue {
        Residue {
            id: ResidueId::PrivilegedHelper,
            what: "MixEngine's privileged helper".to_owned(),
            location: "somewhere only an administrator can write".to_owned(),
            outcome: Removal::Planned {
                how: "remove it".to_owned(),
            },
        }
    }

    /// **A kept helper asks the helper for nothing** — the T88e design, D3. Only a `Planned` row
    /// becomes an operation, so the batch carries no `helper-remove`.
    #[test]
    fn a_kept_helper_is_not_a_row_to_act_on() {
        let kept = Residue {
            id: ResidueId::PrivilegedHelper,
            what: "MixEngine's privileged helper".to_owned(),
            location: "/usr/local/libexec/mixengine/mixengine-elevate".to_owned(),
            outcome: Removal::Kept {
                because: "it came with the mixengine package, and removing that package removes it"
                    .to_owned(),
            },
        };

        assert!(!planned_row(&[kept], ResidueId::PrivilegedHelper));
    }
}
