use std::collections::BTreeMap;
use std::os::windows::io::AsRawHandle as _;

use windows_sys::Win32::System::SystemServices::SE_GROUP_ENABLED;

use super::spawn::Arguments;
use super::*;

/// **The assertion T34a exists to make**, and it is made by reading rather than by trying.
///
/// `docs/standards/testing.md` says why: this repository's Windows CI leg holds a *full*
/// token where `BUILTIN\Administrators` is an enabled group, so a test that proved the exclusion
/// by attempting an access would pass on a developer's filtered token and prove nothing at all
/// on the runner — the one machine where it matters. What is asserted instead is the token's own
/// contents, which mean the same thing on both.
///
/// Present **and** disabled, not absent: `CreateRestrictedToken` leaves a disabled SID in the
/// group list marked `SE_GROUP_USE_FOR_DENY_ONLY`, which is exactly what a UAC-filtered token
/// looks like and exactly what `pgwin32_is_admin` answers no to.
#[test]
fn a_restricted_token_holds_administrators_and_grants_nothing_through_it() {
    let token = token().expect("this process can restrict a copy of its own token");
    let groups = groups_of(token.0).expect("a token this process made can be read");

    let administrators = groups
        .iter()
        .find(|(sid, _)| sid == ADMINISTRATORS)
        .unwrap_or_else(|| {
            panic!("{ADMINISTRATORS} is not in the restricted token at all: {groups:?}")
        });

    assert_eq!(
        administrators.1 & SE_GROUP_ENABLED.cast_unsigned(),
        0,
        "Administrators is still enabled in a token that was supposed to disable it: {groups:?}"
    );
}

/// And the same for Power Users, which PostgreSQL's own `restricted_token.c` drops beside it.
///
/// Absent is the ordinary case: Power Users has been an empty group since Vista, so most
/// machines carry no such membership to disable. Its presence is what is asserted about, not its
/// existence.
#[test]
fn a_restricted_token_disables_power_users_too() {
    let token = token().expect("this process can restrict a copy of its own token");
    let groups = groups_of(token.0).expect("a token this process made can be read");

    if let Some((_, attributes)) = groups.iter().find(|(sid, _)| sid == POWER_USERS) {
        assert_eq!(
            *attributes & SE_GROUP_ENABLED.cast_unsigned(),
            0,
            "{groups:?}"
        );
    }
}

/// How a child ended, for a failure that has to say more than "it printed nothing".
///
/// `0xC0000142` is `STATUS_DLL_INIT_FAILED`: the process was created and died before its
/// first instruction, which is what a token that cannot reach what it creates looks like from
/// out here — see [`keep_what_a_child_creates_reachable`].
fn ended(spawned: &Spawned) -> u32 {
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };

    #[expect(
        unsafe_code,
        reason = "the handle is owned by the caller's `Spawned`, which outlives this call, and \
                      neither call closes it"
    )]
    unsafe {
        let handle: HANDLE = spawned.process.as_raw_handle().cast();
        assert_eq!(WaitForSingleObject(handle, INFINITE), WAIT_OBJECT_0);

        let mut code: u32 = 0;
        assert_ne!(GetExitCodeProcess(handle, &raw mut code), 0);

        code
    }
}

/// **The entry really is in the list**, asserted by reading rather than by starting a child.
///
/// `docs/standards/testing.md`'s rule for Windows exclusions, applied to the repair: an
/// ordinary machine's token already grants its own user, so a child started from a restricted
/// copy of it would run whether or not this code did anything at all. What is asserted here is
/// the list's own contents, which mean the same thing on a developer's filtered token and on
/// the runner's full one.
#[test]
fn a_restricted_token_lets_its_user_reach_what_a_child_creates() {
    let token = token().expect("this process can restrict a copy of its own token");

    let user = user_of(&token).expect("a token this process made can be read");

    #[expect(
        unsafe_code,
        reason = "the SID lives in the buffer above, which is still alive"
    )]
    let user =
        render(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }).expect("a SID renders");

    let held = default_dacl(&token).expect("a token this process made can be read");

    #[expect(
        unsafe_code,
        reason = "the list lives in the buffer above, which is still alive"
    )]
    let entries = entries_of(unsafe { (*held.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl });

    assert!(
        entries.contains(&user),
        "{user} is not in the default access control list of the restricted token: {entries:?}"
    );
}

/// Every SID an access control list names, for the assertion above.
fn entries_of(list: *mut ACL) -> Vec<String> {
    use windows_sys::Win32::Security::{ACCESS_ALLOWED_ACE, GetAce};

    let mut found = Vec::new();

    #[expect(unsafe_code, reason = "the list is the caller's and is only read from")]
    let count = unsafe { (*list).AceCount };

    for index in 0..u32::from(count) {
        let mut ace: *mut std::ffi::c_void = std::ptr::null_mut();

        #[expect(
            unsafe_code,
            reason = "the index is below the count read from the list itself"
        )]
        let got = unsafe { GetAce(list, index, &raw mut ace) };

        assert_ne!(got, 0, "{}", io::Error::last_os_error());

        // Every entry in a token's default list is access-allowed, so the SID is at the same
        // offset in each: the `SidStart` field, read as the start of a SID rather than a u32.
        #[expect(
            unsafe_code,
            reason = "an access-allowed entry begins with its own header"
        )]
        let sid = unsafe { (&raw mut (*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart).cast() };

        if let Ok(rendered) = render(sid) {
            found.push(rendered);
        }
    }

    found
}

/// **The experiment that separates the two explanations**, and it is why `spawn_from` takes a
/// token at all.
///
/// If this passes where [`a_restricted_child_runs_and_is_read_back`] fails, then every part of
/// this module's process creation is right on that machine and the only thing it will not
/// accept is the *token* — which means the restricted token is being denied something the
/// child needs, and the window station is the documented candidate. If it fails too, the
/// explanation is in the spawn rather than in the token, and the token is a red herring.
#[test]
fn a_child_from_this_process_own_token_runs() {
    use std::io::Read as _;

    let shell = std::path::PathBuf::from(std::env::var_os("COMSPEC").expect("Windows has a shell"));

    let mut spawned = spawn_from(
        &own_token().expect("this process can open its own token"),
        &shell,
        &Arguments::Quoted(&["/c".into(), "echo unrestricted".into()]),
        &std::env::temp_dir(),
        &crate::process::whole_environment(&BTreeMap::new()),
        None,
    )
    .expect("a child from this process's own token can be created");

    let mut said = String::new();
    spawned
        .stdout
        .read_to_string(&mut said)
        .expect("its stdout is readable");

    assert!(
        said.contains("unrestricted"),
        "a child from this process's *own* token printed nothing: {said:?} \
             exit=0x{:08X}",
        ended(&spawned)
    );
}

/// A child really is created from that token, and says what it was asked to say.
#[test]
fn a_restricted_child_runs_and_is_read_back() {
    use std::io::Read as _;

    let shell = std::path::PathBuf::from(std::env::var_os("COMSPEC").expect("Windows has a shell"));

    let mut spawned = spawn(
        &shell,
        &["/c".into(), "echo restricted".into()],
        &std::env::temp_dir(),
        // The floor a real spawn composes: a shell handed a wholly empty environment cannot
        // find its own system directory and says so instead of doing what it was asked.
        &crate::process::whole_environment(&BTreeMap::new()),
        None,
    )
    .expect("a restricted child can be created from this process's own token");

    let mut said = String::new();
    spawned
        .stdout
        .read_to_string(&mut said)
        .expect("its stdout is readable");

    let mut complained = String::new();
    spawned
        .stderr
        .read_to_string(&mut complained)
        .expect("its stderr is readable");

    assert!(
        said.contains("restricted"),
        "{said:?} {complained:?} exit=0x{:08X}",
        ended(&spawned)
    );
}

/// And it can be handed something to read, which is the ritual's second step.
///
/// `postgres --single` takes its `ALTER ROLE` there and opens no port and no socket while it
/// does, which is the whole reason a superuser password never has to touch disk.
#[test]
fn a_restricted_child_reads_what_it_was_given() {
    use std::io::{Read as _, Write as _};

    let shell = std::path::PathBuf::from(std::env::var_os("COMSPEC").expect("Windows has a shell"));

    let mut spawned = spawn(
        &shell,
        &["/c".into(), "more".into()],
        &std::env::temp_dir(),
        &crate::process::whole_environment(&BTreeMap::new()),
        Some(()),
    )
    .expect("a restricted child can be given something to read");

    let mut writing = spawned.stdin.take().expect("it was given a pipe");
    writing
        .write_all(b"mixengine\r\n")
        .expect("its stdin is writable");
    drop(writing);

    let mut said = String::new();
    spawned
        .stdout
        .read_to_string(&mut said)
        .expect("its stdout is readable");

    assert!(
        said.contains("mixengine"),
        "{said:?} exit=0x{:08X}",
        ended(&spawned)
    );
}
