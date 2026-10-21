//! What a producer asks for: the helper, the hosts file, the resolver, the trust store and port
//! access, each queued only where the machine does not already agree.

use std::path::Path;

use mixengine_proto::privileged::PrivilegedOp;
use mixengine_proto::{Error, ErrorCode};

use crate::error::ToWire as _;

use super::{Chosen, Elevation, Route};

impl Elevation {
    /// Ask for this helper to be installed where only an administrator can rewrite it — roadmap
    /// task **T85**.
    ///
    /// **Called at every daemon start, before the other three and for their reason**: what it asks
    /// for lands in the single grant first-run setup already costs rather than behind a prompt of
    /// its own. Before them rather than after, because it is about the file those three are applied
    /// *by*. Reading the answer costs two `stat`s in the ordinary case, which is what makes asking
    /// every time affordable.
    ///
    /// Five answers, and only two of them queue anything (the T85 design, D6):
    ///
    /// - **no helper shipped beside this program** — there is nothing to install, so nothing is
    ///   asked for. That is a daemon somebody moved on its own, and `elevation.grant` already says
    ///   so where a person can act on it;
    /// - **nothing installed** — asked for;
    /// - **installed, and the same bytes** — nothing to do, and this is the ordinary case;
    /// - **installed, and different bytes** — asked for, because a MixEngine upgraded yesterday is
    ///   otherwise driving the helper from the version before it for ever. **That is a replacement
    ///   behind an explicit prompt and not an auto-update**: nothing is copied until somebody
    ///   allows a batch, which is exactly what
    ///   `docs/architecture/security-model.md`'s auto-update boundary asks for. The signature
    ///   check that decides whether the new binary deserved that prompt at all is **T88a**;
    /// - **installed and not an administrator's** — nothing is asked for, and a warning is logged.
    ///   The helper would refuse the operation anyway, and `elevation.status` is already saying so
    ///   through [`mixengine_core::elevation::helper`]'s own refusal.
    ///
    /// A read that fails asks for nothing, as [`require_resolver`](Self::require_resolver) does:
    /// a comparison that could not be made has said nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_helper(&self) -> Result<(), Error> {
        let Some(installed) = self.candidates.installed.as_deref() else {
            tracing::warn!("this machine will not name a directory for a privileged helper");
            return Ok(());
        };

        // **The installed helper is asked first, and whether one is shipped beside this daemon has
        // nothing to do with it.** macOS's package puts the helper in `/Library/PrivilegedHelperTools`
        // and nothing in `/usr/local/bin` beside `mixengined`, so a gate on the beside copy — which
        // is what this used to open with — meant no packaged macOS install ever ran the handshake:
        // the helper upgrade reported *no privileged helper installed* on a machine whose
        // helper had just served two grants, and a daemon restart did not change its mind. The
        // beside copy is what an *install* is made from, and that is the only question it answers.
        if installed.is_file() {
            self.learn_installed_helper().await;
            return Ok(());
        }

        let beside = self
            .candidates
            .program
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX));

        if !beside.is_file() {
            tracing::debug!(
                "no helper is shipped beside this daemon, so there is none to install anywhere"
            );
            return Ok(());
        }

        self.enqueue(&PrivilegedOp::HelperInstall {}).await
    }

    /// Ask the installed helper what it is, and remember the answer for `helper::keep_in_step`.
    ///
    /// **Version and not bytes, and nothing is enqueued** — roadmap task T88a. The bytes beside this
    /// daemon are *not* the newer helper after a `mix self-update`, which keeps the helper by name;
    /// and in a development tree they differ on every rebuild, which put a row on `mix status` whose
    /// only meaning was "you rebuilt". What decides is what the installed helper says it is — and a
    /// replacement needs a signed candidate, which has to be fetched, which is `mix elevation
    /// upgrade`'s job. A daemon start that reached the network would be a start an offline machine
    /// pays for, which `docs/features/updates.md` forbids in as many words.
    ///
    /// Called at start, and again after a prompt that installed or replaced the helper: a daemon
    /// that learned the helper's version only at start went on answering *none installed* until it
    /// was restarted. Not after every prompt — [`may_have_changed_the_helper`] says why.
    ///
    /// [`may_have_changed_the_helper`]: super::may_have_changed_the_helper
    pub(super) async fn learn_installed_helper(&self) {
        let Some(installed) = self.candidates.installed.as_deref() else {
            return;
        };

        if !installed.is_file() {
            return;
        }

        if let Err(error) = mixengine_core::elevation::helper(
            &self.candidates.program,
            self.candidates.installed.as_deref(),
        ) {
            tracing::warn!(
                %error,
                "the installed helper is not one this daemon will run as an administrator"
            );
            return;
        }

        let facts = crate::helper::handshake(installed, &self.home, &self.elevate).await;

        if let Some(facts) = &facts {
            tracing::debug!(
                helper = %facts.version,
                protocol = facts.speaks.0,
                "the installed privileged helper answered a probe"
            );
        }

        if let Ok(mut held) = self.facts.lock() {
            *held = facts;
        }
    }

    /// Ask for the hosts file to say what this home's sites say it should — roadmap task **T41**.
    ///
    /// **The disk is read before a prompt is spent** (T41 design, D11). A machine that already
    /// agrees needs nothing, and enqueueing anyway would put a row on `mix status` whose only
    /// possible outcome is `AlreadyDone`.
    ///
    /// **Here rather than on `Sites`** because this object already holds the `Host` and already owns
    /// the "is this worth a prompt" question. `Sites` gains one dependency and three call sites —
    /// after a successful `create`, `update` and `delete`, and never before, so a failed create asks
    /// for nothing.
    ///
    /// A read that fails does not stop the operation being queued: the helper is the authority on
    /// what is in that file, and it will refuse with the reason on the screen T64 built.
    ///
    /// **What the block should hold depends on which TLDs this machine routes here** — roadmap task
    /// **T45**, design D6. A TLD the resolver sends to our server is answered by pattern, so a hosts
    /// entry under it adds nothing; a TLD nothing routes has no other mechanism. Asking for a block
    /// without the routed names is also what clears one an earlier, unwired daemon left behind —
    /// skipping the queue instead would leave those stale names resolving to loopback for ever.
    ///
    /// **Per TLD rather than per mode**, which is T45's correction to T44: T44 computed the whole
    /// block from one home-wide `DnsMode`, which was right only while nothing could be wired at all.
    /// Every mechanism there is scopes to one TLD, and `.local` is never routed — so a home with
    /// both `blog.test` and `shop.local` needs a block with exactly one line in it.
    ///
    /// # Errors
    ///
    /// The wire error of a home whose sites cannot be read, or whose row cannot be written.
    pub(crate) async fn require_hosts(&self) -> Result<(), Error> {
        let desired = mixengine_core::hosts::desired(&self.store, &self.dns.wired())
            .await
            .map_err(|error| error.to_wire())?;

        // T211g: this home's own block, tagged from its path the way the helper tags it.
        let tag = mixengine_platform::hosts::tag_for(&self.home);
        let wanted = PrivilegedOp::hosts_apply(Some(tag.clone()), desired);

        self.withdraw_a_shared_write().await?;

        // Compared as operations rather than as lists, so the ordering and deduplication are
        // `hosts_apply`'s in both directions and there is one definition of "the same block".
        match self.host.hosts_file().managed(Some(&tag)) {
            // Not a pattern guard: `present` is a `Vec` and a guard cannot move out of one.
            Ok(present)
                if PrivilegedOp::hosts_apply(Some(tag.clone()), present.clone()) == wanted =>
            {
                // Nothing to ask for — and nothing to keep asking for either (T179).
                self.no_longer_needed(&wanted.dedupe_key()).await
            }
            Ok(_) => self.enqueue_needing_a_helper(&wanted).await,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "the hosts file cannot be read; asking for permission to write it anyway"
                );

                self.enqueue_needing_a_helper(&wanted).await
            }
        }
    }

    /// Ask for the hosts blocks under `tags` to be emptied — homes that are gone left them (T211g,
    /// D5). The helper empties one only after its own ledger says that home is gone.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn forget_hosts_blocks(&self, tags: &[String]) -> Result<(), Error> {
        for tag in tags {
            self.enqueue_needing_a_helper(&PrivilegedOp::hosts_apply(
                Some(tag.clone()),
                Vec::new(),
            ))
            .await?;
        }

        Ok(())
    }

    /// Withdraw a row queued before T211g that writes names into the block every home shared.
    ///
    /// The helper now refuses to write that block, so the row could only ever come back refused.
    /// A row that only empties it is an uninstall's, and stays.
    async fn withdraw_a_shared_write(&self) -> Result<(), Error> {
        let shared = PrivilegedOp::hosts_apply(None, Vec::new());

        let writes = self.status().await?.pending.iter().any(|waiting| {
            matches!(&waiting.op, PrivilegedOp::HostsApply { home: None, entries } if !entries.is_empty())
        });

        match writes {
            true => self.no_longer_needed(&shared.dedupe_key()).await,
            false => Ok(()),
        }
    }

    /// Ask for this machine to send its managed TLDs to this daemon's DNS server — roadmap task
    /// **T45**.
    ///
    /// **Called at every daemon start, beside [`require_port_access`](Self::require_port_access),
    /// and that ordering is what makes M4's promise true.** On a fresh home this queues the
    /// operation *before any site exists*, so the single grant of first-run setup wires the machine;
    /// from then on `site.create` computes a hosts block that already matches the disk, enqueues
    /// nothing and prompts for nothing.
    ///
    /// Asking after the first site is created gets that wrong in a way that is invisible until it is
    /// counted: the block would already hold that site's line, emptying it is a second operation,
    /// and a second operation is a second prompt — which is the acceptance criterion this phase is
    /// measured against.
    ///
    /// **A machine with no DNS server of its own asks for nothing**, because there would be nothing
    /// to route names to; and a machine with no scoped mechanism asks for nothing either, which is
    /// a Linux without systemd and is a mode rather than a failure (the T45 design, D2).
    ///
    /// A probe that fails asks for nothing, as `require_port_access` does and unlike
    /// [`require_hosts`](Self::require_hosts): there the helper is the authority on the file and
    /// will refuse with a reason on the screen T64 built, and here a probe that could not read the
    /// machine has said nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_resolver(&self) -> Result<(), Error> {
        let Some(port) = self.dns.wirable_port() else {
            tracing::debug!(
                "no DNS server is answering on a port a resolver could be pointed at, so nothing \
                 is asked for"
            );
            return Ok(());
        };

        let want: Vec<&str> = mixengine_proto::domains::WIRED_TLDS.to_vec();

        let state = match self.host.resolver().probe(&want, port) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "this machine's resolver cannot be read; asking for nothing"
                );
                return Ok(());
            }
        };

        match state.plan(&want, port) {
            Some(plan) => {
                self.enqueue_needing_a_helper(
                    &mixengine_proto::privileged::PrivilegedOp::ResolverApply { plan },
                )
                .await
            }
            // The machine already routes what it should, so a row queued earlier is stale (T179).
            None => self.no_longer_needed("resolver").await,
        }
    }

    /// Ask this machine to trust MixEngine's own certificate authority — roadmap task **T49a**.
    ///
    /// **Called at every daemon start, immediately after the authority is made.** Reading a trust
    /// store costs no privilege on any of the three systems — measured by
    /// `mixengine-platform/tests/trust.rs` in CI's ordinary job, not assumed here — which is what
    /// makes asking on every start affordable, and what notices a store an operating-system update
    /// or another account cleared.
    ///
    /// **And here rather than when the first HTTPS site is created**, which is the ordering T48
    /// generated the authority for: `docs/architecture/security-model.md` promises one elevation
    /// prompt at first run covering the CA, the resolver and the port grant together, and an install
    /// that first appeared with the first site would be a second batch behind a second prompt.
    ///
    /// `None` is a home whose authority could not be made or read: nothing is asked for, because
    /// there is nothing to ask about.
    ///
    /// A probe that fails asks for nothing, as [`require_resolver`](Self::require_resolver) does and
    /// unlike [`require_hosts`](Self::require_hosts): there the helper is the authority on the file
    /// and refuses with a reason on the screen T64 built, and here a store that could not be read has
    /// said nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_trust_store(&self, der: Option<&[u8]>) -> Result<(), Error> {
        let Some(der) = der else {
            tracing::debug!("this home has no certificate authority, so nothing is asked to trust");

            // Nothing to trust means nothing to ask about, including what was asked before (T179).
            return self.no_longer_needed("trust-store").await;
        };

        let state = match self.host.trust_store().probe(der) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "this machine's trust store cannot be read; asking for nothing"
                );
                return Ok(());
            }
        };

        match state.plan(der) {
            Some(plan) => {
                self.enqueue_needing_a_helper(
                    &mixengine_proto::privileged::PrivilegedOp::TrustCaInstall { plan },
                )
                .await
            }
            // This machine already trusts it (T179).
            None => self.no_longer_needed("trust-store").await,
        }
    }

    /// The ports a site is reached on. Fixed, per the T42 design, D2: a front end renumbered to 81
    /// is not a front end anybody asked for, and the recipes say so too.
    pub(crate) const ANSWERING: [u16; 2] = [80, 443];

    /// Ask for this machine to let `binary` answer on 80 and 443 — roadmap task **T42**.
    ///
    /// **Called at every daemon start, and that is also the re-probe the roadmap asks for.** A
    /// capability is cleared by any write to the binary, so an update loses it; asking here catches
    /// that, and catches a loss that was not an update, and needs no hook in the updater. What makes
    /// it affordable is that reading the grant back costs one `getxattr` and no privilege at all —
    /// measured, not assumed.
    ///
    /// `None` is a home with no front end: nothing is asked for. **And nothing is ever revoked
    /// here** — the T42 design, D12: on Linux the question needs the binary, which is precisely what
    /// a home with no front end cannot supply, so "no row, therefore withdraw" is a question this
    /// system cannot be asked. Uninstall (T87) is the producer that can.
    ///
    /// A probe that fails asks for nothing, unlike [`require_hosts`](Self::require_hosts): there the
    /// helper is the authority on the file and will refuse with a reason on the screen T64 built,
    /// and here a probe that could not read one attribute has told us nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_port_access(&self, binary: Option<&Path>) -> Result<(), Error> {
        let Some(binary) = binary else {
            tracing::debug!("this home has no front end, so nothing needs to answer on 80 or 443");
            return Ok(());
        };

        let state = match self.host.port_access().probe(binary, &Self::ANSWERING) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    %error,
                    binary = %binary.display(),
                    "cannot tell whether this machine will let the front end answer on 80 and 443"
                );

                return Ok(());
            }
        };

        if state.granted {
            // The binary already answers, so the grant nobody answered is stale. Withdrawing the
            // request is not revoking the capability: the machine keeps what it holds (T179).
            return self.no_longer_needed("port-access").await;
        }

        // Derived from the method rather than from a `#[cfg]`, which is the whole reason
        // `PortAccessState` carries one — the T42 design, D1.
        let Some(plan) = state.plan(binary) else {
            return self.no_longer_needed("port-access").await;
        };

        if let Some(missing) = &state.missing {
            tracing::info!(%missing, "asking for permission to answer on 80 and 443");
        }

        self.enqueue_needing_a_helper(&PrivilegedOp::PortAccessGrant { plan })
            .await
    }

    /// Everything that has to be true before a prompt can be raised, and the one slot, taken.
    ///
    /// Extracted from [`grant`](Self::grant) so that [`grant_within`](Self::grant_within) makes the
    /// same checks in the same order. **The order is the point**: the queue and the helper are read
    /// *before* the slot is taken, so a machine that cannot prompt is told so without a job row
    /// being written and immediately failed.
    ///
    /// # Errors
    ///
    /// As [`grant`](Self::grant), which is the whole of what this decides.
    pub(super) async fn preflight(
        &self,
        only: Option<&[mixengine_proto::PendingOpId]>,
        route: Route,
    ) -> Result<(Vec<mixengine_proto::PendingOp>, Chosen), Error> {
        let mut waiting = mixengine_core::elevation::pending(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        // T182b, D6: a caller that queued its own operations raises the prompt over those alone.
        if let Some(only) = only {
            waiting.retain(|pending| only.contains(&pending.id));
        }

        if waiting.is_empty() {
            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                "nothing is waiting for permission",
            )
            .with_hint("`mix elevation status` lists what would be asked for"));
        }

        // What a terminal over SSH cannot grant stays queued for a grant raised at the machine's
        // own screen. A terminal at the screen reaches the dialog the operation asks in.
        if route == (Route::ByHand { over_ssh: true }) {
            let machine = self.host.elevation();
            let mut screen_only = None;
            waiting.retain(|pending| match machine.needs_the_screen(&pending.op) {
                Some(reason) => {
                    screen_only.get_or_insert(reason);
                    false
                }
                None => true,
            });

            if let (true, Some(reason)) = (waiting.is_empty(), screen_only) {
                return Err(Error::new(
                    ErrorCode::PrivilegedRequired,
                    format!("this cannot be allowed over SSH: {reason}"),
                )
                .with_hint(at_the_screen(self.reason().is_none())));
            }
        }

        let helper = self.choose_for(&waiting).await?;

        // A terminal raises its own prompt, so only a missing helper stops it — ADR 0065.
        let reason = match route {
            Route::Dialog => self.reason(),
            Route::ByHand { .. } => self.helper_reason(),
        };

        if let Some(reason) = reason {
            return Err(Error::new(
                ErrorCode::PrivilegedRequired,
                format!("this machine cannot raise an elevation prompt: {reason}"),
            ));
        }

        self.reserve()?;

        Ok((waiting, helper))
    }
}

/// Where a row left for the screen can be granted.
///
/// MixLab only when this daemon can raise a dialog: one started over SSH cannot, and a
/// window talking to it has no Allow to offer. A terminal at the screen still can, through `sudo`.
fn at_the_screen(can_prompt: bool) -> &'static str {
    if can_prompt {
        "run `mix elevation grant` on this machine itself, or allow it in MixLab"
    } else {
        "run `mix elevation grant` in a terminal on this machine itself"
    }
}
