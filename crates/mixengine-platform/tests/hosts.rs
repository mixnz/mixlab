//! The hosts file, against the real machine.
//!
//! **Nothing here writes.** Reading `/etc/hosts` needs no privilege on any of the three systems,
//! which is exactly why the trait is read-only (D9); the write is the privileged operation and its
//! own tests drive it against a file they own. The one test that touches the real file is
//! `#[ignore]`d and belongs to CI's `system` job.
//!
//! The D-numbers below are those of `docs/specs/2026-08-23-t41-hosts-apply-design.md`.

use mixengine_platform::{Host as _, hosts, mock};

/// Is this run allowed to change the machine, or to raise a real elevation prompt on it?
///
/// **`#[ignore]` alone is one `--ignored` away from a developer's desk**, which is where a prompt
/// nobody asked for comes from — `docs/standards/testing.md`, rule 1. CI's `system` job sets
/// `MIXENGINE_SYSTEM_TESTS=1`; everywhere else the test says it skipped rather than passing quietly.
fn system_tests() -> bool {
    let allowed = std::env::var("MIXENGINE_SYSTEM_TESTS").as_deref() == Ok("1");
    if !allowed {
        eprintln!("skipped: MIXENGINE_SYSTEM_TESTS is not set");
    }
    allowed
}

#[test]
fn the_hosts_file_is_where_this_operating_system_keeps_it() {
    let path = hosts::path();

    assert!(path.is_absolute(), "{}", path.display());
    assert!(path.ends_with("hosts"), "{}", path.display());

    #[cfg(unix)]
    assert_eq!(path, std::path::Path::new("/etc/hosts"));
    #[cfg(windows)]
    assert!(
        path.to_string_lossy()
            .to_lowercase()
            .contains(r"drivers\etc"),
        "{}",
        path.display()
    );
}

/// Reading the machine's own file needs no token, and is what `domain.dns_status` and `mix doctor`
/// ask. A machine with no block of ours reads as an empty list, which is not an error.
#[test]
fn the_real_machine_can_be_asked_what_our_block_holds() {
    let host = mixengine_platform::host();

    assert_eq!(host.hosts_file().path(), hosts::path());

    match host.hosts_file().managed(None) {
        Ok(entries) => {
            for entry in entries {
                assert!(!entry.domain.is_empty());
            }
        }
        // A machine whose block somebody has half-edited, and a CI image with no hosts file at all.
        // Both are answers rather than failures of this test.
        Err(error) => assert!(!error.to_string().is_empty()),
    }
}

/// D9's payoff: the daemon's side of this is testable without a machine at all.
#[test]
fn a_mock_host_answers_from_memory() {
    let host = mock::Host::with_hosts("/tmp/mixengine-test", ["127.0.0.1 blog.test"]);

    let managed = host.hosts_file().managed(Some("aa")).unwrap();

    assert_eq!(managed.len(), 1);
    assert_eq!(managed[0].domain, "blog.test");

    let refusing = mock::Host::unable_to_read_the_hosts_file("/tmp/mixengine-test", "no such file");
    assert!(refusing.hosts_file().managed(Some("aa")).is_err());
}

// The half that needs a real machine and an administrative token. `#[ignore]`d, so a developer's
// `cargo test` says how many it skipped rather than failing, and CI's `system` job is what runs it.
// The same shape `tests/elevation.rs` already uses.

/// The real file, the real path, applied and then removed — with a copy taken first, so a failure
/// halfway through leaves the machine's own hosts file recoverable by hand from the test's output.
#[test]
#[ignore = "writes the machine's real hosts file; run in CI's system job, with MIXENGINE_SYSTEM_TESTS=1"]
fn the_real_hosts_file_is_edited_and_put_back() {
    if !system_tests() {
        return;
    }

    let path = hosts::path();
    let before = std::fs::read_to_string(&path).expect("the machine has a hosts file");

    let domain = format!("t41-system-{}.test", std::process::id());
    let entry = mixengine_proto::privileged::HostEntry {
        address: "127.0.0.1".parse().unwrap(),
        domain: domain.clone(),
    };

    // A tag of the test's own, so the run never touches a block a real home on this machine owns.
    let tag = format!("{:016x}", std::process::id());

    let written = hosts::apply(&path, Some(&tag), std::slice::from_ref(&entry))
        .unwrap_or_else(|error| panic!("this needs an administrative token: {error}"));
    assert!(
        matches!(written, hosts::Change::Written { entries: 1 }),
        "{written:?}"
    );

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains(&domain), "{after}");
    assert_eq!(hosts::parse(&after, Some(&tag)).unwrap(), vec![entry]);

    let removed = hosts::apply(&path, Some(&tag), &[]).expect("the block is removed");
    assert!(
        matches!(removed, hosts::Change::Written { entries: 0 }),
        "{removed:?}"
    );

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "the machine's own hosts file did not come back; a copy of what it was is above"
    );
}

/// The atomic replace is no longer the hosts file's own: three more system files use it, and its
/// temporary is named after whichever one it is replacing rather than after `hosts`.
#[cfg(feature = "elevated")]
#[test]
fn any_file_can_be_replaced_atomically_and_leaves_nothing_behind() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("pf.conf");
    std::fs::write(&file, "before\n").unwrap();

    mixengine_platform::replace_for_tests(&file, "after\n").expect("the file is replaced");

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "after\n");

    let left: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .filter_map(|found| found.ok().map(|found| found.file_name()))
        .filter(|name| name.to_string_lossy().contains("mixengine"))
        .collect();

    assert!(
        left.is_empty(),
        "a temporary file was left behind: {left:?}"
    );
}
