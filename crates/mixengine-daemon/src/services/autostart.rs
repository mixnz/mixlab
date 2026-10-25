//! What starts when the daemon does.
//!
//! **A plan, not a loop over flagged rows.** A php-fpm pool that is set and depends on a MariaDB
//! that is not must bring MariaDB up, because a pool whose database is missing is a pool that fails
//! its health check. So the flagged ids are the *roots* of a
//! [`ServiceGraph::start_plan`](mixengine_core::services::graph::ServiceGraph::start_plan) and never
//! the walk itself, and a service with the setting off can be started here by something that depends
//! on it — the same rule `mix service start php-fpm@8.3` already follows, and the reason
//! `mix service autostart` says so in its own help.
//!
//! **Spawned after the endpoint is serving**, which is the one thing separating this from the four
//! loops it sits beside in `serve`. Those are sweeps whose first pass must not race a client; this
//! one starts real programs and takes seconds, and a daemon that will not answer `daemon.status`
//! until MariaDB's first run has finished looks hung at exactly the moment somebody is looking at
//! it. Every state it reaches is announced on the event stream a client is already reading, so a
//! dashboard fills in as it goes rather than appearing complete.
//!
//! **And after recovery**, on `Registry::recover`'s own reasoning: a reading must never be taken of
//! a service this daemon has not decided about yet. A service recovery adopted is already up, and
//! `Registry::start` counts one that is up as reached rather than restarting it — so an adopted
//! service is not disturbed by having been flagged.
//!
//! **One attempt, and the failure is a state.** The two reasons a boot start fails are a port
//! somebody else holds and a program that is not there any more, and neither is fixed by trying
//! again thirty seconds later. Both are already reported — [`StateReason::PortInUse`] names the
//! process holding the port — and both are already on the row a client lists. There is no retry, no
//! backoff and no second pass here; what puts a *crashed* service back is the restart policy, which
//! is a different question and already has an answer.
//!
//! **It is not the idle sweeper's opposite.** A service that is set here and also has an idle
//! timeout starts at boot and is stopped again when nothing uses it, and that is two settings
//! answering different questions rather than one overruling the other. Nothing in this file exempts
//! anything from anything.

use std::sync::Arc;

use mixengine_core::Store;
use mixengine_proto::StateReason;
use tokio_util::sync::CancellationToken;

use super::Registry;

/// Start what this home asked to have running, behind an endpoint that is already serving.
///
/// Spawned rather than awaited, and cancelled by the same token as its four siblings: a daemon told
/// to stop while it is still walking stops walking, and what it has already started is stopped by
/// the shutdown that follows — in reverse dependency order, by the code that owns that question.
pub(crate) fn start(
    background: &crate::background::Background,
    services: Arc<Registry>,
    store: Store,
    shutdown: CancellationToken,
) {
    background.spawn("autostart", async move {
        tokio::select! {
            () = shutdown.cancelled() => {}
            () = walk(services, store) => {}
        }
    });
}

/// The walk itself.
///
/// Every failure to *read* something ends the walk with a line and starts nothing, rather than
/// starting the part it could work out: a home whose rows or graph cannot be read is one where
/// "what asked to start" has no answer, and starting a guess at it is worse than starting nothing.
/// Each of those is already a `mix doctor` check and already has its own line at daemon start.
async fn walk(services: Arc<Registry>, store: Store) {
    let roots = match mixengine_core::services::autostart_ids(&store).await {
        Ok(roots) => roots,
        Err(error) => {
            tracing::warn!(%error, "autostart could not read which services asked to start");
            return;
        }
    };

    if roots.is_empty() {
        tracing::debug!("nothing in this home asked to start with the daemon");
        return;
    }

    let graph = match services.graph().await {
        Ok(graph) => graph,
        Err(error) => {
            tracing::warn!(?error, "autostart could not read what this home declares");
            return;
        }
    };

    // A row flagged for a service nothing declares any more — a package uninstalled under it, a
    // hand-edited database — takes the whole walk down rather than being passed over, because the
    // plan is built from the roots as one set and a caller cannot say which root was the bad one.
    // Reported and not repaired: `mix doctor` is what says a home has a row nothing declares.
    let plan = match graph.start_plan(&roots) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(
                %error,
                "autostart names a service this home does not declare, so nothing was started"
            );
            return;
        }
    };

    tracing::info!(
        asked = roots.len(),
        planned = plan.flat().count(),
        "starting what asked to start with the daemon"
    );

    let walk = services
        .start_because(&graph, &plan, StateReason::Autostart)
        .await;

    match &walk.failed {
        None => tracing::info!(
            reached = walk.reached.len(),
            "everything that asked to start with the daemon is up"
        ),

        // One line and no retry. What a person acts on is the row and the event, both of which
        // carry the reason; this exists so that `daemon.log` explains a machine nobody was watching.
        Some((service, reason)) => tracing::warn!(
            service = service.as_str(),
            reason = ?reason,
            reached = walk.reached.len(),
            blocked = walk.blocked.len(),
            "a service that asked to start with the daemon did not, and nothing after it was tried"
        ),
    }
}
