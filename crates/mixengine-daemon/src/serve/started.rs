//! What a start does once the API exists and before the first client is accepted: the
//! work that needs the API's own `Sites` and `Extensions`, and what an update left behind.

use std::sync::Arc;

use mixengine_core::{Paths, config};
use tokio_util::sync::CancellationToken;

use crate::{api, background, elevation, services, updates};

/// **What this home was already sharing, announced again.** The rows outlive the daemon and the
/// advertisement does not, so a restart with a site shared would otherwise leave a name that
/// resolved yesterday and resolves nothing today. Whole state, through the same call every share
/// and unshare makes.
///
/// Not fatal, on the rule the responder itself follows: a home that cannot advertise is a home
/// whose shared sites are reached by address.
pub(super) async fn after_the_api(
    api: &api::Api,
    elevation: &elevation::Elevation,
    config: &config::Config,
    shutdown: &CancellationToken,
    background: &mut background::Background,
) {
    if let Err(error) = api.sites.advertises_what_it_declares().await {
        tracing::warn!(%error, "this home's shared sites are reachable by address only");
    }

    // **Every installed `web-app`'s generated configuration, written again.**
    // The rule `etc/` follows, applied to the one generated file that has to live inside an install
    // directory because the application it configures says so: it is ours, it is written from the
    // rows, and it is thrown away. Writing it here is what makes a database that was re-provisioned
    // or a port that moved take effect on the next start, with no repair anybody has to know to run.
    //
    // Nothing here fails the start, on the rule the pool block in `repairs` follows: a `web-app`
    // whose configuration could not be written is a tool that shows its own setup screen, where
    // refusing to start would leave the user with no daemon at all. Each extension is logged on its
    // own.
    //
    // Spawned rather than awaited, for the reason the activation block gives in as many words: the
    // endpoint is bound and the accept loop is still below, and every moment spent here is a moment
    // a queued client waits for its answer.
    background.spawn("php extension pools", {
        let extensions = Arc::clone(&api.extensions);

        async move {
            // **Before the configuration is written.** A `web-app` installed before extensions had
            // a pool of their own is still served on the shared pool, and the file written below
            // belongs to the site as it will be served — so the repair goes first. Idempotent, and
            // one query on a home with no extension sites, which is what lets it run at every boot
            // instead of being a migration somebody has to know to run.
            extensions.ensure_pools().await;
            extensions.configure().await;
        }
    });

    // **And a third clock ends a share nobody ended.** The same shape as the renewal and idle loops
    // in `clocks`, and here rather than beside them for one reason: it needs the `Sites` the API
    // holds, and a second one built for it would answer a different question about the same home.
    //
    // After the reconciliation in `recovery`, so its first pass never reads a home this daemon has
    // not finished starting; before the accept loop, so it does not race the first client.
    crate::sites::revoke::start(
        Arc::clone(&api.sites),
        elevation.host(),
        std::time::Duration::from_secs(config.sharing.check_seconds),
        shutdown.clone(),
    );
}

/// **And what an update left behind, before anything else asks about it.** Reads the two records
/// the daemon that replaced these binaries wrote, deletes them, checks that this build is the
/// version that release declared, discards the `.old` files it can, and starts the services that
/// were running. On every ordinary start it reads two absent rows and returns.
///
/// Spawned rather than awaited, on the extension-configuration block's reasoning in as many
/// words: the endpoint is bound and the accept loop is still below, and every moment spent here
/// is a moment a queued client waits for its answer.
pub(super) fn after_an_update(
    updates: &Arc<updates::Updates>,
    services: &Arc<services::Registry>,
    elevation: &Arc<elevation::Elevation>,
    paths: &Paths,
    background: &mut background::Background,
) {
    background.spawn("restore after update", {
        let updates = Arc::clone(updates);
        let services = Arc::clone(services);

        async move {
            // The names an update would have replaced, which is the same list on every install this
            // release knows how to make. A `.old` that is not there is not an error — see
            // `updates::apply::discard_old`.
            let replaced = ["mix".to_owned(), "mixengined".to_owned()];

            updates.restore_after_update(&services, &replaced).await;
        }
    });

    // **And the installed helper, brought into step with this release.** After the update's own
    // restore, so a start that follows an update reads the helper the update left. Spawned for the
    // same reason as the block above, and because fetching this release's signed helper can wait on
    // the network, which a start must never do.
    background.spawn("helper handshake", {
        let elevation = Arc::clone(elevation);
        let updates = Arc::clone(updates);
        let paths = paths.clone();

        async move {
            let row = crate::helper::keep_in_step(&elevation, &updates, &paths).await;
            tracing::debug!(
                ?row,
                "the installed privileged helper was compared with this release"
            );
        }
    });
}
