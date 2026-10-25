//! Starting a child under a token: its pipes, its command line, its environment block and the
//! attribute list that hands it only the handles it needs.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, UpdateProcThreadAttribute,
};

use super::super::sid::Token;
use super::{Spawned, token};
use crate::{Error, Result};

/// Start `program` from a restricted token, with both output streams piped.
///
/// Standard input is the null device unless `input` is `Some`, in which case the caller is handed
/// the write end and is the one that has to close it — an end of file is what tells a program its
/// instruction is complete.
///
/// `env` arrives already composed by
/// [`whole_environment`](crate::process::whole_environment): this builds a block out of it and does
/// not add to it.
///
/// # Errors
///
/// [`Error::Os`] when the token, the pipes or the attribute list cannot be made, and [`Error::Io`]
/// naming the program when it cannot be started at all.
pub(crate) fn spawn(
    program: &Path,
    args: &[OsString],
    directory: &Path,
    env: &BTreeMap<String, String>,
    input: Option<()>,
) -> Result<Spawned> {
    spawn_from(
        &token()?,
        program,
        &Arguments::Quoted(args),
        directory,
        env,
        input,
    )
}

/// [`spawn`], with everything after the program handed over verbatim.
///
/// **For `cmd.exe /C <command>` and nothing else.**
pub(crate) fn spawn_raw(
    program: &Path,
    tail: &OsStr,
    directory: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Spawned> {
    spawn_from(
        &token()?,
        program,
        &Arguments::Raw(tail),
        directory,
        env,
        None,
    )
}

/// [`spawn`], from a token the caller already has.
///
/// The parameter exists for one experiment and is not a way to start a child unrestricted: the only
/// other caller is the test that runs this exact path from the *unrestricted* token, which is what
/// separates "this machine will not perform this spawn" from "this machine will not grant this
/// token what the spawn needs".
pub(super) fn spawn_from(
    token: &Token,
    program: &Path,
    args: &Arguments<'_>,
    directory: &Path,
    env: &BTreeMap<String, String>,
    input: Option<()>,
) -> Result<Spawned> {
    let (out_read, out_write) = pipe()?;
    let (err_read, err_write) = pipe()?;
    let (in_read, in_write) = match input {
        None => (null_device()?, None),
        Some(()) => {
            let (read, write) = pipe()?;

            (read, Some(write))
        }
    };

    // Only these three may cross into the child, and they are named rather than left to
    // `bInheritHandles` — which is process-wide for the length of a spawn and is exactly the window
    // `hide_stdio_from_children` exists to guard on the paths that still use `Command`.
    let inheritable: [HANDLE; 3] = [
        in_read.as_raw_handle().cast(),
        out_write.as_raw_handle().cast(),
        err_write.as_raw_handle().cast(),
    ];

    for handle in inheritable {
        #[expect(
            unsafe_code,
            reason = "each handle is owned by a local in this frame and outlives the call, which \
                      sets a flag and closes nothing"
        )]
        let marked =
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };

        if marked == 0 {
            return Err(Error::Os {
                action: "let a child inherit the pipe it writes to",
                source: io::Error::last_os_error(),
            });
        }
    }

    let mut attributes = AttributeList::naming(&inheritable)?;

    let mut started = STARTUPINFOEXW::default();
    started.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(u32::MAX);
    started.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    started.StartupInfo.hStdInput = inheritable[0];
    started.StartupInfo.hStdOutput = inheritable[1];
    started.StartupInfo.hStdError = inheritable[2];
    started.lpAttributeList = attributes.list;

    let mut line = command_line(program, args);
    let mut block = environment_block(env);
    let directory = wide(directory.as_os_str());
    let mut running = PROCESS_INFORMATION::default();

    #[expect(
        unsafe_code,
        reason = "every buffer passed is a local that outlives the call; the command line is \
                  mutable because CreateProcess may write into it; the handles named in the \
                  attribute list are all owned by this frame"
    )]
    let made = unsafe {
        CreateProcessAsUserW(
            token.0,
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            block.as_mut_ptr().cast(),
            directory.as_ptr(),
            (&raw const started).cast(),
            &raw mut running,
        )
    };

    if made == 0 {
        return Err(Error::Io {
            action: "start",
            path: program.to_path_buf(),
            source: io::Error::last_os_error(),
        });
    }

    attributes.release();

    // The child holds its own copies now. Ours are dropped by leaving this scope; the thread handle
    // is closed here because nothing ever waits on a thread.
    #[expect(
        unsafe_code,
        reason = "the thread handle was created by the call above, is used nowhere else, and is \
                  closed exactly once"
    )]
    unsafe {
        CloseHandle(running.hThread);
    }

    drop(in_read);
    drop(out_write);
    drop(err_write);

    #[expect(
        unsafe_code,
        reason = "the process handle was created by the call above and is handed to an OwnedHandle \
                  that closes it exactly once"
    )]
    let process = unsafe { OwnedHandle::from_raw_handle(running.hProcess.cast()) };

    Ok(Spawned {
        process,
        pid: running.dwProcessId,
        stdout: File::from(out_read),
        stderr: File::from(err_read),
        stdin: in_write.map(File::from),
    })
}

/// An anonymous pipe, as `(read, write)`.
///
/// Neither end is inheritable here. The one that crosses is marked afterwards, by name, which is
/// what keeps a spawn from handing a child the end this process is reading.
fn pipe() -> Result<(OwnedHandle, OwnedHandle)> {
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();

    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 0,
    };

    #[expect(
        unsafe_code,
        reason = "the attributes are a local that outlives the call, and both handles are written \
                  into locals this function owns and hands straight to a guard"
    )]
    let made = unsafe { CreatePipe(&raw mut read, &raw mut write, &raw const attributes, 0) };

    if made == 0 {
        return Err(Error::Os {
            action: "make a pipe for a supervised child",
            source: io::Error::last_os_error(),
        });
    }

    #[expect(
        unsafe_code,
        reason = "both handles were created by the call above and are each handed to one \
                  OwnedHandle, which closes them exactly once"
    )]
    let ends = unsafe {
        (
            OwnedHandle::from_raw_handle(read.cast()),
            OwnedHandle::from_raw_handle(write.cast()),
        )
    };

    Ok(ends)
}

/// `NUL`, for the child that was given nothing to read.
///
/// A real handle rather than a null one: a child whose `hStdInput` is null inherits *this process's*
/// standard input, which for a daemon is a console it must never touch and for a test is the
/// harness's own pipe.
fn null_device() -> Result<OwnedHandle> {
    File::open("NUL")
        .map(OwnedHandle::from)
        .map_err(|source| Error::Io {
            action: "open",
            path: std::path::PathBuf::from("NUL"),
            source,
        })
}

/// What follows the program on the command line.
///
/// **Two shapes, because two readers.** Everything this crate starts is a program that parses its
/// line the way `CommandLineToArgvW` does, and [`Arguments::Quoted`] is that rule. `cmd.exe` is not
/// one of those programs: its own parser does not honour a backslash-escaped quote, so a command
/// with a quote in it would arrive mangled — [`Arguments::Raw`] hands the tail over byte for byte
/// instead, which is what a blueprint's `[scaffold]` needs.
pub(crate) enum Arguments<'a> {
    /// A list, quoted only where it has to be.
    Quoted(&'a [OsString]),

    /// One string, appended exactly as it is.
    Raw(&'a OsStr),
}

/// The command line `CreateProcess` is handed.
///
/// `CreateProcess` takes one string where `Command` takes a list, so the quoting the standard
/// library does is this function's to do — see [`append`], which is where the rule is.
fn command_line(program: &Path, args: &Arguments<'_>) -> Vec<u16> {
    let mut line = Vec::new();

    append(program.as_os_str(), &mut line);

    match args {
        Arguments::Quoted(args) => {
            for arg in *args {
                line.push(u16::from(b' '));
                append(arg, &mut line);
            }
        }

        // Not quoted, and not escaped: the caller has already written the line the way its reader
        // parses it, which for `cmd.exe /C` is the only way that survives.
        Arguments::Raw(tail) => {
            line.push(u16::from(b' '));
            line.extend(tail.encode_wide());
        }
    }

    line.push(0);
    line
}

/// One argument of [`command_line`], quoted only where it has to be.
///
/// **Only where it has to be, and that is not a stylistic choice.** `cmd.exe /c` removes the outer
/// quotes around what follows only when the whole line carries exactly two of them; a spawn that
/// quoted the program path as well would leave four, and `cmd /c "echo hello"` would then look for
/// a program called `"echo hello`. The standard library quotes on the same condition for the same
/// reason, and a probe that spelled its line differently from the `Command` beside it would be a
/// difference nobody could see until a service failed to start.
///
/// The quoting rule inside is `CommandLineToArgvW`'s: a backslash is an escape only immediately
/// before a quote, so the run of backslashes before one is doubled, and so is the run before the
/// closing quote this adds.
fn append(argument: &OsStr, line: &mut Vec<u16>) {
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    const SPACE: u16 = b' ' as u16;
    const TAB: u16 = b'\t' as u16;

    let units: Vec<u16> = argument.encode_wide().collect();
    let needs_quoting = units.is_empty()
        || units
            .iter()
            .any(|unit| matches!(*unit, SPACE | TAB | QUOTE));

    if !needs_quoting {
        line.extend(units);

        return;
    }

    line.push(QUOTE);

    let mut backslashes = 0;

    for unit in units {
        match unit {
            BACKSLASH => backslashes += 1,
            QUOTE => {
                // The run before a quote is doubled, and the quote itself escaped.
                for _ in 0..=backslashes {
                    line.push(BACKSLASH);
                }

                backslashes = 0;
            }
            _ => backslashes = 0,
        }

        line.push(unit);
    }

    // And the run before the closing quote, for the same reason.
    for _ in 0..backslashes {
        line.push(BACKSLASH);
    }

    line.push(QUOTE);
}

/// The environment block: `name=value\0` for each, then a final `\0`.
///
/// Sorted, because [`BTreeMap`] already is and Windows documents the block as being in sort order.
/// A `CREATE_UNICODE_ENVIRONMENT` block that is empty is still two NULs rather than nothing, which
/// is the difference between "no variables" and "inherit the parent's".
fn environment_block(env: &BTreeMap<String, String>) -> Vec<u16> {
    let mut block = Vec::new();

    for (name, value) in env {
        block.extend(OsStr::new(name).encode_wide());
        block.push(u16::from(b'='));
        block.extend(OsStr::new(value).encode_wide());
        block.push(0);
    }

    block.push(0);
    block
}

/// A NUL-terminated wide string, for the arguments that are read rather than written.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// The attribute list naming exactly the handles that may be inherited.
///
/// A guard, because `InitializeProcThreadAttributeList` allocates into a buffer that has to be
/// deleted whichever way the spawn ends — and after a *successful* `CreateProcess` the list has been
/// consumed, which is what [`release`](Self::release) records.
struct AttributeList {
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
    buffer: Vec<u8>,
    live: bool,
}

impl AttributeList {
    /// One attribute: the handle list.
    fn naming(handles: &[HANDLE]) -> Result<Self> {
        let mut size: usize = 0;

        #[expect(
            unsafe_code,
            reason = "a null list with a zero size is the documented way to ask how large the \
                      buffer has to be; it writes only to `size`"
        )]
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &raw mut size)
        };

        let mut buffer = vec![0_u8; size];
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = buffer.as_mut_ptr().cast();

        #[expect(
            unsafe_code,
            reason = "the buffer is exactly the length the call above asked for and outlives this \
                      value, which owns it"
        )]
        let made = unsafe { InitializeProcThreadAttributeList(list, 1, 0, &raw mut size) };

        if made == 0 {
            return Err(Error::Os {
                action: "make the list of handles a child may inherit",
                source: io::Error::last_os_error(),
            });
        }

        let attributes = Self {
            list,
            buffer,
            live: true,
        };

        #[expect(
            unsafe_code,
            reason = "the handles are owned by the caller's frame, which outlives both this value \
                      and the spawn that reads the list"
        )]
        let updated = unsafe {
            UpdateProcThreadAttribute(
                attributes.list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                size_of_val(handles),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };

        if updated == 0 {
            return Err(Error::Os {
                action: "name the handles a child may inherit",
                source: io::Error::last_os_error(),
            });
        }

        Ok(attributes)
    }

    /// The spawn succeeded and the list has been consumed; delete it now rather than on drop.
    fn release(&mut self) {
        if !self.live {
            return;
        }

        self.live = false;

        #[expect(
            unsafe_code,
            reason = "the list was initialised by this value, is deleted exactly once, and its \
                      buffer is still alive"
        )]
        unsafe {
            DeleteProcThreadAttributeList(self.list);
        }
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        self.release();

        // Named so a reader does not think the buffer is unused: it is what `list` points into.
        let _ = &self.buffer;
    }
}
