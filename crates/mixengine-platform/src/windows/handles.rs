//! Who holds a file or a directory, read from the system's handle table — roadmap task **T182e**.
//!
//! **The one place Windows says who holds a directory.** The Restart Manager answers for files; a
//! directory an editor watches, or a terminal stands in, refuses the rename of every directory above
//! it just the same and is in no list but this one (the T182e design, D2). It is what Process
//! Explorer and `handle.exe` read.
//!
//! **Naming a handle can hang.** A synchronous pipe with a read outstanding makes any query on it
//! wait for that read, and a pipe is a `File` object like any other. So only `File` objects are
//! looked at, the naming runs on a worker thread, a worker that does not answer within [`STUCK`] is
//! abandoned and a fresh one carries on from the next handle, and the whole walk has a budget. What
//! was found by then is the answer.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStringExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
use windows_sys::Win32::Foundation::{CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_TYPE_DISK, GetFileType, GetFinalPathNameByHandleW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE};

/// `SystemExtendedHandleInformation`: every handle on the system, with 64-bit-safe fields.
const SYSTEM_EXTENDED_HANDLE_INFORMATION: i32 = 64;

/// `STATUS_INFO_LENGTH_MISMATCH`: the buffer was too small, and the table has to be read again.
const STATUS_INFO_LENGTH_MISMATCH: i32 = i32::from_ne_bytes(0xC000_0004_u32.to_ne_bytes());

/// How long one naming call may take before its worker is given up on.
const STUCK: Duration = Duration::from_millis(250);

/// How many workers may be given up on before the walk stops — each one is a thread left waiting.
const ABANDONED_AT_MOST: usize = 16;

/// The largest table this reads, so a runaway count cannot ask for all of memory.
const TABLE_AT_MOST: usize = 1 << 30;

/// One row of the table: `SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX`, which `windows-sys` does not declare.
#[repr(C)]
struct Entry {
    object: *mut c_void,
    pid: usize,
    handle: usize,
    access: u32,
    back_trace: u16,
    type_index: u16,
    attributes: u32,
    reserved: u32,
}

/// A handle to something on a disk, and the process holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenHandle {
    /// The process holding it.
    pub(crate) pid: u32,

    /// What it is open on, as `GetFinalPathNameByHandleW` spells it: a `\\?\` verbatim path.
    pub(crate) path: PathBuf,
}

/// Every handle to a file or a directory on a disk that this process may look at, within `budget`.
///
/// A process this one may not duplicate from — another user's, SYSTEM's — is skipped: what it holds
/// is met by the rename's retry instead (the T182e design, "What this does not do").
pub(crate) fn open_on_disk(budget: Duration) -> Vec<OpenHandle> {
    if budget.is_zero() {
        return Vec::new();
    }
    let deadline = Instant::now() + budget;

    // Held open across the read, so the table holds a handle known to be a file and the `File`
    // type's index is learned from it rather than assumed.
    let Ok(marker) = std::env::current_exe().and_then(std::fs::File::open) else {
        return Vec::new();
    };
    let marker_handle = marker.as_raw_handle() as usize;

    let Some(entries) = read_table() else {
        return Vec::new();
    };

    let me = std::process::id() as usize;
    let Some(file_type) = entries
        .iter()
        .find(|(pid, handle, _)| *pid == me && *handle == marker_handle)
        .map(|(_, _, type_index)| *type_index)
    else {
        return Vec::new();
    };
    drop(marker);

    let candidates: Arc<Vec<(u32, usize)>> = Arc::new(
        entries
            .into_iter()
            .filter(|(_, _, type_index)| *type_index == file_type)
            .filter_map(|(pid, handle, _)| Some((u32::try_from(pid).ok()?, handle)))
            .collect(),
    );

    walk(&candidates, deadline)
}

/// What a worker says as it goes.
enum Said {
    /// About to name the candidate at this index.
    At(usize),
    /// One handle, named.
    Found(OpenHandle),
}

/// Name every candidate, on workers that are abandoned when stuck, until done or `deadline`.
fn walk(candidates: &Arc<Vec<(u32, usize)>>, deadline: Instant) -> Vec<OpenHandle> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut abandoned = 0;

    while start < candidates.len() && abandoned <= ABANDONED_AT_MOST {
        let (tell, hear) = mpsc::channel();
        let theirs = Arc::clone(candidates);
        let spawned = std::thread::Builder::new()
            .name("handle-names".to_owned())
            .spawn(move || name_from(&theirs, start, &tell));
        if spawned.is_err() {
            break;
        }

        let mut at = start;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return found;
            }

            match hear.recv_timeout(STUCK.min(left)) {
                Ok(Said::At(index)) => at = index,
                Ok(Said::Found(open)) => found.push(open),
                // Finished: the worker dropped its end.
                Err(mpsc::RecvTimeoutError::Disconnected) => return found,
                // Stuck on `at`: leave it waiting, and carry on from the next one.
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    abandoned += 1;
                    start = at + 1;
                    break;
                }
            }
        }
    }

    found
}

/// The worker: name candidates from `start` on, telling `tell` as it goes.
fn name_from(candidates: &[(u32, usize)], start: usize, tell: &mpsc::Sender<Said>) {
    let mut processes: HashMap<u32, usize> = HashMap::new();

    for (index, &(pid, handle)) in candidates.iter().enumerate().skip(start) {
        if tell.send(Said::At(index)).is_err() {
            break;
        }

        let process = *processes.entry(pid).or_insert_with(|| open_process(pid));
        if process == 0 {
            continue;
        }

        if let Some(path) = name(process, handle)
            && tell.send(Said::Found(OpenHandle { pid, path })).is_err()
        {
            break;
        }
    }

    for process in processes.into_values().filter(|&process| process != 0) {
        #[expect(unsafe_code, reason = "a process handle opened above, closed once")]
        unsafe {
            CloseHandle(process as HANDLE);
        }
    }
}

/// `pid` opened for duplicating its handles, as an integer so it may cross threads; 0 when refused.
fn open_process(pid: u32) -> usize {
    #[expect(
        unsafe_code,
        reason = "no pointers; a null result is checked by the caller"
    )]
    let process = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, pid) };
    process as usize
}

/// What `handle`, in `process`, is open on — when it is on a disk.
fn name(process: usize, handle: usize) -> Option<PathBuf> {
    let mut ours: HANDLE = std::ptr::null_mut();

    #[expect(
        unsafe_code,
        reason = "`process` was opened with PROCESS_DUP_HANDLE, `handle` is a value from its table, \
                  and `ours` is a local the call writes"
    )]
    let duplicated = unsafe {
        DuplicateHandle(
            process as HANDLE,
            handle as HANDLE,
            GetCurrentProcess(),
            &raw mut ours,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if duplicated == 0 {
        return None;
    }

    let named = (|| {
        #[expect(unsafe_code, reason = "a handle this process owns")]
        if unsafe { GetFileType(ours) } != FILE_TYPE_DISK {
            return None;
        }

        let mut buffer = vec![0u16; 1024];
        loop {
            let room = u32::try_from(buffer.len()).unwrap_or(u32::MAX);

            #[expect(
                unsafe_code,
                reason = "`buffer` has exactly `room` elements and outlives the call"
            )]
            let written = unsafe { GetFinalPathNameByHandleW(ours, buffer.as_mut_ptr(), room, 0) };
            let written = usize::try_from(written).ok()?;

            if written == 0 {
                return None;
            }
            if written < buffer.len() {
                return Some(PathBuf::from(std::ffi::OsString::from_wide(
                    &buffer[..written],
                )));
            }
            // Too small: `written` is the length needed, terminator included.
            buffer = vec![0u16; written + 1];
        }
    })();

    #[expect(unsafe_code, reason = "the duplicate made above, closed once")]
    unsafe {
        CloseHandle(ours);
    }

    named
}

/// The whole table, as `(pid, handle value, type index)`.
fn read_table() -> Option<Vec<(usize, usize, u16)>> {
    let header = 2 * std::mem::size_of::<usize>();
    let mut size: usize = 4 << 20;

    loop {
        // `u64`s so the buffer is aligned for the `usize` fields read out of it.
        let mut buffer = vec![0u64; size / 8];
        let mut needed = 0u32;

        #[expect(
            unsafe_code,
            reason = "the buffer is `size` bytes, writable, and outlives the call"
        )]
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buffer.as_mut_ptr().cast(),
                u32::try_from(size).ok()?,
                &raw mut needed,
            )
        };

        if status == STATUS_INFO_LENGTH_MISMATCH {
            size = size.checked_mul(2)?;
            if size > TABLE_AT_MOST {
                return None;
            }
            continue;
        }
        if status < 0 {
            return None;
        }

        let bytes = buffer.as_ptr().cast::<u8>();

        #[expect(
            unsafe_code,
            reason = "the first field of the header, inside the aligned buffer"
        )]
        let count = unsafe { bytes.cast::<usize>().read() };
        let count = count.min((size - header) / std::mem::size_of::<Entry>());

        let entries = (0..count)
            .map(|index| {
                #[expect(
                    unsafe_code,
                    reason = "`index` is below the count the call wrote and the room the buffer has"
                )]
                let entry = unsafe {
                    bytes
                        .add(header + index * std::mem::size_of::<Entry>())
                        .cast::<Entry>()
                        .read_unaligned()
                };
                (entry.pid, entry.handle, entry.type_index)
            })
            .collect();

        return Some(entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt as _;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn a_table_entry_is_forty_bytes() {
        assert_eq!(std::mem::size_of::<Entry>(), 40);
    }

    fn held_by_this_process(found: &[OpenHandle], name: &str) -> bool {
        found.iter().any(|open| {
            open.pid == std::process::id()
                && open
                    .path
                    .file_name()
                    .is_some_and(|file| file.eq_ignore_ascii_case(name))
        })
    }

    #[test]
    fn a_file_this_process_holds_is_found() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let file = root.path().join("t182e-held.log");
        std::fs::write(&file, b"held").expect("a file");
        let _open = std::fs::File::open(&file).expect("held open");

        let found = open_on_disk(Duration::from_secs(5));

        assert!(
            held_by_this_process(&found, "t182e-held.log"),
            "{} handles",
            found.len()
        );
    }

    #[test]
    fn a_directory_this_process_holds_is_found() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let directory = root.path().join("t182e-watched");
        std::fs::create_dir(&directory).expect("a directory");
        let _open = std::fs::OpenOptions::new()
            .access_mode(0x0010_0081)
            .share_mode(0x7)
            .custom_flags(0x0200_0000)
            .open(&directory)
            .expect("the directory held open");

        let found = open_on_disk(Duration::from_secs(5));

        assert!(
            held_by_this_process(&found, "t182e-watched"),
            "{} handles",
            found.len()
        );
    }

    /// A scan with no time left answers at once instead of hanging an uninstall.
    #[test]
    fn a_zero_budget_returns_at_once() {
        let began = Instant::now();
        let _ = open_on_disk(Duration::ZERO);
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
    }
}
