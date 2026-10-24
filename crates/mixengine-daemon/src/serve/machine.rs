//! What a start asks of this machine: the grants first-run setup batches into one prompt,
//! and the certificate authority, wherever something has to trust it.

use std::sync::Arc;

use mixengine_core::{Paths, Store};

use super::Shared;
use crate::{background, elevation, services};

/// **Every start asks whether this machine will still let the front end answer on 80 and 443.** A
/// capability is cleared by any write to the binary, so an update loses it and the next start is
/// what notices; the answer costs one read and no privilege. A home with no front end asks for
/// nothing.
///
/// After recovery, because that is what has just rendered the graph this reads the program path
/// out of. Nothing here fails the start, on the rule every block around it follows: a machine
/// that was not asked is one command away from being asked, where refusing to start would leave
/// the user with no daemon at all.
/// **And every start asks whether the file this daemon would run as root is somewhere only an
/// administrator can rewrite.** First of the four, because what it asks for is about the helper the
/// other three are applied *by*; and here rather than behind a prompt of its own for the reason
/// every block in this run of them shares — first-run setup is one grant.
///
/// Nothing here fails the start, on the rule the blocks around it follow: a machine that was not
/// asked is one command away from being asked, where refusing to start would leave the user with
/// no daemon at all.
pub(super) async fn ask_for_grants(
    elevation: &elevation::Elevation,
    services: &services::Registry,
) {
    if let Err(error) = elevation.require_helper().await {
        tracing::warn!(%error, "could not ask for the privileged helper to be installed");
    }

    if let Err(error) = elevation
        .require_port_access(services.front_end_program().await.as_deref())
        .await
    {
        tracing::warn!(%error, "could not ask for permission to answer on 80 and 443");
    }

    // **And every start asks whether this machine still sends its managed TLDs here**, the same
    // shape as the block above for the same reason: reading the wiring costs one file or one
    // registry key and no privilege, so asking on every start is what notices a resolver an OS
    // update, another home, or a person removed.
    //
    // **Here, before any site exists, and that ordering is the point.** On a fresh home this puts
    // the operation in the queue in time for first-run setup's single grant; asking after the first
    // site was created would mean emptying a hosts block that already had a line in it, which is a
    // second operation and therefore a second prompt.
    if let Err(error) = elevation.require_resolver().await {
        tracing::warn!(%error, "could not ask for this machine's managed TLDs to be routed here");
    }

    // After the resolver, whose reading decides what the hosts block should hold: a row queued
    // under another reading is recomputed rather than granted as it stood.
    if let Err(error) = elevation.refresh_a_queued_hosts_row().await {
        tracing::warn!(%error, "could not recompute the queued hosts row");
    }
}

/// **And every start makes sure this home has a certificate authority**, here for the reason the
/// two blocks above are here rather than for one of its own. The machine's trust stores are given
/// this certificate in the same single prompt as the resolver wiring and the port grant; an
/// authority that first appeared when somebody created an HTTPS site would put that install in a
/// second batch and therefore behind a second prompt.
///
/// Idempotent, and never destructive: a home whose authority is damaged keeps it and is told, on
/// the reasoning in `mixengine_core::certs::ca`. Nothing here fails the start, on the rule every
/// block around it follows — a home with no authority is one command away from having one, where
/// a daemon that refuses to start leaves the user with nothing at all.
#[warn(clippy::cognitive_complexity)]
pub(super) async fn certificates(shared: &Shared<'_>, background: &mut background::Background) {
    discard_an_uncommitted_rotation(shared.paths);
    the_authority_and_who_trusts_it(shared).await;
    trust_bundle_in_background(shared, background);
    jdk_trust_stores_in_background(shared, background);
    site_certificates(shared).await;
}

/// **A candidate a rotation never committed does not outlive the daemon that made it.**
/// `certs/pending/` holds a private key nothing uses, and a crash between generating one and
/// deciding about it would leave it there for as long as the home exists. The daemon is single, so
/// at start there is no rotation in flight for this to race.
#[warn(clippy::cognitive_complexity)]
fn discard_an_uncommitted_rotation(paths: &Paths) {
    if let Err(error) = mixengine_core::certs::ca::discard(paths.certs()) {
        tracing::warn!(%error, "a staged certificate authority could not be thrown away");
    }
}

/// Make the authority, then ask this machine and its browsers to trust it.
///
/// **Every start asks whether this machine trusts that authority yet**, immediately after making
/// it and for the same reason it is made here: the install belongs in first-run setup's single
/// grant rather than behind a second prompt. Reading a store costs no privilege on any of the three
/// systems, which is what makes asking on every start affordable — and what notices a store an OS
/// update or another account cleared.
#[warn(clippy::cognitive_complexity)]
async fn the_authority_and_who_trusts_it(shared: &Shared<'_>) {
    let status = match crate::certs::Certificates::new(shared.paths).ensure().await {
        Ok(status) => status,
        Err(error) => {
            tracing::warn!(%error, "could not make this home's certificate authority");
            return;
        }
    };

    // Only a present authority has bytes to install. `Absent` warned above, and `Unusable` is a
    // certificate that exists and is broken — asking a machine to trust one of those would be
    // spending a prompt on something a rotation has to replace anyway.
    let der = match &status.state {
        mixengine_proto::CaState::Present { ca } => {
            mixengine_core::certs::ca::der(&ca.certificate_pem)
        }
        mixengine_proto::CaState::Absent {} | mixengine_proto::CaState::Unusable { .. } => None,
    };

    if let Err(error) = shared.elevation.require_trust_store(der.as_deref()).await {
        tracing::warn!(%error, "could not ask this machine to trust MixEngine's authority");
    }

    trust_in_browsers(shared, &status.state).await;
}

/// **And the browsers, which read none of that.** Firefox and Chrome on Linux carry certificate
/// databases of their own, so a machine whose system store holds this authority still shows a red
/// padlock in both of them.
///
/// Apart from the trust store's request because these databases belong to the user: there is no
/// prompt to batch it into. Nothing about it can fail the start — a machine with no `certutil`, no
/// profile, or a locked one is a machine that keeps working, and `mix doctor` is where it is
/// reported.
#[warn(clippy::cognitive_complexity)]
async fn trust_in_browsers(shared: &Shared<'_>, state: &mixengine_proto::CaState) {
    let change = crate::certs::Certificates::reading(shared.paths, Arc::clone(shared.host))
        .install_in_browsers(state)
        .await;

    if !change.written.is_empty() {
        tracing::info!(
            databases = change.written.len(),
            "wrote MixEngine's authority into this machine's browser databases"
        );
    }

    for refused in change.refused {
        tracing::warn!(%refused, "a browser database would not take the authority");
    }
}

/// **And the runtimes, which read none of that either.** A browser reads the operating system's
/// trust store; Node, Python, Ruby and PHP each carry a set of their own, so a site this machine
/// shows a padlock for is one a `fetch()` in the same project refuses. The bundle is what they can
/// be pointed at: every root this machine trusts, and then ours.
///
/// **Spawned rather than awaited, and that is not a preference.** The endpoint is bound before this
/// step and the accept loop runs after the last one, so — as `started::after_the_api` says in as
/// many words — every moment spent here is a moment a queued client waits for its answer. Reading
/// this machine's whole trust store and writing a quarter of a megabyte is the most expensive thing
/// that was ever put between those two points, and it was measured: before the pipe backlog, when
/// that wait was a refusal on Windows, ten of `tests/api.rs`' twenty-four daemons stopped
/// answering.
///
/// Nothing about it can fail the start, on the rule the authority follows.
#[warn(clippy::cognitive_complexity)]
fn trust_bundle_in_background(shared: &Shared<'_>, background: &mut background::Background) {
    background.spawn(
        "trust bundle",
        write_the_trust_bundle(
            shared.paths.clone(),
            Arc::clone(shared.host),
            shared.store.clone(),
        ),
    );
}

/// The spawned half of [`trust_bundle_in_background`].
#[warn(clippy::cognitive_complexity)]
async fn write_the_trust_bundle(
    paths: Paths,
    host: Arc<dyn mixengine_platform::Host>,
    store: Store,
) {
    if !crate::certs::bundle::render(&paths, host.as_ref()) {
        return;
    }

    // **The bundle is what a PHP's generated ini set names**, so a home that has just got one — or
    // just lost one — needs its `conf.d` written again. The pass at every start runs unconditionally
    // as well; this is the one that catches the first start of a home, where the file did not exist
    // when that pass read for it. Idempotent, and a comparison per installed runtime, so an
    // ordinary start reaches neither.
    if let Err(error) = mixengine_core::runtimes::extensions::refresh_all(&store, &paths).await {
        tracing::warn!(
            %error,
            "the trust bundle moved and the generated conf.d could not be written again"
        );
    }
}

/// **And every installed JDK, which reads neither the machine's store nor that bundle** — ADR
/// 0039. A JDK verifies against its own `lib/security/cacerts`, and no variable adds an authority
/// to it, so this is the only way a `java` reaches an HTTPS site of this home's.
///
/// Spawned for the trust bundle's reason, and it matters more here: this is one `keytool` per
/// installed JDK, each of them a JVM start, and none of it may stand between the bind and
/// `accept`. A home with no Java spends one query on it.
#[warn(clippy::cognitive_complexity)]
fn jdk_trust_stores_in_background(shared: &Shared<'_>, background: &mut background::Background) {
    let store = shared.store.clone();
    let certs = shared.paths.certs().to_path_buf();

    background.spawn("jdk trust stores", async move {
        crate::certs::jdks::hold(&store, &certs)
            .await
            .log("at start");
    });
}

/// **And every site that declares HTTPS gets the certificate its names need**, here and not inside
/// any of the generators. `CLAUDE.md` says generated configuration is disposable and rebuilt from
/// the database; a certificate is state, cannot be rebuilt from a row, and throwing one away costs
/// the trust of every browser holding a cached chain. So issuance is a *precondition* of
/// generation rather than part of it, and the front end relies on this ordering when it wires these
/// files in.
///
/// Nothing here fails the start, on the rule every block around it follows: a site with no
/// certificate serves over HTTP and is reported by `mix doctor`, where a daemon that refused to
/// start would leave the user with nothing at all.
#[warn(clippy::cognitive_complexity)]
async fn site_certificates(shared: &Shared<'_>) {
    let report = match crate::certs::Certificates::issuing(
        shared.paths,
        Arc::clone(shared.host),
        shared.store.clone(),
    )
    .issue(None)
    .await
    {
        Ok(report) => report,
        Err(error) => {
            tracing::warn!(%error, "this home's sites could not be issued certificates");
            return;
        }
    };

    say_what_was_issued(report);
}

/// What issuing the sites' certificates did, in `daemon.log`.
#[warn(clippy::cognitive_complexity)]
fn say_what_was_issued(report: mixengine_proto::CertIssueReport) {
    let issued = report
        .sites
        .iter()
        .filter(|site| matches!(site.outcome, mixengine_proto::IssueOutcome::Issued {}))
        .count();

    if issued > 0 {
        tracing::info!(sites = issued, "signed certificates for this home's sites");
    }

    for site in report.sites {
        if let mixengine_proto::IssueOutcome::Refused { because } = site.outcome {
            tracing::warn!(domain = %site.domain, %because, "no certificate for this site");
        }
    }
}
