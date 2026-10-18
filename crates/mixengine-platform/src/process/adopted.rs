//! A process that outlived the daemon which started it, taken over by the one running now.

use crate::Result;
use crate::sys::process as sys;

use super::Exit;
use super::table::{StartTime, started_at};

/// A process that survived the daemon which started it, taken over by the one running now.
///
/// **The third kind of relationship in this module, and the weakest.** A [`Supervised`] child is
/// this process's own — pipes, a group, a status to wait for; a [`Detached`] one is a process this
/// one let go of but still has a handle to. This is neither: the process was started by a daemon
/// that is gone, its pipes went with that daemon, and this process is not its parent. What is left
/// is exactly two things, and they are what roadmap task **T18** promises and nothing more:
///
/// - **whether it is still there**, asked by re-reading its start time rather than by trusting its
///   pid, and
/// - **that it can be stopped**, addressed as the process group it leads on Unix and as one process
///   on Windows, where the job object it belonged to went with its daemon.
///
/// What is not available is stated where it costs something: its output is not captured, because the
/// pipes belong to a process that no longer exists, so an adopted service's `current.log` stops at
/// the moment the daemon died and resumes when the service is next started properly. Its exit
/// **status** is not available either — see [`exited`](Self::exited).
///
/// Dropping this does nothing, unlike [`Supervised`], and the asymmetry is deliberate: this value is
/// not the group's ownership, it is a way of addressing something that was already running. A daemon
/// that goes away for a second time leaves the survivor exactly as it found it, for the next one to
/// adopt.
///
/// [`Supervised`]: super::Supervised
/// [`Detached`]: super::Detached
#[derive(Debug)]
pub struct Adopted {
    /// The process, and on Unix the group it leads.
    pid: u32,

    /// What was recorded when it was started, and what every later question is asked against.
    started: StartTime,
}

impl Adopted {
    /// Take over the process `pid` if it is still the one that began at `started`.
    ///
    /// **The pair is the whole check.** A pid that has been handed out again names a process created
    /// later, which carries a different start time and is refused here — and refusing is what keeps
    /// the one accident this product cannot have out of reach: signalling somebody else's program
    /// because a number was reused.
    ///
    /// [`None`] means the process is gone, in every sense the caller needs: nothing has that pid,
    /// something else does, or what does has ended. None of them is a failure — a daemon that was
    /// killed usually took its services with it, and that is the ordinary outcome of this call.
    ///
    /// # Errors
    ///
    /// As [`started_at`]: only the case where the OS has the answer and refuses to give it.
    pub fn identify(pid: u32, started: StartTime) -> Result<Option<Self>> {
        Ok(started_at(pid)?
            .filter(|running| *running == started)
            .map(|started| Self { pid, started }))
    }

    /// The process this handle addresses.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Whether it has ended, and — as far as this process can tell — how.
    ///
    /// Never waits, like [`Supervised::exited`], and answers the same shape so that a supervisor
    /// watching an adopted service can be the same loop. What it does *not* share is where the
    /// answer comes from: there is no status to reap for a process this one did not start, so the
    /// question is asked by re-reading the identity — the process is there while its pid still
    /// carries the start time that was recorded, and gone the moment it does not.
    ///
    /// **The [`Exit`] it hands back therefore carries no code on any platform**, and says so when it
    /// is rendered. Windows would in fact give one, through a handle this could keep open; it is not
    /// used, because a restart policy that behaved differently on one system for a service that
    /// merely disappeared would be a difference nobody could act on. It is reported as
    /// *unsuccessful* for the same reason the code is absent: nothing here saw the process end, so a
    /// clean exit cannot be claimed, and the safer of the two readings for a restart policy that
    /// only puts back what *failed* is the one that puts the service back.
    ///
    /// # Errors
    ///
    /// As [`started_at`].
    ///
    /// [`Supervised::exited`]: super::Supervised::exited
    pub fn exited(&self) -> Result<Option<Exit>> {
        let still_there = self.still_the_one_recorded()?;

        Ok((!still_there).then(|| Exit {
            success: false,
            code: None,
            described: "gone (it outlived the daemon that started it, so no status was read)"
                .to_owned(),
        }))
    }

    /// Ask the whole group to stop and give it a chance to tidy up.
    ///
    /// `SIGTERM` to `-pgid` on Unix, exactly as [`Supervised::ask_to_stop`] sends it and resting on
    /// the same fact: the survivor called `setsid` before it became the program, so its pgid is its
    /// pid for as long as it lives.
    ///
    /// **Check [`CAN_ASK_TO_STOP`] first**, which is `false` on Windows for the reason it is false
    /// there for a child of our own — with even less of a console to reach this one through.
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedPlatform`] where the system has no way to ask, and [`Error::Os`] if it
    /// has one and refused — including when the identity could not be re-read, which is the one
    /// state this must not act through. A group that has already gone is not a failure.
    ///
    /// [`Supervised::ask_to_stop`]: super::Supervised::ask_to_stop
    /// [`CAN_ASK_TO_STOP`]: super::CAN_ASK_TO_STOP
    /// [`Error::UnsupportedPlatform`]: super::Error::UnsupportedPlatform
    /// [`Error::Os`]: super::Error::Os
    pub fn ask_to_stop(&self) -> Result<()> {
        if !self.still_the_one_recorded()? {
            return Ok(());
        }

        sys::ask_foreign_to_stop(self.pid)
    }

    /// Stop it, without a chance to tidy up.
    ///
    /// `SIGKILL` to `-pgid` on Unix; `TerminateProcess` on Windows, where the job object that would
    /// have taken the whole group went with the daemon that created it.
    ///
    /// **Returns as soon as the request is made, and cannot wait**: this process is not the
    /// survivor's parent, so there is no status to reap and nothing to block on. A caller that needs
    /// to know it has gone polls [`exited`](Self::exited), which is the same thing the identity check
    /// answers everywhere else.
    ///
    /// # Errors
    ///
    /// [`Error::Os`] if the OS refuses, or if the identity could not be re-read — which is deliberate
    /// and is the one case a stop must not push through. A process that had already gone is not a
    /// failure.
    ///
    /// [`Error::Os`]: super::Error::Os
    pub fn stop(&self) -> Result<()> {
        if !self.still_the_one_recorded()? {
            return Ok(());
        }

        sys::stop_foreign(self.pid)
    }

    /// Whether the pid this holds still carries the start time it was identified by.
    ///
    /// **Asked again immediately before every signal**, not only when this value was made. The
    /// alternative is a handle that was right when it was built and is acted on later — the window
    /// between the two being the whole of a boot's reconciliation, or the days a service runs for —
    /// and what fills that window is the process ending and its number being handed to somebody else.
    /// This narrows the race to the two instructions between the check and the `kill`, which is the
    /// same residual `docs/decisions/0007-supervised-child-owns-a-process-group.md` already
    /// accepts for a `Supervised`.
    ///
    /// It is also what makes Unix's fallback to signalling the bare pid defensible: a group id could
    /// only ever have been ours, where a pid is only ours because this said so a moment ago.
    fn still_the_one_recorded(&self) -> Result<bool> {
        Ok(started_at(self.pid)?.is_some_and(|running| running == self.started))
    }
}
