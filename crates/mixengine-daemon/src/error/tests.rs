use std::path::PathBuf;

use mixengine_core::services::GraphError;
use mixengine_proto::ServiceId;

use super::*;

#[test]
fn an_io_failure_keeps_the_path_and_says_who_is_not_going_to_fix_it() {
    let error = mixengine_core::Error::Io {
        action: "create",
        path: PathBuf::from("/srv/mixengine"),
        source: io::Error::from(io::ErrorKind::PermissionDenied),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::Io);
    assert!(
        error.message.starts_with("cannot create /srv/mixengine: "),
        "the cause is appended, not dropped: {}",
        error.message
    );
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("/srv/mixengine")),
        "the hint names the path the user has to act on: {:?}",
        error.hint
    );
}

#[test]
fn a_path_that_is_not_there_points_at_the_relocation_and_not_at_a_parent() {
    let error = mixengine_core::Error::Io {
        action: "create directory",
        path: PathBuf::from("Z:/mixengine/data"),
        source: io::Error::from(io::ErrorKind::NotFound),
    }
    .to_wire();

    let hint = error.hint.expect("a missing path has advice attached");
    assert!(hint.contains("[paths]"), "{hint}");
    // `create` goes through `create_dir_all`, which makes the parents itself, and the same
    // variant covers `read`, where the leaf is what is missing. Neither claim is safe to make.
    assert!(!hint.contains("directory above"), "{hint}");
}

#[test]
fn a_missing_entity_is_answered_with_the_command_that_lists_them() {
    let error = mixengine_core::Error::NotFound {
        kind: "site",
        id: "blog.test".to_owned(),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.message, "no such site: blog.test");
    assert_eq!(
        error.hint.as_deref(),
        Some("`mix site list` shows what does exist")
    );
}

/// A data directory somebody else holds is the caller's mistake, not the daemon's — T36.
///
/// `internal` is what the catch-all would have made of it, and `internal` tells a person to
/// report a bug about a create they can fix by naming another directory. The hint has to name
/// the service already there, because the repair is a choice between two directories and one of
/// them is in use.
#[test]
fn a_data_directory_already_in_use_is_the_caller_s_to_fix() {
    let error = mixengine_core::Error::Service(ServiceError::DataDirectoryTaken {
        path: "/srv/db".to_owned(),
        holder: "mariadb@main".to_owned(),
    })
    .to_wire();

    assert_eq!(error.code, ErrorCode::AlreadyExists);
    assert_eq!(
        error.message,
        "mariadb@main already keeps its data in /srv/db"
    );
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("mariadb@main")),
        "the repair is picking another directory, and the hint says who is in this one: {:?}",
        error.hint
    );
}

/// The one a user can actually fix, and the one `internal` would have hidden: the loop has to
/// survive into the message, and the code has to say whose fault it is.
#[test]
fn a_dependency_loop_is_the_declaration_s_fault_and_says_so() {
    let id = |value: &str| ServiceId::parse(value).expect("a valid service id");
    let error = mixengine_core::Error::Graph(GraphError::Cycle {
        path: vec![id("caddy"), id("php-fpm@8.3"), id("mariadb@main")],
    })
    .to_wire();

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(
        error
            .message
            .ends_with("caddy → php-fpm@8.3 → mariadb@main → caddy"),
        "the loop is what says which edge to delete: {}",
        error.message
    );
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("extension.toml")),
        "the message names the services; the hint says where they were written: {:?}",
        error.hint
    );
}

/// The other half of the same variant, which is not a declaration failure at all.
#[test]
fn asking_about_a_service_that_is_not_there_is_a_plain_not_found() {
    let error = mixengine_core::Error::Graph(GraphError::NoSuchService {
        id: ServiceId::parse("mailpit").expect("a valid service id"),
    })
    .to_wire();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(
        error.hint.as_deref(),
        Some("`mix service list` shows what does exist")
    );
}

#[test]
fn an_io_failure_with_nothing_to_suggest_carries_no_hint() {
    let error = mixengine_core::Error::Io {
        action: "read",
        path: PathBuf::from("/srv/mixengine/config.toml"),
        source: io::Error::from(io::ErrorKind::InvalidData),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::Io);
    assert_eq!(error.hint, None);
}

#[test]
fn a_platform_failure_keeps_its_own_code_through_core() {
    let error = mixengine_core::Error::Platform(mixengine_platform::Error::UnsupportedPlatform {
        capability: "PortAccess",
        reason: "no pf on this system".to_owned(),
    })
    .to_wire();

    assert_eq!(error.code, ErrorCode::UnsupportedPlatform);
    assert_eq!(
        error.message,
        "PortAccess is not available on this platform: no pf on this system"
    );
}

#[test]
fn a_transparent_variant_is_not_printed_twice() {
    // What `chain` would do to `Core::Platform` if the mapping flattened it instead of
    // delegating: thiserror keeps the inner error as the source *and* borrows its message.
    let error = mixengine_core::Error::Platform(mixengine_platform::Error::NoHomeDirectory {
        reason: "%LOCALAPPDATA% is not set",
    });

    let message = chain(&error);
    assert_eq!(message, error.to_string());
    assert_eq!(message.matches("%LOCALAPPDATA%").count(), 1);
}

#[test]
fn a_missing_home_directory_is_a_precondition_not_an_io_failure() {
    let error = mixengine_platform::Error::NoHomeDirectory {
        reason: "$HOME is not set",
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::PreconditionFailed);
    // The message already ends in "set MIXENGINE_HOME to choose one explicitly".
    assert_eq!(error.hint, None);
}

#[test]
fn an_endpoint_held_by_another_account_is_not_offered_as_a_daemon_to_stop() {
    // The hint for `EndpointInUse` says "`mix daemon stop` ends it", which is the wrong thing
    // to be doing when the name is somebody else's: there is no daemon of the user's on it, and
    // the advice would send them looking for one.
    let error = mixengine_platform::Error::EndpointNotOurs {
        address: r"\\.\pipe\mixengine.S-1-5-21-1-2-3-1001.6bf2c0d4e5a19837".to_owned(),
        account: "S-1-5-21-1-2-3-1002".to_owned(),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::Conflict);
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| !hint.contains("mix daemon stop")),
        "the hint sends the user after a daemon of their own: {:?}",
        error.hint
    );
}

#[test]
fn an_empty_home_is_an_invalid_argument() {
    let error = mixengine_core::Error::EmptyHome.to_wire();

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(error.hint, None);
}

/// **T182f.** A directory an earlier home left is `already_exists`, and the hint is one sentence
/// naming both ways on — with no run of spaces where a line continuation went missing.
#[test]
fn an_unrecorded_install_names_adopt_and_the_directory() {
    let error = mixengine_core::Error::Runtime(RuntimeError::UnrecordedInstall {
        path: PathBuf::from("/data/runtimes/node/24.19.0"),
        reason: "its marker names another install".to_owned(),
    })
    .to_wire();

    assert_eq!(error.code, ErrorCode::AlreadyExists);
    let hint = error.hint.expect("a hint");
    assert!(hint.contains("mix runtime adopt"), "{hint}");
    assert!(hint.contains("/data/runtimes/node/24.19.0"), "{hint}");
    assert!(!hint.contains("  "), "{hint:?}");
}

#[test]
fn a_configuration_that_does_not_parse_names_the_file_in_its_hint() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let file = home.path().join(mixengine_core::config::FILE_NAME);
    std::fs::write(&file, "[log]\nlevel = \"chatty\"\n").expect("write the broken config");

    let error = mixengine_core::config::load(&file)
        .expect_err("`chatty` is not a level")
        .to_wire();

    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("config.toml")),
        "the hint names the file to fix: {:?}",
        error.hint
    );
    // `toml` ends its complaint with a newline, which would print a blank line above the hint.
    assert_eq!(error.message.trim_end(), error.message);
    assert!(
        error.message.contains("unknown variant `chatty`"),
        "the parse failure is what makes this actionable: {}",
        error.message
    );
}

#[test]
fn a_database_that_cannot_be_opened_names_the_file_and_not_the_query() {
    let error = mixengine_core::Error::Database {
        action: "open",
        path: PathBuf::from("Z:/mixengine/mixengine.db"),
        source: sqlx::Error::Io(io::Error::from(io::ErrorKind::NotFound)),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::Io);
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("Z:/mixengine/mixengine.db")),
        "{:?}",
        error.hint
    );
}

#[test]
fn a_database_from_another_build_is_something_the_user_can_fix() {
    let error = mixengine_core::Error::IncompatibleDatabase {
        path: PathBuf::from("/home/dev/.local/share/mixengine/mixengine.db"),
        source: sqlx::migrate::MigrateError::VersionNotPresent(2),
    }
    .to_wire();

    // Not `internal`: nothing is broken, the file is simply newer than this binary.
    assert_eq!(error.code, ErrorCode::PreconditionFailed);

    // **The database's own path.** Every action available here is about that file — running
    // what wrote it, or moving it aside — and the daemon is not listening, so no `mix` command
    // can lead the reader to it afterwards. The `.bak-…` copy is mentioned by the hint and is
    // deliberately not asserted: whether it opens depends on how this database got ahead of
    // this build, which is not something the hint can know.
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("/home/dev/.local/share/mixengine/mixengine.db")),
        "the way out is about the database itself: {:?}",
        error.hint
    );
}

#[test]
fn a_migration_that_fails_is_ours_and_says_the_database_is_untouched() {
    let error = mixengine_core::Error::Migration {
        path: PathBuf::from("/home/dev/mixengine.db"),
        source: sqlx::migrate::MigrateError::VersionMissing(1),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::Internal);
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("left as it was")),
        "{:?}",
        error.hint
    );
}

#[test]
fn a_backup_that_cannot_be_written_stops_the_upgrade_and_says_so() {
    let error = mixengine_core::Error::Backup {
        path: PathBuf::from("/home/dev/mixengine.db.bak-0.1.0"),
        source: sqlx::Error::Io(io::Error::from(io::ErrorKind::StorageFull)),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::Io);
    let hint = error.hint.expect("a failed backup has advice attached");
    assert!(hint.contains("mixengine.db.bak-0.1.0"), "{hint}");
    // The database is untouched, and the user needs to know that before they panic.
    assert!(hint.contains("stopped rather than migrate"), "{hint}");
}

#[test]
fn a_program_that_is_not_installed_is_a_missing_dependency() {
    let error = mixengine_supervisor::Error::Spawn {
        program: "php-fpm".to_owned(),
        source: io::Error::from(io::ErrorKind::NotFound),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::DependencyMissing);
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("php-fpm")),
        "the hint names the program: {:?}",
        error.hint
    );
}

#[test]
fn a_program_that_will_not_run_is_a_failed_process() {
    let error = mixengine_supervisor::Error::Spawn {
        program: "php-fpm".to_owned(),
        source: io::Error::from(io::ErrorKind::PermissionDenied),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::ProcessFailed);
}

/// **An image this machine's policy refused is neither of the two above** — roadmap task
/// **T94**. It is not a missing dependency and not a permission bit, it will not start on the
/// next attempt, and the OS error number is the only thing that says so — which is why the
/// hint says it in words.
///
/// **Both arms in one test, through `cfg!` as a value.** The same number means nothing on macOS
/// or Linux, and a `#[cfg(windows)]` here would be this crate compiling a line away by
/// operating system — which `workspace_layering` refuses. The classifier's own `cfg!(windows)`
/// is the thing under test, so asserting it from the caller's side is what keeps the two from
/// drifting.
#[test]
fn an_image_the_policy_refused_is_named_exactly_where_that_number_can_mean_it() {
    let error = mixengine_supervisor::Error::Spawn {
        program: "caddy".to_owned(),
        source: io::Error::from_raw_os_error(mixengine_platform::APPLICATION_CONTROL_BLOCKED),
    }
    .to_wire();

    assert_eq!(error.code, ErrorCode::ProcessFailed);

    let named = error
        .hint
        .as_deref()
        .is_some_and(|hint| hint.contains("caddy") && hint.contains("application control"));

    assert_eq!(named, cfg!(windows), "{:?}", error.hint);
}

/// T194, D3: a Linux release with no keyring is told the one command MixEngine offers, and
/// nothing switches on its own.
#[test]
fn a_linux_release_with_no_keyring_is_told_about_the_file_store() {
    let hint = keyring_hint(true, "linux").expect("a hint");
    assert!(hint.contains("mix daemon credential-store home"), "{hint}");
    assert_eq!(keyring_hint(true, "windows"), None);
    assert_eq!(keyring_hint(true, "macos"), None);
    assert_eq!(keyring_hint(false, "linux"), None);
}
