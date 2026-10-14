//! The running daemon's two last acts: accepting clients until something asks it to stop,
//! then stopping — services and jobs within the budget, then clients and the work a start
//! left running, within their grace.

use std::sync::Arc;
use std::time::Duration;

use mixengine_platform::{ipc, signal};
use tokio_util::sync::CancellationToken;

use super::{
    ACCEPT_PAUSE, CLIENT_GRACE, KILL_GRACE, SIGNAL_CLIENT_GRACE, shut_down, signalled_budget,
};
use crate::{api, background, jobs, services};

/// Connections are tracked rather than detached, because `docs/standards/rust.md` forbids a
/// task that outlives shutdown and because a `/events` stream would otherwise be cut mid-frame.
pub(super) async fn accept_until_stopped(
    listener: &mut ipc::Backlog,
    signals: &mut signal::Signals,
    api: &Arc<api::Api>,
    services: &services::Registry,
    shutdown: &CancellationToken,
    shutdown_grace: Duration,
) -> (tokio::task::JoinSet<()>, bool) {
    let mut connections = tokio::task::JoinSet::new();

    // Which of the two shutdowns this turned out to be, kept because the connections' grace differs
    // between them and nothing below can tell them apart afterwards — see `SIGNAL_CLIENT_GRACE`. It
    // is set on every path where the OS has started counting, including a console event that arrives
    // during a `daemon.shutdown` that was already under way.
    let mut on_the_os_clock = false;

    loop {
        tokio::select! {
            // Ctrl-C, `systemctl --user stop`, the console closing, the machine shutting down —
            // whichever of them this OS has. Cancel safe, so a turn that serves a client instead
            // has not swallowed one.
            stop = signals.stopped() => {
                // **The budget is granted before the token is cancelled, and that order is the
                // whole of the signal half of T9a.** Cancelling first would release every runner
                // into the stop its spec asks for, with nothing having said how long the *daemon*
                // has — and on Windows the OS is already counting.
                let budget = signalled_budget(shutdown_grace);

                tracing::info!(%stop, ?budget, "shutting down");
                on_the_os_clock = true;
                services.stopping_within(budget);
                shutdown.cancel();
                break;
            }

            // Cancelled by something inside the daemon rather than by the OS: `daemon.shutdown`,
            // which has already stopped the services in dependency order and granted its own budget
            // before it got here (T9a). The loop understands both, and neither is the only one.
            () = shutdown.cancelled() => {
                tracing::info!("shutting down");
                break;
            }

            accepted = listener.accept() => match accepted {
                Ok(ipc::Accepted::Trusted(connection)) => {
                    tracing::debug!("a client connected");
                    connections.spawn(api::serve_connection(Arc::clone(api), connection));
                }

                // Not an error and not a failure of anything: the endpoint's own permissions
                // should already have made this impossible, so it is worth a line saying whose
                // connection was turned away and never worth ending the loop over.
                Ok(ipc::Accepted::Untrusted(peer)) => {
                    tracing::warn!(%peer, "refused a connection from another account");
                }

                Err(error) => {
                    tracing::warn!(%error, "cannot accept a connection");
                    tokio::time::sleep(ACCEPT_PAUSE).await;
                }
            },

            // Reaped as they finish rather than only at shutdown, so a daemon a client has been
            // connecting to all day does not accumulate one completed task per connection.
            Some(finished) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = finished {
                    tracing::warn!(%error, "a client connection task did not finish cleanly");
                }
            }
        }
    }

    (connections, on_the_os_clock)
}

/// Before the connections, and that order is the point: a service is what a user loses if this
/// process exits while it is still flushing, and a client mid-request is not. The root token is
/// already cancelled, so every runner is performing the stop its spec asks for; this is where the
/// daemon waits for them instead of leaving the job to a destructor that kills.
///
/// **Pinned and selected against rather than simply awaited, because this wait is the one part of
/// a shutdown a person can be trapped in** — roadmap task T9a. Leaving the accept loop used to be
/// the last time anything read `signals`, although the handlers stay installed for the rest of
/// `serve`: on Unix tokio's `SIGINT`/`SIGTERM` handlers are process-global and permanent,
/// so the default disposition is gone, and a second Ctrl-C during a stop that had wedged on one
/// service was delivered into a channel nobody was reading and did nothing whatever. The only
/// escape left was `kill -9`, which is the outcome this task exists to remove.
/// **Jobs stop beside the services rather than after them** — roadmap task T22. The root token
/// has already cancelled both, and what is left is the waiting: a service holds a port and a data
/// directory, a job holds a staging directory it is the only thing that can remove. Neither wait
/// is shortened by the other finishing, so running them in sequence would add one budget to the
/// other — which is the arithmetic T9a's single budget exists to prevent. A job that will not
/// stop inside it is left, and its row is the next daemon's `abandon` to close.
pub(super) async fn stop_everything(
    signals: &mut signal::Signals,
    services: &services::Registry,
    jobs: &jobs::Jobs,
    shutdown_grace: Duration,
    connections: tokio::task::JoinSet<()>,
    mut on_the_os_clock: bool,
    background: &mut background::Background,
) {
    let mut stopping = std::pin::pin!(async {
        tokio::join!(services.shut_down(), jobs.shut_down(shutdown_grace));
    });

    tokio::select! {
        () = &mut stopping => {}

        // **An escalation and not an exit.** Returning from `serve` here would skip `Store::close`
        // and leave behind the `-wal` sidecar every number in `serve.rs` is sized around — one bad
        // outcome traded for a worse one. What a second request means is that the person is no
        // longer willing to wait for the polite stop, so the budget is narrowed to nothing and
        // every runner still to reach a stop goes straight to the kill: `Registry::stopping_within`
        // is the narrow-only mechanism T9a already built for a second shutdown arriving during a
        // first, and a second answer to the same question here would be one for them to disagree
        // about.
        //
        // Then the *same* future is waited on again rather than dropped, because dropping it detaches
        // the runners it drained out of the registry and the children they own are orphaned instead
        // of reaped — the thing the wait was for. Bounded by `KILL_GRACE` so that a third signal is
        // never the answer.
        stop = signals.stopped() => {
            on_the_os_clock = true;

            tracing::warn!(
                %stop,
                grace = ?KILL_GRACE,
                "asked to stop again while services were still stopping; killing them now"
            );

            services.stopping_within(Duration::ZERO);

            if tokio::time::timeout(KILL_GRACE, &mut stopping).await.is_err() {
                tracing::warn!(
                    "some services were still stopping when the escalated shutdown stopped waiting \
                     for them; whatever they had left goes with this process"
                );
            }
        }
    }

    let grace = if on_the_os_clock {
        SIGNAL_CLIENT_GRACE
    } else {
        CLIENT_GRACE
    };

    // **Together, inside the one grace** — CI run 37679651605. The work a start left running reads
    // and writes the store `main` closes next; left to run on, the trust bundle's `conf.d` pass met
    // a closed store and a daemon asked to stop stayed alive for forty seconds. Run beside the
    // clients rather than after them, so the shutdown's arithmetic has no new term in it.
    tokio::join!(shut_down(connections, grace), background.shut_down(grace));
}
