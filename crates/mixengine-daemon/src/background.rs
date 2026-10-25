//! The work this daemon leaves running behind a call, owned until shutdown — CI run 37679651605.
//!
//! **Why this exists rather than `tokio::spawn`.** A start hands several slow jobs to the runtime so
//! that none of them stands between the bind and the first `accept`: the trust bundle, the JDKs, the
//! helper's handshake, the PHP pools. Spawned and dropped, they ran on while the daemon shut down —
//! the trust bundle's `conf.d` pass met a store that had already been closed, and a daemon asked to
//! stop 60 ms after it started was still alive forty seconds later. `docs/standards/rust.md` says it
//! in one line: no detached task outlives shutdown.
//!
//! **Not only the start's.** The clocks, a walk nobody waits for, every request's handler and every
//! connection an activator carries run here too, reached through `api::Shutdown::background` and
//! `services::Registry::background`. One handle, cloned: there is one set, and one shutdown that
//! drains it.
//!
//! **Reaped as it goes.** A request or a proxied connection is an entry, so a set emptied only at
//! shutdown would grow for as long as the daemon runs. Every [`Background::spawn`] first collects
//! what has already finished.
//!
//! **Waited for beside the clients, not after them.** A shutdown's arithmetic has one slot for the
//! wait between the services and `Store::close`, sized by `CLIENT_GRACE` or `SIGNAL_CLIENT_GRACE`;
//! a second wait after the first would be time nobody budgeted. So [`Background::shut_down`] is
//! given the same grace and runs inside the same wait. What has not finished by then is named in
//! the log and dropped, which releases any connection it held, so the close that follows has nothing
//! to wait on.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::task::{Id, JoinError, JoinSet};

/// The tasks this daemon left running, each with a name for the log.
///
/// A handle: clones share one set.
#[derive(Debug, Clone, Default)]
pub(crate) struct Background(Arc<Mutex<Inner>>);

#[derive(Debug, Default)]
struct Inner {
    tasks: JoinSet<()>,
    names: HashMap<Id, &'static str>,

    /// Set by [`Background::shut_down`]; nothing spawned afterwards runs.
    closed: bool,
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
    ///
    /// **Once the shutdown has begun, `work` is dropped unstarted.** The root token is cancelled by
    /// then, so it would end at its first `select!`; starting it would only reopen the race with
    /// `Store::close` this set exists to close.
    pub(crate) fn spawn<F>(&self, name: &'static str, work: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut inner = self.lock();

        if inner.closed {
            tracing::debug!(task = name, "not started: this daemon is already stopping");
            return;
        }

        while let Some(outcome) = inner.tasks.try_join_next_with_id() {
            finished(&mut inner.names, outcome);
        }

        let id = inner.tasks.spawn(work).id();

        inner.names.insert(id, name);
    }

    /// Wait up to `grace` for what is still running, then stop the rest and say which they were.
    ///
    /// Returns the names it stopped, in no particular order. **Stopping is dropping**: a task is
    /// ended at its next `.await`, and whatever it held — a connection to the store above all — is
    /// released by that drop.
    pub(crate) async fn shut_down(&self, grace: Duration) -> Vec<&'static str> {
        // Taken out of the lock rather than joined inside it: the lock is a `std` one, and is never
        // held across an `.await`.
        let (mut tasks, mut names) = {
            let mut inner = self.lock();
            inner.closed = true;

            (
                std::mem::take(&mut inner.tasks),
                std::mem::take(&mut inner.names),
            )
        };

        let _ = tokio::time::timeout(grace, async {
            while let Some(outcome) = tasks.join_next_with_id().await {
                finished(&mut names, outcome);
            }
        })
        .await;

        let stopped: Vec<&'static str> = names.drain().map(|(_, name)| name).collect();

        if !stopped.is_empty() {
            tracing::warn!(
                tasks = ?stopped,
                ?grace,
                "work left running was still running when this daemon stopped; it is stopped \
                 here, before the database closes"
            );
        }

        tasks.shutdown().await;

        stopped
    }

    /// How many tasks the set still holds.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().tasks.len()
    }

    /// The set, whoever panicked while holding it: nothing in it is left half-written by a panic,
    /// since every change is one insert or one `take`.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Forget a finished task's name, and say so if it did not finish cleanly.
fn finished(names: &mut HashMap<Id, &'static str>, outcome: Result<(Id, ()), JoinError>) {
    match outcome {
        Ok((id, ())) => {
            names.remove(&id);
        }

        Err(error) => {
            let name = names.remove(&error.id()).unwrap_or("unnamed");

            tracing::warn!(task = name, %error, "a task left running did not finish cleanly");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use mixengine_core::Store;

    /// What finishes in time is not named, and what does not is — and is stopped.
    #[tokio::test]
    async fn a_task_still_running_at_shutdown_is_named_and_stopped() {
        let background = Background::new();

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

    /// A task spawned once the shutdown has begun never runs: the root token is already cancelled,
    /// and running it would only reopen the race with `Store::close`.
    #[tokio::test]
    async fn a_task_spawned_after_the_shutdown_began_never_runs() {
        let background = Background::new();
        background.shut_down(Duration::from_millis(50)).await;

        let ran = Arc::new(AtomicBool::new(false));
        background.spawn("too late", {
            let ran = Arc::clone(&ran);

            async move { ran.store(true, Ordering::SeqCst) }
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !ran.load(Ordering::SeqCst),
            "a spawn after the shutdown ran"
        );
    }

    /// A finished task is gone from the set by the next spawn, so a daemon that serves requests all
    /// day does not hold one completed task per request.
    #[tokio::test]
    async fn a_finished_task_is_reaped_by_the_next_spawn() {
        let background = Background::new();

        background.spawn("finishes", async {});
        tokio::time::sleep(Duration::from_millis(50)).await;
        background.spawn("never finishes", std::future::pending());

        assert_eq!(background.len(), 1);
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
        let background = Background::new();

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
