//! The work a start leaves running behind it, owned until shutdown — CI run 37679651605.
//!
//! **Why this exists rather than `tokio::spawn`.** A start hands several slow jobs to the runtime so
//! that none of them stands between the bind and the first `accept`: the trust bundle, the JDKs, the
//! helper's handshake, the PHP pools. Spawned and dropped, they ran on while the daemon shut down —
//! the trust bundle's `conf.d` pass met a store that had already been closed, and a daemon asked to
//! stop 60 ms after it started was still alive forty seconds later. `docs/standards/rust.md` says it
//! in one line: no detached task outlives shutdown.
//!
//! **Waited for beside the clients, not after them.** A shutdown's arithmetic has one slot for the
//! wait between the services and `Store::close`, sized by `CLIENT_GRACE` or `SIGNAL_CLIENT_GRACE`;
//! a second wait after the first would be time nobody budgeted. So [`Background::shut_down`] is
//! given the same grace and runs inside the same wait. What has not finished by then is named in
//! the log and dropped, which releases any connection it held, so the close that follows has nothing
//! to wait on.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use tokio::task::{Id, JoinSet};

/// The tasks a start left running, each with a name for the log.
#[derive(Debug, Default)]
pub(crate) struct Background {
    tasks: JoinSet<()>,
    names: HashMap<Id, &'static str>,
}

impl Background {
    /// Nothing running yet.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Run `work` until it finishes or the daemon shuts down, whichever is first.
    ///
    /// `name` is what the log calls it if it is still running when the daemon stops — the one fact
    /// the next unreadable shutdown needs.
    pub(crate) fn spawn<F>(&mut self, name: &'static str, work: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.tasks.spawn(work).id();

        self.names.insert(id, name);
    }

    /// Wait up to `grace` for what is still running, then stop the rest and say which they were.
    ///
    /// Returns the names it stopped, in no particular order. **Stopping is dropping**: a task is
    /// ended at its next `.await`, and whatever it held — a connection to the store above all — is
    /// released by that drop.
    pub(crate) async fn shut_down(&mut self, grace: Duration) -> Vec<&'static str> {
        let _ = tokio::time::timeout(grace, async {
            while let Some(finished) = self.tasks.join_next_with_id().await {
                match finished {
                    Ok((id, ())) => {
                        self.names.remove(&id);
                    }

                    Err(error) => {
                        let name = self.names.remove(&error.id()).unwrap_or("unnamed");

                        tracing::warn!(task = name, %error, "a task left running at start did not finish cleanly");
                    }
                }
            }
        })
        .await;

        let stopped: Vec<&'static str> = self.names.drain().map(|(_, name)| name).collect();

        if !stopped.is_empty() {
            tracing::warn!(
                tasks = ?stopped,
                ?grace,
                "work left running at start was still running when this daemon stopped; it is \
                 stopped here, before the database closes"
            );
        }

        self.tasks.shutdown().await;

        stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Instant;

    use mixengine_core::Store;

    /// What finishes in time is not named, and what does not is — and is stopped.
    #[tokio::test]
    async fn a_task_still_running_at_shutdown_is_named_and_stopped() {
        let mut background = Background::new();

        background.spawn("finishes", async {});
        background.spawn("never finishes", std::future::pending());

        let began = Instant::now();
        let stopped = background.shut_down(Duration::from_millis(200)).await;

        assert_eq!(stopped, vec!["never finishes"]);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the shutdown waited {:?} for a grace of 200 ms",
            began.elapsed()
        );
    }

    /// **The defect itself**: a task holding a connection kept `Store::close` waiting.
    ///
    /// `Pool::close` waits for every checked-out connection to come back. A start's task that held
    /// one across an `.await` that did not end held the daemon open with it; stopped here, the
    /// connection is dropped and the close has nothing left to wait for.
    #[tokio::test]
    async fn a_task_holding_the_store_does_not_keep_it_from_closing() {
        let home = tempfile::tempdir().expect("a directory");
        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a store");

        let (holding, held) = tokio::sync::oneshot::channel();
        let mut background = Background::new();

        background.spawn("holds a connection", {
            let store = store.clone();

            async move {
                let _connection = store.pool().acquire().await.expect("a connection");
                let _ = holding.send(());

                std::future::pending::<()>().await;
            }
        });

        held.await.expect("the task took its connection");

        let stopped = background.shut_down(Duration::from_millis(200)).await;
        assert_eq!(stopped, vec!["holds a connection"]);

        assert!(
            tokio::time::timeout(Duration::from_secs(10), store.close())
                .await
                .is_ok(),
            "the store would not close although the task holding a connection was stopped"
        );
    }
}
