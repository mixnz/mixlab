//! The job object a supervised process and everything it starts run in, and what it caps, lists and
//! ends.

use std::io;
use std::os::windows::io::{AsRawHandle as _, OwnedHandle};
use std::time::Instant;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
    JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PRIORITY_CLASS,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    WaitForSingleObject,
};

use crate::{Error, Result};

use super::{GROUP_GONE, RawChild, still_running, unsafe_zeroed_because_every_field_is_a_number};

/// A Job Object holding one supervised service and everything it starts.
///
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the whole mechanism: when the last handle to the job
/// closes, the kernel terminates every process in it. A daemon that *exits* closes this handle, and
/// so does a daemon that is killed — there is no cleanup code that has to run for the guarantee to
/// hold, which is why Windows is the strongest of the three platforms in
/// `docs/decisions/0007-supervised-child-owns-a-process-group.md`.
///
/// One job per service rather than one for the daemon, so `TerminateJobObject` means "stop this
/// service and its children" and not "stop everything". It is also the object Phase 7 hangs CPU and
/// memory caps on (roadmap task T68), which per-daemon jobs would have had to be rebuilt to allow.
#[derive(Debug)]
pub(crate) struct Group {
    /// Owned. Closing it is what kills the group, so it is closed in exactly one place — [`Drop`].
    pub(super) job: HANDLE,
}

/// A kernel handle is process-wide and not tied to the thread that opened it, so a `Group` may be
/// moved and shared like the pgid its Unix counterpart holds. Without this the supervisor could not
/// keep a `Supervised` across an `await`, and the two platforms would have different API shapes for
/// no reason the caller could act on.
#[expect(
    unsafe_code,
    reason = "a job object handle is valid from any thread of this process and this type owns it, \
              so nothing here can be closed twice or used after closing"
)]
unsafe impl Send for Group {}

#[expect(
    unsafe_code,
    reason = "every call this type makes on the handle is atomic in the kernel and none of them \
              mutate the Rust value, so a shared reference from two threads is sound"
)]
unsafe impl Sync for Group {}

/// Create the job a supervised child will be put into, before there is a child to put into it.
///
/// Anonymous and with default security: nothing else has any business finding it by name, and a job
/// nobody can open by name cannot be joined by a process we did not start.
pub(crate) fn group() -> Result<Group> {
    #[expect(
        unsafe_code,
        reason = "CreateJobObjectW is given two null pointers, which is how it is asked for an \
                  anonymous job with default security; it borrows nothing"
    )]
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };

    if job.is_null() {
        return Err(Error::Os {
            action: "create a job object for a supervised process",
            source: io::Error::last_os_error(),
        });
    }

    // Owned from here on, so the failure below closes it rather than leaking it.
    let group = Group { job };
    // `KILL_ON_JOB_CLOSE`, and no cap yet: the ceilings a service asks for are written by this same
    // function a moment later, from `spawn_supervised`. **One writer of that struct** — see
    // [`Group::set_extended_limits`] for why there may not be two.
    group.set_extended_limits(None, crate::process::Priority::Normal)?;

    Ok(group)
}

impl Group {
    /// There is no signal to send; see [`CAN_SIGNAL`].
    ///
    /// Reached only by a caller that ignored that constant, so it says what it is rather than
    /// pretending to have sent one — a silent success here would be a configuration a user believes
    /// is live and is not.
    ///
    /// [`CAN_SIGNAL`]: super::CAN_SIGNAL
    pub(crate) fn signal_leader(&self, _pid: u32, _which: crate::process::Signal) -> Result<()> {
        Err(Error::UnsupportedPlatform {
            capability: "signalling a supervised process",
            reason: "Windows has no signal a daemon can send a process it did not give a console \
                     to; a service that needs to re-read its configuration on this system is \
                     restarted instead"
                .to_owned(),
        })
    }

    /// There is no way to ask; see [`CAN_ASK_TO_STOP`].
    ///
    /// Reached only by a caller that ignored that constant, so it says what it is rather than
    /// pretending to have asked — a silent success here would be a grace period spent waiting for a
    /// message nobody sent.
    ///
    /// [`CAN_ASK_TO_STOP`]: super::CAN_ASK_TO_STOP
    pub(crate) fn request_stop(&self, _pid: u32) -> Result<()> {
        Err(Error::UnsupportedPlatform {
            capability: "asking a supervised process group to stop",
            reason:
                "Windows has no signal a daemon can send to a process it did not give a console \
                     to; a service that needs to shut down cleanly is stopped with its own command \
                     instead"
                    .to_owned(),
        })
    }

    /// Write this service's ceilings into the job object it is already in.
    ///
    /// **No second object.** The job [`group`] created carries `KILL_ON_JOB_CLOSE` and, once
    /// [`adopt`](Self::adopt) has run, everything the service forks — so a limit set on it is a limit
    /// on all of that, which is what a limit on a *service* has to mean. A php-fpm master under a cap
    /// whose workers were outside it would be a cap in name only.
    ///
    /// Called before the child exists by [`crate::process::spawn_supervised`], and again for every
    /// later change by [`crate::process::Supervised::set_limits`] — the same code both times, so a
    /// limit applied at start and one applied to a running service cannot drift apart.
    pub(crate) fn set_limits(&self, limits: &crate::process::Limits) -> Result<()> {
        self.set_cpu(limits.cpu_percent)?;
        self.set_extended_limits(limits.memory_mb, limits.priority)
    }

    /// The one place `cpu_percent`'s unit is converted, and the conversion is the whole method.
    ///
    /// `cpu_percent` is a percentage of **one core**; `CpuRate` is hundredths of a percent of **all
    /// cores together**. So 50% of one core on an eight-core machine is `CpuRate = 625`. Linux needs
    /// no counterpart, because `cpu.max`'s `$MAX $PERIOD` pair is per-core by construction.
    ///
    /// **[`None`] is written as "the whole machine", not as an all-zero struct.**
    ///
    /// Removing a limit has to be a *write*, because this is also how a cap comes off a running
    /// service and a silent return would leave the previous ceiling in place. But this kernel refuses
    /// any `JobObjectCpuRateControlInformation` whose `ControlFlags` is `0` — measured, not assumed:
    /// all-zero and flags-zero-with-a-valid-rate both come back `ERROR_INVALID_PARAMETER`, and only
    /// a write with `ENABLE` set is accepted. So there is no way through this API to put a job back
    /// to having no rate control at all once it has had some.
    ///
    /// What is written instead is the nearest true statement: `ENABLE` **without** `HARD_CAP`, at
    /// `10_000` — a hundred per cent of the whole machine, which is not a ceiling a process can
    /// reach. A reader of the job object sees rate control switched on, and that is honest: it is on,
    /// and it is not limiting anything.
    ///
    /// The rate is floored at 1 rather than allowed to round to zero, for the same reason: a `CpuRate`
    /// of 0 with the control enabled is refused, and somebody asking for 1% of one core on a
    /// 128-core machine has asked for the smallest cap this system can express.
    fn set_cpu(&self, cpu_percent: Option<u8>) -> Result<()> {
        /// A hundred per cent of the whole machine, in the hundredths of a percent `CpuRate` counts
        /// in. The largest value the field accepts, and therefore the way "no ceiling" is spelled.
        const WHOLE_MACHINE: u32 = 10_000;

        let mut control: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION =
            unsafe_zeroed_because_every_field_is_a_number();

        match cpu_percent {
            Some(percent) => {
                control.ControlFlags =
                    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                control.Anonymous.CpuRate =
                    (u32::from(percent) * 100 / super::super::limits::cores().max(1)).max(1);
            }

            None => {
                control.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE;
                control.Anonymous.CpuRate = WHOLE_MACHINE;
            }
        }

        #[expect(
            unsafe_code,
            reason = "the pointer is to a local this frame owns and the length is that local's own \
                      size, so the kernel reads exactly the struct that is there"
        )]
        let set = unsafe {
            SetInformationJobObject(
                self.job,
                JobObjectCpuRateControlInformation,
                std::ptr::from_ref(&control).cast(),
                size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            )
        };

        if set == 0 {
            return Err(Error::Os {
                action: "cap a supervised service's CPU",
                source: io::Error::last_os_error(),
            });
        }

        Ok(())
    }

    /// **The only writer of `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`, and that is the whole point.**
    ///
    /// `LimitFlags` is one field describing every extended limit at once, so a second writer setting
    /// only *its* flag would silently clear `KILL_ON_JOB_CLOSE` — and the guarantee that a killed
    /// daemon takes its services with it (ADR 0007) would go away with no error and no test failing.
    /// One function composes the whole word, and `KILL_ON_JOB_CLOSE` is unconditional in it.
    ///
    /// Called by [`group`] before there is anything to cap, with `None` and
    /// [`Priority::Normal`](crate::process::Priority::Normal), and by
    /// [`set_limits`](Self::set_limits) afterwards.
    fn set_extended_limits(
        &self,
        memory_mb: Option<u32>,
        priority: crate::process::Priority,
    ) -> Result<()> {
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            unsafe_zeroed_because_every_field_is_a_number();

        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        if let Some(mb) = memory_mb {
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            limits.JobMemoryLimit = usize::try_from(mb)
                .unwrap_or(usize::MAX)
                .saturating_mul(1024 * 1024);
        }

        if matches!(priority, crate::process::Priority::Background) {
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PRIORITY_CLASS;
            limits.BasicLimitInformation.PriorityClass = BELOW_NORMAL_PRIORITY_CLASS;
        }

        #[expect(
            unsafe_code,
            reason = "the pointer is to a local this frame owns and the length is that local's own \
                      size, so the kernel reads exactly the struct that is there"
        )]
        let set = unsafe {
            SetInformationJobObject(
                self.job,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };

        if set == 0 {
            return Err(Error::Os {
                action: "cap a supervised service's memory",
                source: io::Error::last_os_error(),
            });
        }

        Ok(())
    }

    /// Put the child into the job, now that it exists.
    ///
    /// **After the spawn, and that is the one weak point in the Windows story.** Assigning before
    /// the child runs means `CREATE_SUSPENDED` plus a `ResumeThread` on the child's initial thread,
    /// and `std::process::Child` hands out a process handle but never a thread one — so doing it in
    /// the right order means replacing `Command` with a hand-rolled `CreateProcessW`. A daemon
    /// killed between those two calls therefore leaves the child behind; the window is one call
    /// wide, and crash recovery (roadmap task T18) covers exactly that case. ADR 0007 records the
    /// trade.
    ///
    /// A child that has already exited cannot be assigned, and Windows says so with
    /// `ERROR_ACCESS_DENIED` — indistinguishable from the real thing here, so it is reported as the
    /// failure it is rather than guessed at.
    pub(crate) fn adopt(&self, child: &RawChild) -> Result<()> {
        self.assign(child.raw_handle())
    }

    /// The same assignment, addressed by handle.
    ///
    /// A shim's hand-over used to be the other caller; since T185 it keeps a job of its own in
    /// `windows/handover.rs`, so that the `handover` feature needs nothing from `process`. The
    /// caller borrows a handle it owns, which is the whole of what this needs to be true.
    fn assign(&self, process: HANDLE) -> Result<()> {
        #[expect(
            unsafe_code,
            reason = "both handles are owned elsewhere and outlive the call — the job by this \
                      value, the process by the caller — and the call closes neither"
        )]
        let assigned = unsafe { AssignProcessToJobObject(self.job, process) };

        if assigned == 0 {
            return Err(Error::Os {
                action: "put a supervised process into its job object",
                source: io::Error::last_os_error(),
            });
        }

        Ok(())
    }

    /// Kill the job: every process in it, at once, without a chance to tidy up.
    ///
    /// The pid is what the Unix counterpart addresses and is unused here — the job knows its own
    /// members, including the ones the service started after we stopped looking.
    ///
    /// Exit code 1, so a service killed this way is not mistaken for one that stopped successfully
    /// by whatever reads the status afterwards.
    ///
    /// **Returns once every member has ended, not once the kill was accepted.** `TerminateJobObject`
    /// only starts each member on its way out, and a member's open files are released when it has
    /// gone. CI run 37679651605 is what that costs: `mysqld.exe` runs the real server as a child of a
    /// monitor, the stop waited for the monitor alone, and the credential reset started 60 ms later
    /// met the child still holding `ibdata1`. So the members are opened **before** the kill — a
    /// process that has left the job's list may not have finished leaving, and could no longer be
    /// found to wait for — and each is waited for afterwards. A member that appeared in the meantime
    /// is caught by asking again, until the job is empty or [`GROUP_GONE`] has passed.
    ///
    /// # Errors
    ///
    /// [`Error::Os`] if the kill is refused, or a member is still running when [`GROUP_GONE`] has
    /// passed.
    pub(crate) fn terminate(&self, _pid: u32) -> Result<()> {
        let deadline = Instant::now() + GROUP_GONE;

        loop {
            let members = self.members();

            #[expect(
                unsafe_code,
                reason = "the handle is owned by this value and is not closed by the call; the \
                          exit code is passed by value"
            )]
            let terminated = unsafe { TerminateJobObject(self.job, 1) };

            if terminated == 0 {
                return Err(Error::Os {
                    action: "stop a supervised process group",
                    source: io::Error::last_os_error(),
                });
            }

            if members.is_empty() {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(still_running());
            }

            for member in &members {
                let left = deadline.saturating_duration_since(Instant::now());
                let patience = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);

                #[expect(
                    unsafe_code,
                    reason = "the handle is owned by `members` and the call does not close it"
                )]
                let waited =
                    unsafe { WaitForSingleObject(member.as_raw_handle().cast(), patience) };

                if waited != WAIT_OBJECT_0 {
                    return Err(still_running());
                }
            }
        }
    }

    /// Every process in the job now, each opened so that it can be waited for.
    ///
    /// **Checked to be a member after it is opened**, because a pid read from the list may belong to
    /// somebody else by the time it is opened if its process ended in between — and opened with the
    /// query right as well as `SYNCHRONIZE`, because `IsProcessInJob` refuses a handle without it. A member that cannot
    /// be opened has ended, which is what it would be waited for to do.
    fn members(&self) -> Vec<OwnedHandle> {
        use std::os::windows::io::FromRawHandle as _;
        use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
        use windows_sys::Win32::System::JobObjects::{
            IsProcessInJob, JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList,
            QueryInformationJobObject,
        };
        use windows_sys::Win32::System::Threading::PROCESS_SYNCHRONIZE;

        /// Where the list of ids begins, in `usize`s — the buffer below is made of them, which is
        /// what keeps the header read through it aligned.
        const HEADER: usize = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
            / size_of::<usize>();

        let mut room = 16usize;

        let pids: Vec<u32> = loop {
            let mut buffer = vec![0usize; HEADER + room];

            #[expect(
                unsafe_code,
                reason = "the pointer and the length describe `buffer`, which this frame owns and \
                          which is aligned for the struct the kernel writes into it"
            )]
            let queried = unsafe {
                QueryInformationJobObject(
                    self.job,
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    u32::try_from(buffer.len() * size_of::<usize>()).unwrap_or(u32::MAX),
                    std::ptr::null_mut(),
                )
            };

            #[expect(
                unsafe_code,
                reason = "the buffer starts with the struct's header, written by the kernel or \
                          still zero"
            )]
            let header = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let assigned = header.NumberOfAssignedProcesses as usize;

            if queried == 0 {
                let error = io::Error::last_os_error();

                // A job grown since the last ask. Anything else is nothing to wait for: the kill
                // still happens, and the leader is still waited for by the caller.
                if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) && assigned > room {
                    room = assigned;
                    continue;
                }

                break Vec::new();
            }

            let listed = (header.NumberOfProcessIdsInList as usize).min(room);

            break buffer[HEADER..HEADER + listed]
                .iter()
                .filter_map(|&pid| u32::try_from(pid).ok())
                .collect();
        };

        pids.into_iter()
            .filter_map(|pid| {
                #[expect(
                    unsafe_code,
                    reason = "OpenProcess takes three integers; the handle it returns is owned at \
                              once"
                )]
                let process = unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        pid,
                    )
                };

                if process.is_null() {
                    return None;
                }

                #[expect(
                    unsafe_code,
                    reason = "the handle was just returned by OpenProcess and nothing else owns it"
                )]
                let process = unsafe { OwnedHandle::from_raw_handle(process.cast()) };

                let mut member = 0;

                #[expect(
                    unsafe_code,
                    reason = "both handles are owned and outlive the call; the answer is written \
                              into a local"
                )]
                let asked = unsafe {
                    IsProcessInJob(process.as_raw_handle().cast(), self.job, &raw mut member)
                };

                (asked != 0 && member != 0).then_some(process)
            })
            .collect()
    }
}

impl Drop for Group {
    /// Close the handle, which is what kills the group.
    ///
    /// Failure is unreportable and uninteresting: a handle this value owns and never hands out
    /// cannot already be closed, and nothing useful is left to do if the kernel disagrees.
    fn drop(&mut self) {
        #[expect(
            unsafe_code,
            reason = "the handle was created by this type, has not been closed before, and is not \
                      used again after this"
        )]
        unsafe {
            CloseHandle(self.job);
        }
    }
}
