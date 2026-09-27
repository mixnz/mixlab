//! Noticing that somebody installed a tool into a runtime — roadmap tasks **T131** and **T185b**.
//!
//! `npm install -g yarn` writes a program into the Node install's own bindir and tells nobody. This
//! is what turns that into a command: a watch on each installed runtime's bindir, and — when one
//! gains or loses an entry — a re-scan that rewrites [`bin_commands`](mixengine_core::bin_commands)
//! and refills `<root>/bin`.
//!
//! # Nothing runs on a timer
//!
//! T131 shipped a two-second `stat` loop here. It is gone: the kernel says when a bindir changes
//! ([`mixengine_platform::watch`]), so an idle machine pays nothing, and everything the daemon does
//! itself — installing or removing a runtime, starting — brings `bin/` up to date before it answers
//! rather than leaving it for a pass to find. ADR 0057 records the change.
//!
//! # What is watched changes with what is installed
//!
//! A runtime installed is a bindir to add, one removed is a bindir to drop, and a fresh Python's
//! `Scripts` appears only with its first `pip install`. So the watch is armed again after every
//! change it hears and whenever [`Shims::runtimes_changed`] says the rows moved — always *before*
//! the re-scan, so a file written between the two is either read by the scan or heard by the new
//! watch.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mixengine_core::{Store, runtimes};
use tokio::sync::Notify;

use crate::shims::Shims;

/// How long a bindir has to be quiet before it is scanned.
///
/// `npm install -g` writes three files per tool on Windows and a link on Unix, and `pip` writes one
/// per entry point; scanning after the first would find the rest missing and scan again. This is a
/// wait after an event, never a period: with nothing happening, nothing waits.
const SETTLE: Duration = Duration::from_millis(250);

/// Start watching, and hand back the handle that stops it.
///
/// Nothing here can fail the daemon: a bindir that cannot be watched is logged, every command the
/// daemon runs itself still leaves `bin/` right, and `mix path rescan` is there for the rest.
pub(crate) fn start(shims: Arc<Shims>, store: Store) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let changed = Arc::new(Notify::new());
        let mut heard = false;

        loop {
            let _watch = arm(&store, &changed).await;

            if heard {
                heard = false;

                match shims.rescan().await {
                    Ok(refreshed)
                        if refreshed.written.is_empty() && refreshed.removed.is_empty() => {}
                    Ok(refreshed) => tracing::info!(
                        written = ?refreshed.written,
                        removed = ?refreshed.removed,
                        "bin/ caught up with what is installed"
                    ),
                    Err(error) => tracing::warn!(
                        %error,
                        "a tool installed into a runtime could not be made a command"
                    ),
                }
            }

            tokio::select! {
                () = changed.notified() => {
                    settle(&changed).await;
                    heard = true;
                }
                () = shims.rearmed() => {}
            }
        }
    })
}

/// Wait until [`SETTLE`] passes with no further change.
async fn settle(changed: &Notify) {
    while tokio::time::timeout(SETTLE, changed.notified())
        .await
        .is_ok()
    {}
}

/// A watch on each installed runtime's bindir, as the rows say now — whether or not a bindir
/// exists yet.
async fn arm(store: &Store, changed: &Arc<Notify>) -> Option<mixengine_platform::watch::Watch> {
    let installed = match runtimes::records(store, None).await {
        Ok(installed) => installed,
        Err(error) => {
            tracing::warn!(%error, "could not read which runtimes to watch for installed tools");
            return None;
        }
    };

    let bindirs: Vec<PathBuf> = installed
        .into_iter()
        .filter_map(|runtime| {
            runtimes::globals::directory(runtime.kind, std::path::Path::new(&runtime.path))
        })
        .collect();

    let notifier = Arc::clone(changed);
    let (watch, unwatched) = mixengine_platform::watch::watch(&bindirs, move || {
        notifier.notify_one();
    });

    for unwatched in unwatched {
        tracing::warn!(
            directory = %unwatched.path.display(),
            reason = %unwatched.reason,
            "a runtime's bindir cannot be watched; `mix path rescan` finds a tool installed into it"
        );
    }

    Some(watch)
}
