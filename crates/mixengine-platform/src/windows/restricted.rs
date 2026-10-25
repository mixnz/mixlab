//! A child created from a restricted copy of this process's token.
//!
//! **Why this exists, in one sentence a reader can check:** `postgres` calls `check_root()` at
//! startup, which on Windows asks `pgwin32_is_admin()`, and a process whose token holds
//! `BUILTIN\Administrators` as an *enabled* group is refused with *Execution of PostgreSQL by a
//! user with administrative permissions is not permitted*.
//!
//! An ordinary machine never meets that: an interactive administrator carries a UAC-*filtered*
//! token where the group is present deny-only and grants nothing. The machine that meets it is this
//! repository's own Windows CI leg, which holds a full token deliberately and asserts that it still
//! does (`.github/workflows/_test.yml`).
//!
//! **So every supervised child is created from a restricted token, not only PostgreSQL's.** The
//! decision and its reasons are
//! `docs/decisions/0010-supervised-child-never-inherits-administrators.md`; the shortest of them
//! is that on a normal machine this changes nothing, because disabling a group that is already
//! deny-only is a no-op.
//!
//! The two SIDs dropped are the two PostgreSQL's own `src/common/restricted_token.c` drops, for the
//! same reason and in the same order.
//!
//! # Why this is hand-rolled rather than a `Command`
//!
//! [`std::process::Command`] has no way to state a token, and `CreateProcessAsUserW` hands back a
//! raw handle that no [`std::process::Child`] can be built from. What buys the cost is a second
//! thing this gets for free: handles are inherited **explicitly**, through
//! `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, rather than by setting `bInheritHandles` and hoping — which
//! is the window `hide_stdio_from_children` exists to guard on the paths that still use `Command`.
//!
//! `CreateProcessAsUserW` needs no privilege when the token is a restricted version of the caller's
//! own. That special case is why `initdb` and `pg_ctl` can already do this to themselves, and it is
//! why nothing here goes through `mixengine-elevate`.
//!
//! # A weaker token still has to reach what its child creates
//!
//! Disabling a group a token's *default* access control list is written in terms of leaves a child
//! that cannot open the objects it creates itself, and a process in that state cannot finish
//! starting. See [`keep_what_a_child_creates_reachable`], which is where the measurement that found
//! it is written down.

use std::fs::File;
use std::io;
use std::os::windows::io::OwnedHandle;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GENERIC_ALL, HANDLE, LocalFree, WIN32_ERROR};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, NO_MULTIPLE_TRUSTEE, SetEntriesInAclW, TRUSTEE_IS_SID,
    TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACL, AllocateAndInitializeSid, CreateRestrictedToken, FreeSid, GetTokenInformation, PSID,
    SECURITY_NT_AUTHORITY, SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ALL_ACCESS,
    TOKEN_DEFAULT_DACL, TOKEN_USER, TokenDefaultDacl, TokenUser,
};
use windows_sys::Win32::System::SystemServices::{
    DOMAIN_ALIAS_RID_ADMINS, DOMAIN_ALIAS_RID_POWER_USERS, SECURITY_BUILTIN_DOMAIN_RID,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::sid::Token;
use crate::{Error, Result};

// Reading a token's *groups* back is the assertions' business and nobody else's — see [`groups_of`].
// Its user is read by [`admit`] and is not conditional.
#[cfg(test)]
use super::sid::render;
#[cfg(test)]
use windows_sys::Win32::Security::{TOKEN_GROUPS, TokenGroups};

mod spawn;

#[cfg(test)]
use spawn::spawn_from;
pub(crate) use spawn::{spawn, spawn_raw};

/// `BUILTIN\Administrators`, the group `pgwin32_is_admin` looks for.
///
/// Named only by the assertions: the token is built from RIDs, and this is how the result is read
/// back. See [`groups_of`].
#[cfg(test)]
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// `BUILTIN\Power Users`, dropped beside it because PostgreSQL drops it.
#[cfg(test)]
const POWER_USERS: &str = "S-1-5-32-547";

/// A child created from a restricted token, and everything the caller owns of it.
#[derive(Debug)]
pub(crate) struct Spawned {
    /// The process itself. Closed when this is dropped, which does **not** end the process.
    pub(crate) process: OwnedHandle,

    /// Its pid, for the job object and for the log line.
    pub(crate) pid: u32,

    /// Its standard output, already ours to read.
    pub(crate) stdout: File,

    /// Its standard error, likewise.
    pub(crate) stderr: File,

    /// Its standard input, for a caller that asked for one — `postgres --single`, and nothing else.
    pub(crate) stdin: Option<File>,
}

/// A restricted copy of this process's token: Administrators and Power Users disabled.
///
/// # Errors
///
/// [`Error::Os`] when the process token cannot be opened or restricted, which means a machine where
/// something has gone very wrong — this is a copy of a token this process already holds.
pub(crate) fn token() -> Result<Token> {
    let original = own_token()?;

    let admins = builtin(DOMAIN_ALIAS_RID_ADMINS)?;
    let power = builtin(DOMAIN_ALIAS_RID_POWER_USERS)?;

    let disable = [
        SID_AND_ATTRIBUTES {
            Sid: admins.0,
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: power.0,
            Attributes: 0,
        },
    ];

    let mut restricted: HANDLE = std::ptr::null_mut();

    #[expect(
        unsafe_code,
        reason = "the SIDs live in the two guards above and outlive the call; the array is a local \
                  of exactly the length passed; the new token handle is written into a local this \
                  function owns and is wrapped in a guard immediately"
    )]
    let made = unsafe {
        CreateRestrictedToken(
            original.0,
            0,
            2,
            disable.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            &raw mut restricted,
        )
    };

    if made == 0 {
        return Err(Error::Os {
            action: "restrict a copy of this process's access token",
            source: io::Error::last_os_error(),
        });
    }

    let restricted = Token(restricted);
    keep_what_a_child_creates_reachable(&restricted)?;

    Ok(restricted)
}

/// Give a restricted token's own user a say over the objects a child creates with it.
///
/// # What was measured, and where
///
/// A child created from the restricted token was created and then died with `0xC0000142` —
/// `STATUS_DLL_INIT_FAILED`, before its first instruction — having printed nothing, while the same
/// spawn from this process's own token ran and printed. Bisecting the restriction on an elevated
/// machine said the rest: a token with nothing disabled runs, a token with Power Users disabled
/// runs, and a token with **`BUILTIN\Administrators`** disabled does not.
///
/// The reason is the token's **default access control list**, which is what every kernel object a
/// process creates without a security descriptor of its own is given — including the ones the
/// loader and CSRSS create on its behalf while it is starting. An elevated administrator's token
/// carries a default list granting `NT AUTHORITY\SYSTEM` and `BUILTIN\Administrators`, and nothing
/// else, because `BUILTIN\Administrators` is that token's owner. Disable that group and the child
/// has no access to the objects it creates itself, which is a process that cannot finish starting.
///
/// The window station was the plausible candidate and was measured to be innocent: granting the
/// user `WINSTA_ALL_ACCESS` on `WinSta0` and every right on its desktop changed nothing, and adding
/// the user to the default list alone was enough.
///
/// # What is added
///
/// One allow entry for the token's own user, merged into the list that is already there. It is not
/// conditional on anything: this changes a token this function has just made and no object anybody
/// else can see, and a user who cannot reach what their own process creates is wrong on every
/// machine, not only on the one where it was noticed.
///
/// # Errors
///
/// [`Error::Os`] when the token's user or default list cannot be read, when the entry cannot be
/// merged into it, or when the result cannot be set back on the token.
fn keep_what_a_child_creates_reachable(token: &Token) -> Result<()> {
    let user = user_of(token)?;

    #[expect(
        unsafe_code,
        reason = "`user_of` filled the buffer with exactly a TOKEN_USER, and the SID it points at \
                  lives inside that same buffer, which outlives the merge below"
    )]
    let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };

    let held = default_dacl(token)?;

    #[expect(
        unsafe_code,
        reason = "`default_dacl` filled the buffer with exactly a TOKEN_DEFAULT_DACL, and the list \
                  it points at lives inside that same buffer"
    )]
    let existing = unsafe { (*held.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl };

    let entry = allow(sid, GENERIC_ALL);
    let mut merged: *mut ACL = std::ptr::null_mut();

    #[expect(
        unsafe_code,
        reason = "the entry and the SID it names both outlive the call; the list it allocates is \
                  written into a local and wrapped in a guard immediately"
    )]
    let built = unsafe { SetEntriesInAclW(1, &raw const entry, existing, &raw mut merged) };

    if built != ERROR_SUCCESS {
        return Err(refused(
            "add this user to the default access control list of a restricted token",
            built,
        ));
    }

    let merged = Local(merged.cast());

    let replacement = TOKEN_DEFAULT_DACL {
        DefaultDacl: merged.0.cast(),
    };

    #[expect(
        unsafe_code,
        reason = "the token was opened for TOKEN_ALL_ACCESS, and the list is the one just built, \
                  still owned by the guard above"
    )]
    let set = unsafe {
        SetTokenInformation(
            token.0,
            TokenDefaultDacl,
            (&raw const replacement).cast(),
            u32::try_from(size_of::<TOKEN_DEFAULT_DACL>()).unwrap_or(u32::MAX),
        )
    };

    if set == 0 {
        return Err(Error::Os {
            action: "set the default access control list of a restricted token",
            source: io::Error::last_os_error(),
        });
    }

    Ok(())
}

/// One access-allowed entry, naming a SID directly rather than an account.
fn allow(sid: PSID, rights: u32) -> EXPLICIT_ACCESS_W {
    EXPLICIT_ACCESS_W {
        grfAccessPermissions: rights,
        grfAccessMode: GRANT_ACCESS,
        // Nothing is created *under* a token, so there is nothing for this entry to propagate to.
        grfInheritance: 0,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        },
    }
}

/// A token's default access control list, as the buffer `GetTokenInformation` filled.
///
/// The buffer is what is handed back rather than the list: the `ACL` inside a `TOKEN_DEFAULT_DACL`
/// points into the same allocation, so returning it alone would be returning a dangling pointer.
///
/// # Errors
///
/// [`Error::Os`] when the token cannot be queried.
fn default_dacl(token: &Token) -> Result<Vec<u64>> {
    // The first call is expected to fail and only the size it writes back is of interest, which is
    // why its return value is ignored — the same shape as [`user_of`].
    let mut needed: u32 = 0;

    #[expect(
        unsafe_code,
        reason = "a null buffer with a zero length is the documented way to ask how large the \
                  answer is; it writes only to `needed`"
    )]
    unsafe {
        GetTokenInformation(
            token.0,
            TokenDefaultDacl,
            std::ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };

    // `Vec<u64>` rather than `Vec<u8>`, because the bytes are read back as a `TOKEN_DEFAULT_DACL`
    // and that contains a pointer: a byte vector guarantees an alignment of one.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>()).max(1)];

    #[expect(
        unsafe_code,
        reason = "the buffer is at least `needed` bytes long, is aligned for the pointer inside \
                  TOKEN_DEFAULT_DACL, and is what the caller keeps alive"
    )]
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenDefaultDacl,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    };

    if read == 0 {
        return Err(Error::Os {
            action: "read the default access control list of a restricted token",
            source: io::Error::last_os_error(),
        });
    }

    Ok(buffer)
}

/// One of these failures, as an [`Error::Os`] carrying a code that was returned rather than set.
fn refused(action: &'static str, code: WIN32_ERROR) -> Error {
    Error::Os {
        action,
        source: io::Error::from_raw_os_error(code.cast_signed()),
    }
}

/// The user a token names, as the buffer `GetTokenInformation` filled.
///
/// The buffer is what is handed back rather than the SID: the `PSID` inside a `TOKEN_USER` points
/// into the same allocation, so returning it alone would be returning a dangling pointer.
///
/// # Errors
///
/// [`Error::Os`] when the token cannot be queried.
fn user_of(token: &Token) -> Result<Vec<u64>> {
    // Asked for twice, as in [`default_dacl`] and for the reason `sid::of_token` writes down.
    let mut needed: u32 = 0;

    #[expect(
        unsafe_code,
        reason = "a null buffer with a zero length is the documented way to ask how large the \
                  answer is; it writes only to `needed`"
    )]
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &raw mut needed)
    };

    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>()).max(1)];

    #[expect(
        unsafe_code,
        reason = "the buffer is at least `needed` bytes long, is aligned for the pointer inside \
                  TOKEN_USER, and is what the caller keeps alive"
    )]
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    };

    if read == 0 {
        return Err(Error::Os {
            action: "read the user out of a restricted access token",
            source: io::Error::last_os_error(),
        });
    }

    Ok(buffer)
}

/// A block one of the access-control functions allocated, freed on drop.
struct Local(*mut std::ffi::c_void);

impl Drop for Local {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }

        #[expect(
            unsafe_code,
            reason = "the pointer came from an access-control function that documents LocalFree as \
                      its release, and this value owns it"
        )]
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// This process's own access token, unrestricted.
///
/// Split out from [`token`] so that the same spawn can be run with and without the restriction —
/// which is the one experiment that tells a spawn this machine will not perform apart from a token
/// this machine will not grant. See `a_child_from_this_process_own_token_runs`.
///
/// # Errors
///
/// [`Error::Os`] when this process cannot open a token it already holds.
fn own_token() -> Result<Token> {
    let mut opened: HANDLE = std::ptr::null_mut();

    #[expect(
        unsafe_code,
        reason = "GetCurrentProcess returns a pseudo-handle that needs no closing, and the token \
                  handle is written into a local this function owns"
    )]
    let got = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &raw mut opened) };

    if got == 0 {
        return Err(Error::Os {
            action: "open this process's access token",
            source: io::Error::last_os_error(),
        });
    }

    Ok(Token(opened))
}

/// One of the two `BUILTIN\` SIDs, released when the guard is dropped.
fn builtin(rid: i32) -> Result<Sid> {
    let authority = SECURITY_NT_AUTHORITY;
    let mut sid: PSID = std::ptr::null_mut();

    #[expect(
        unsafe_code,
        reason = "the authority is a local that outlives the call, and the SID it allocates is \
                  written into a local this function owns and is freed by the guard below"
    )]
    let made = unsafe {
        AllocateAndInitializeSid(
            &raw const authority,
            2,
            SECURITY_BUILTIN_DOMAIN_RID.cast_unsigned(),
            rid.cast_unsigned(),
            0,
            0,
            0,
            0,
            0,
            0,
            &raw mut sid,
        )
    };

    if made == 0 {
        return Err(Error::Os {
            action: "name a built-in group",
            source: io::Error::last_os_error(),
        });
    }

    Ok(Sid(sid))
}

/// A SID from `AllocateAndInitializeSid`, freed on drop.
struct Sid(PSID);

impl Drop for Sid {
    #[expect(
        unsafe_code,
        reason = "the SID came from AllocateAndInitializeSid, is owned by this guard, and is freed \
                  exactly once"
    )]
    fn drop(&mut self) {
        unsafe {
            FreeSid(self.0);
        }
    }
}

/// Every group in `token`, as `(SID, attributes)`.
///
/// **For the test that proves the exclusion structurally**, and for nothing else: reading a token is
/// how `docs/standards/testing.md` says a Windows exclusion must be asserted.
///
/// # Errors
///
/// [`Error::Os`] when the token cannot be queried.
#[cfg(test)]
fn groups_of(token: HANDLE) -> Result<Vec<(String, u32)>> {
    let mut needed: u32 = 0;

    #[expect(
        unsafe_code,
        reason = "GetTokenInformation with a null buffer and a zero length is the documented way \
                  to ask how large the answer is; it writes only to `needed`"
    )]
    unsafe {
        GetTokenInformation(token, TokenGroups, std::ptr::null_mut(), 0, &raw mut needed)
    };

    // `Vec<u64>` for the reason `sid.rs` uses one: the bytes are read back as a `TOKEN_GROUPS`,
    // which contains pointers and wants pointer alignment.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>()).max(1)];

    #[expect(
        unsafe_code,
        reason = "the buffer is at least `needed` bytes long, is aligned for the pointers inside \
                  TOKEN_GROUPS, and outlives every read below"
    )]
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenGroups,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    };

    if read == 0 {
        return Err(Error::Os {
            action: "read the groups out of an access token",
            source: io::Error::last_os_error(),
        });
    }

    let mut groups = Vec::new();

    #[expect(
        unsafe_code,
        reason = "the call above filled the buffer with exactly a TOKEN_GROUPS followed by \
                  GroupCount entries, and every SID it names lives inside that same buffer"
    )]
    let entries = unsafe {
        let header = buffer.as_ptr().cast::<TOKEN_GROUPS>();
        let count = (*header).GroupCount as usize;
        let first = (&raw const (*header).Groups).cast::<SID_AND_ATTRIBUTES>();

        (0..count)
            .map(|index| {
                let entry = &*first.add(index);

                (entry.Sid, entry.Attributes)
            })
            .collect::<Vec<_>>()
    };

    for (sid, attributes) in entries {
        groups.push((render(sid)?, attributes));
    }

    Ok(groups)
}

#[cfg(test)]
mod tests;
