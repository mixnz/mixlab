use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use super::models::TerminalSize;

/// A session's handle. Local and SSH differ in who builds it, not in how it is used.
pub struct Session {
    /// The bytes the user types, flowing to the far end.
    pub input: UnboundedSender<Vec<u8>>,
    /// cols/rows each time the frame is resized.
    pub resize: UnboundedSender<TerminalSize>,
    /// The tab closing, or the app exiting.
    pub kill: CancellationToken,
}

impl Drop for Session {
    /// Dropping the handle kills the session: the child process is killed, and the writer and
    /// resize threads see their channels close and exit by themselves. So there is no way to leave
    /// a session behind.
    fn drop(&mut self) {
        self.kill.cancel();
    }
}

/// Every open session, by the id the frontend assigns. A plain lock rather than an async one:
/// nothing is awaited while holding it.
#[derive(Default)]
pub struct TerminalState {
    pub sessions: Mutex<HashMap<String, Session>>,
}

impl TerminalState {
    /// Removes a session from the map, if it is still there.
    ///
    /// It is let go *outside* the lock's scope: `Session`'s `Drop` cancels the token and lets go of
    /// both senders, and that is what wakes the threads and tasks still waiting on them — nothing
    /// in there needs the lock, and nothing in there should run while holding it.
    pub fn forget(&self, id: &str) {
        let gone = self.sessions.lock().unwrap().remove(id);
        drop(gone);
    }
}
