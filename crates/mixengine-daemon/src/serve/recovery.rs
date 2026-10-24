//! What the last daemon left behind, reconciled before the first client: rows that claim a
//! supervisor nobody is, adopted, stopped or cleared, and one line saying which.

use crate::services;

/// **Before the first client, and after the listener is bound.** A daemon that was killed leaves
/// rows claiming a supervisor that no longer exists, and until they are reconciled `service.list`
/// would report a machine that does not exist: services running with nothing behind them. Doing it
/// here rather than earlier costs nothing and buys the ordering that matters, which is that the
/// single-instance lock is long since held: no second daemon can be looking at these rows, and
/// nothing that survived can be adopted twice.
///
/// Nothing here fails the start. A survivor that cannot be stopped, a row that cannot be
/// cleared, a source that cannot say what is declared — each is reported and each leaves one
/// service in a state a user can see and act on, where refusing to start would leave them with a
/// machine that has no daemon at all.
///
/// **Stale endpoint files are not part of it**, although the architecture document lists them in
/// the same sentence. `ipc::Listener::bind` already unlinks a socket nothing answers on and binds
/// again, and there is no pid file to go stale: `run/mixengined.lock` is held as an open handle the
/// OS releases even when the daemon is killed, so the file surviving means nothing and its contents
/// are rewritten by whoever takes the lock next.
pub(super) async fn recover(services: &services::Registry) {
    let recovered = services.recover().await;

    if recovered.is_empty() {
        tracing::debug!("nothing was left running by a previous daemon");
    } else if recovered.refused.is_empty() {
        tracing::info!(
            adopted = ?recovered.adopted,
            stopped = ?recovered.stopped,
            cleared = ?recovered.cleared,
            "reconciled what the last daemon left behind"
        );
    } else {
        // **Warn rather than info, and said differently**, because this is the one boot where the
        // sentence above would be untrue: something the last daemon left behind is still running,
        // still holding whatever it held, and its row still names it. Each of them has its own
        // `error!` from the registry saying which and why; this is the line that stops the summary
        // from reading like a clean start.
        tracing::warn!(
            adopted = ?recovered.adopted,
            stopped = ?recovered.stopped,
            cleared = ?recovered.cleared,
            refused = ?recovered.refused,
            "could not reconcile everything the last daemon left behind; the services listed as \
             refused are still running with nothing supervising them"
        );
    }
}
