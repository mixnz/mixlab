use std::ffi::OsString;
use std::path::Path;

use super::program_on_path;

/// A `PATH` of exactly these directories, joined the way this system joins one.
fn a_path_of(directories: &[&Path]) -> OsString {
    std::env::join_paths(directories.iter().map(|one| one.to_path_buf())).expect("a PATH")
}

/// A file the shell would run as `name`, on this system.
fn a_program_called(directory: &Path, name: &str) {
    #[cfg(windows)]
    {
        std::fs::write(directory.join(format!("{name}.bat")), "@echo hello\r\n")
            .expect("a batch file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let file = directory.join(name);
        std::fs::write(&file, "#!/bin/sh\necho hello\n").expect("a script");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
            .expect("an executable bit");
    }
}

/// **The parent table knows this process, and knows its parent** — roadmap task **T181**.
///
/// Asserted against the process running the test because it is the one process every system
/// guarantees is there and this account may ask about. Windows has no parent query in `std`,
/// so there the claim is only that this process is listed with one.
#[cfg(feature = "host")]
#[test]
fn the_parent_table_knows_this_process_and_its_parent() {
    let table = super::parent_table().expect("this machine's processes can be listed");
    let parent = table.get(&std::process::id()).copied();

    #[cfg(unix)]
    assert_eq!(parent, Some(std::os::unix::process::parent_id()));

    #[cfg(windows)]
    assert!(parent.is_some_and(|parent| parent != 0), "{parent:?}");

    assert!(table.len() > 1, "more than this one process is running");
}

/// **The rule is the shell's** — roadmap task **T78b**, its design's D4. A program the shell
/// would run is found; a name nothing answers to is not.
#[test]
fn a_program_is_found_by_the_rule_the_shell_uses() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    a_program_called(temp.path(), "tool");

    let path = a_path_of(&[temp.path()]);

    assert!(program_on_path("tool", &path).is_some());
    assert!(program_on_path("nothing-of-that-name", &path).is_none());
}

/// **A bare file with no extension is not a program to `cmd.exe`** (D4) — which is exactly the
/// file a person copies from a Unix machine, and exactly what `cmd.exe` said about it.
#[cfg(windows)]
#[test]
fn a_bare_name_with_no_extension_is_not_a_program() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(temp.path().join("composer"), "#!/bin/sh\n").expect("a file");

    assert!(program_on_path("composer", &a_path_of(&[temp.path()])).is_none());

    // And an extension of its own is tried as written.
    std::fs::write(temp.path().join("run.cmd"), "@echo hello\r\n").expect("a file");
    assert!(program_on_path("run.cmd", &a_path_of(&[temp.path()])).is_some());
}

/// **A file without an execute bit is not a program to `execvp`** (D4).
#[cfg(unix)]
#[test]
fn a_file_without_an_execute_bit_is_not_a_program() {
    use std::os::unix::fs::PermissionsExt as _;

    let temp = tempfile::tempdir().expect("a temporary directory");
    let dull = temp.path().join("dull");
    std::fs::write(&dull, "#!/bin/sh\n").expect("a file");
    std::fs::set_permissions(&dull, std::fs::Permissions::from_mode(0o644)).expect("no bit");

    assert!(program_on_path("dull", &a_path_of(&[temp.path()])).is_none());
}

/// An empty entry means the current directory to the shell and is skipped here (D4); a
/// directory that does not exist is skipped rather than being an error.
#[test]
fn an_empty_entry_and_a_missing_directory_are_skipped() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    a_program_called(temp.path(), "tool");
    let missing = temp.path().join("nowhere");

    let path = a_path_of(&[Path::new(""), &missing, temp.path()]);

    assert_eq!(
        program_on_path("tool", &path).and_then(|found| found.parent().map(Path::to_path_buf)),
        Some(temp.path().to_path_buf())
    );
}
