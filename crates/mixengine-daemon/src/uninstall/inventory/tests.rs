use super::*;
use mixengine_platform::occupants::{HeldItem, Holder};

fn pkg_placement() -> mixengine_core::updates::Placement {
    mixengine_core::updates::Placement::Installer {
        directory: "/usr/local/bin".into(),
        receipt: "dev.mixengine.cli".to_owned(),
    }
}

fn reading<'a>(
    os: &'a str,
    placement: &'a mixengine_core::updates::Placement,
    asked: bool,
    present: &[&str],
) -> PackageReading<'a> {
    PackageReading {
        os,
        placement,
        asked,
        present: present.iter().map(std::path::PathBuf::from).collect(),
        window: true,
    }
}

/// A `.pkg` copy whose removal was asked for plans the package, named by its receipt.
#[test]
fn a_pkg_copy_asked_for_plans_the_package_row() {
    let placement = pkg_placement();
    let row = package_row(&reading(
        "macos",
        &placement,
        true,
        &["/Applications/MixLab.app"],
    ))
    .expect("a row");

    assert_eq!(row.id, ResidueId::Package);
    assert!(matches!(row.outcome, Removal::Planned { .. }), "{row:?}");
    assert_eq!(row.location, "dev.mixengine.cli");
}

/// Not asked for, the program stays and the row says how to remove it too.
#[test]
fn a_pkg_copy_not_asked_for_keeps_the_row_naming_the_flag() {
    let placement = pkg_placement();
    let row = package_row(&reading(
        "macos",
        &placement,
        false,
        &["/usr/local/bin/mix"],
    ))
    .expect("a row");

    assert!(
        matches!(&row.outcome, Removal::Kept { because } if because.contains("mix uninstall --package")),
        "{row:?}"
    );
}

/// A second run after the package went finds nothing there, which is not a failure.
#[test]
fn a_package_row_with_nothing_there_is_absent() {
    let placement = pkg_placement();
    let row = package_row(&reading("macos", &placement, true, &[])).expect("a row");

    assert!(matches!(row.outcome, Removal::Absent {}), "{row:?}");
}

/// Where no `.pkg` placed the copy, the row is kept with the sentence that removes the program
/// there; a copy no installer placed has no row at all.
#[test]
fn package_is_kept_with_a_reason_where_no_pkg_placed_the_copy() {
    let deb = mixengine_core::updates::Placement::Installer {
        directory: "/usr/bin".into(),
        receipt: "dpkg:mixlab".to_owned(),
    };
    let row = package_row(&reading("linux", &deb, true, &[])).expect("a row");
    assert!(
        matches!(&row.outcome, Removal::Kept { because } if because.contains("sudo apt remove mixlab")),
        "{row:?}"
    );

    let rpm = mixengine_core::updates::Placement::Installer {
        directory: "/usr/bin".into(),
        receipt: "rpm:mixlab".to_owned(),
    };
    let row = package_row(&reading("linux", &rpm, true, &[])).expect("a row");
    assert!(
        matches!(&row.outcome, Removal::Kept { because } if because.contains("sudo dnf remove mixlab")),
        "{row:?}"
    );

    let swap = mixengine_core::updates::Placement::SelfUpdatable {
        directory: "C:\\MixLab".into(),
    };
    let row = package_row(&reading("windows", &swap, true, &[])).expect("a row");
    assert!(
        matches!(&row.outcome, Removal::Kept { because } if because.contains("Windows uninstaller")),
        "{row:?}"
    );

    let dev = mixengine_core::updates::Placement::SelfUpdatable {
        directory: "/work/target/debug".into(),
    };
    assert!(package_row(&reading("macos", &dev, true, &[])).is_none());
}

/// Found running M34: the headless package's row named a MixLab application it never carried.
#[test]
fn a_copy_with_no_window_does_not_name_one() {
    let headless = mixengine_core::updates::Placement::Installer {
        directory: "/usr/bin".into(),
        receipt: "dpkg:mixengine-headless".to_owned(),
    };

    let row = package_row(&PackageReading {
        window: false,
        ..reading("linux", &headless, false, &[])
    })
    .expect("a row");
    assert_eq!(
        row.what,
        "the program itself: its commands and the package receipt"
    );

    let row = package_row(&reading("linux", &headless, false, &[])).expect("a row");
    assert!(row.what.contains("MixLab application"), "{row:?}");
}

fn held(path: &str, movable: bool, holders: &[(u32, &str)]) -> HeldItem {
    HeldItem {
        path: std::path::PathBuf::from(path),
        movable,
        holders: holders
            .iter()
            .map(|(pid, name)| Holder {
                pid: *pid,
                name: (*name).to_owned(),
            })
            .collect(),
    }
}

/// A movable item is not a row at all — the rename moves it.
#[test]
fn a_movable_item_is_not_a_row() {
    let rows = stuck_rows(&[held(r"C:\home\bin", true, &[(1200, "Code.exe")])], &[]);
    assert!(rows.is_empty(), "{rows:?}");
}

/// A stuck item is a `Blocked` row naming the program, the path and what to do.
#[test]
fn a_stuck_item_is_a_blocked_row() {
    let rows = stuck_rows(&[held(r"C:\home\data", false, &[(7, "pwsh.exe")])], &[]);

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].id, ResidueId::InUse);
    assert_eq!(rows[0].what, "pwsh.exe (pid 7)");
    assert_eq!(rows[0].location, r"C:\home\data");
    let Removal::Blocked { by } = &rows[0].outcome else {
        panic!("{:?}", rows[0].outcome)
    };
    assert!(
        by.contains(r"has C:\home\data open, so it cannot be moved or deleted"),
        "{by}"
    );
    assert!(by.ends_with("close it and run the uninstall again"), "{by}");
}

/// One program holding many stuck things is one row with a count.
#[test]
fn many_stuck_items_of_one_process_are_one_row() {
    let rows = stuck_rows(
        &[
            held(r"C:\home\a", false, &[(9, "tool.exe")]),
            held(r"C:\home\b", false, &[(9, "tool.exe")]),
            held(r"C:\home\c", false, &[(9, "tool.exe")]),
        ],
        &[],
    );

    assert_eq!(rows.len(), 1, "{rows:?}");
    let Removal::Blocked { by } = &rows[0].outcome else {
        panic!("{:?}", rows[0].outcome)
    };
    assert!(by.contains("(and 2 more)"), "{by}");
    assert_eq!(rows[0].location, r"C:\home\a");
}

/// A process already a row for running from the home is not a second row.
#[test]
fn a_process_running_and_stuck_is_one_row() {
    let running = Residue {
        id: ResidueId::InUse,
        what: "php.exe (pid 42)".to_owned(),
        location: r"C:\home\runtimes\php.exe".to_owned(),
        outcome: Removal::Blocked {
            by: "running".to_owned(),
        },
    };

    let rows = stuck_rows(
        &[held(r"C:\home\data", false, &[(42, "php.exe")])],
        &[running],
    );

    assert!(rows.is_empty(), "{rows:?}");
}

fn found(daemon: &[&str], window: Option<&[&str]>) -> Found {
    Found {
        home: Some("0123456789ab".to_owned()),
        daemon: Ok(daemon.iter().map(|key| (*key).to_owned()).collect()),
        window: window.map(|keys| Ok(keys.iter().map(|key| (*key).to_owned()).collect())),
    }
}

/// Review focus 4: another home's keys are neither counted nor offered.
#[test]
fn only_this_homes_keys_make_the_row() {
    let rows = credential_rows(
        &found(
            &[
                "0123456789ab/mariadb@main/root",
                "ffffffffffff/mariadb@main/root",
                "mariadb@main/root",
            ],
            None,
        ),
        false,
    );

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].id, ResidueId::Credentials);
    assert!(rows[0].what.starts_with("1 password"), "{rows:?}");
    assert!(matches!(rows[0].outcome, Removal::Planned { .. }));
}

#[test]
fn nothing_stored_is_no_row() {
    assert!(credential_rows(&found(&["ffffffffffff/x/y"], Some(&[][..])), false).is_empty());
}

#[test]
fn a_kept_home_keeps_both_rows() {
    let rows = credential_rows(&found(&["0123456789ab/a/b"], Some(&["vault"][..])), true);

    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        [ResidueId::Credentials, ResidueId::WindowCredentials]
    );
    assert!(
        rows.iter()
            .all(|row| matches!(row.outcome, Removal::Kept { .. })),
        "{rows:?}"
    );
}

/// Review focus 1: a machine with no credential store has nothing in one.
#[test]
fn no_store_at_all_is_no_row() {
    let absent = || {
        Err(mixengine_platform::Error::UnsupportedPlatform {
            capability: "Keyring",
            reason: "none".to_owned(),
        })
    };
    let rows = credential_rows(
        &Found {
            home: Some("0123456789ab".to_owned()),
            daemon: absent(),
            window: Some(absent()),
        },
        false,
    );

    assert!(rows.is_empty(), "{rows:?}");
}

/// A store that is there and refused is a failure, never "nothing there".
#[test]
fn a_store_that_refused_is_a_failed_row() {
    let refused = Err(mixengine_platform::Error::Secret {
        action: "list",
        service: "mixengine".to_owned(),
        key: "*".to_owned(),
        source: "denied".into(),
    });
    let rows = credential_rows(
        &Found {
            home: Some("0123456789ab".to_owned()),
            daemon: refused,
            window: None,
        },
        false,
    );

    assert!(
        matches!(
            rows[..],
            [Residue {
                id: ResidueId::Credentials,
                outcome: Removal::Failed { .. },
                ..
            }]
        ),
        "{rows:?}"
    );
}

/// What the install added to itself is one planned row, listing the recorded names.
#[test]
fn what_the_install_added_to_itself_is_one_planned_row() {
    let directory = tempfile::tempdir().expect("a directory");
    let exe = std::env::consts::EXE_SUFFIX;
    std::fs::write(
        directory.path().join(format!("mixengine-trampoline{exe}")),
        b"x",
    )
    .expect("a file");

    let row =
        completed_row(Some(directory.path()), &["mixengine-trampoline".to_owned()]).expect("a row");

    assert_eq!(row.id, ResidueId::CompletedBinary);
    assert!(row.what.contains("mixengine-trampoline"), "{row:?}");
    assert!(matches!(row.outcome, Removal::Planned { .. }), "{row:?}");
}

/// No row for a copy a package manager placed (ADR 0048), and none for a file already gone.
#[test]
fn no_row_without_a_writable_install_or_without_a_file() {
    let directory = tempfile::tempdir().expect("a directory");
    let recorded = ["mixengine-trampoline".to_owned()];

    assert!(completed_row(None, &recorded).is_none());
    assert!(completed_row(Some(directory.path()), &recorded).is_none());
}

fn query(keep_home: bool, keep_relocated: bool) -> mixengine_proto::UninstallQuery {
    mixengine_proto::UninstallQuery {
        keep_home,
        keep_relocated,
        grant: false,
        skip_holders: false,
        package: false,
        by_hand: false,
        over_ssh: false,
    }
}

/// The home answers `keep_home` and a relocated directory answers `keep_relocated`,
/// in all four combinations.
#[test]
fn the_home_and_the_relocated_directories_are_kept_separately() {
    let place = tempfile::tempdir().expect("tempdir");
    let root = place.path().join("MixEngine");
    let moved = vec![place.path().join("logs")];

    for (keep_home, keep_relocated) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let rows = directory_rows(&root, &moved, &query(keep_home, keep_relocated));
        let kept = |id| {
            rows.iter()
                .find(|row| row.id == id)
                .map(|row| matches!(row.outcome, Removal::Kept { .. }))
                .expect("a row")
        };

        assert_eq!(kept(ResidueId::Home), keep_home, "{rows:?}");
        assert_eq!(
            kept(ResidueId::RelocatedDirectory),
            keep_relocated,
            "{rows:?}"
        );

        let going = going(&root, &moved, &query(keep_home, keep_relocated));
        assert_eq!(going.contains(&root), !keep_home);
        assert_eq!(going.contains(&moved[0]), !keep_relocated);
    }
}

/// A tombstone an earlier run could not delete is planned for removal, whatever is kept, under the
/// id of the directory it was.
#[test]
fn an_old_tombstone_is_planned_for_removal_whatever_is_kept() {
    let place = tempfile::tempdir().expect("tempdir");
    let root = place.path().join("MixEngine");
    let old = place.path().join("MixEngine.removing-1");
    std::fs::create_dir_all(&old).expect("a tombstone");

    let rows = directory_rows(&root, &[], &query(true, true));

    let row = rows
        .iter()
        .find(|row| row.location == old.display().to_string())
        .expect("the tombstone is a row");
    assert_eq!(row.id, ResidueId::Home);
    assert!(matches!(row.outcome, Removal::Planned { .. }), "{row:?}");
}

/// `keep_home` is a row's answer and not a missing row: a person reading the plan has to see
/// that the home was considered and deliberately left.
#[test]
fn keeping_the_home_says_so_on_the_home_row() {
    let kept = home(Path::new("/tmp/home"), true);

    assert_eq!(kept.id, ResidueId::Home);
    assert!(matches!(kept.outcome, Removal::Kept { .. }), "{kept:?}");
    assert_eq!(kept.location, "/tmp/home");
}

/// And removing it names what goes with it. The databases in `data/` are the thing a person
/// most needs to have been told about before they answer the question.
#[test]
fn removing_the_home_says_what_goes_with_it() {
    let planned = home(Path::new("/tmp/home"), false);

    let Removal::Planned { how } = &planned.outcome else {
        panic!("{planned:?}");
    };

    assert!(how.contains("data/"), "{how}");
}

/// Every mechanism has somewhere to point a person, including the two that mean "this machine
/// has none of that": a row whose location is empty is a row nobody can act on.
#[test]
fn every_mechanism_names_a_place_to_look() {
    for method in [
        mixengine_platform::ResolverMethod::ResolverDirectory,
        mixengine_platform::ResolverMethod::SystemdLink,
        mixengine_platform::ResolverMethod::Nrpt,
        mixengine_platform::ResolverMethod::None,
    ] {
        assert!(!resolver_place(method).is_empty());
    }

    for method in [
        mixengine_platform::TrustStoreMethod::SystemRoot,
        mixengine_platform::TrustStoreMethod::SystemKeychain,
        mixengine_platform::TrustStoreMethod::CaCertificates,
        mixengine_platform::TrustStoreMethod::CaTrustAnchors,
        mixengine_platform::TrustStoreMethod::None,
    ] {
        assert!(!trust_place(method).is_empty());
    }
}

/// The label is composed once. A rule written under one name and looked for under another is a
/// rule nobody ever removes.
#[test]
fn the_firewall_label_is_the_one_sharing_writes() {
    assert_eq!(firewall_label(), format!("{FIREWALL_LABEL}shared sites"));
}

/// A dangling symlink is something, not nothing: `exists` answers `false` for one, and a plan
/// that skipped it would leave a link named after MixEngine's helper on the machine.
#[cfg(unix)]
#[test]
fn a_dangling_link_is_still_something_that_is_there() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let link = directory.path().join("helper");
    std::os::unix::fs::symlink(directory.path().join("nowhere"), &link).expect("the link");

    assert!(!link.exists());
    assert!(there(&link));
}

/// **A helper a package placed is kept, and the row names the package.**
#[test]
fn a_helper_a_package_placed_is_kept_and_names_the_package() {
    let row = helper_row(
        Some(Path::new("/usr/local/libexec/mixengine/mixengine-elevate")),
        true,
        Some("mixengine".to_owned()),
    );

    assert_eq!(row.id, ResidueId::PrivilegedHelper);
    assert_eq!(
        row.outcome,
        Removal::Kept {
            because: "it came with the mixengine package, and removing that package removes it"
                .to_owned(),
        }
    );
}

/// A helper no package claims is still removed, and one that is not there is not, whoever
/// would have owned it.
#[test]
fn a_helper_no_package_claims_is_planned_and_an_absent_one_is_absent() {
    let path = Path::new("/usr/local/libexec/mixengine/mixengine-elevate");

    assert!(matches!(
        helper_row(Some(path), true, None).outcome,
        Removal::Planned { .. }
    ));
    assert_eq!(
        helper_row(Some(path), false, Some("mixengine".to_owned())).outcome,
        Removal::Absent {}
    );
    assert_eq!(helper_row(None, false, None).outcome, Removal::Absent {});
}

/// The window's folders are this home's only when it is the one the installed window uses, what a
/// person made follows the home, and the cache goes whatever is kept.
#[test]
fn the_windows_folders_follow_the_home_and_the_cache_always_goes() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let data = root.path().join("Roaming").join("io.github.mixnz.mixlab");
    let cache = root.path().join("Local").join("io.github.mixnz.mixlab");
    let missing = root.path().join("Caches").join("io.github.mixnz.mixlab");
    std::fs::create_dir_all(&data).expect("the data folder");
    std::fs::create_dir_all(&cache).expect("the cache folder");

    let found = mixengine_platform::window_data::WindowData {
        data: vec![data.clone()],
        cache: vec![cache.clone(), missing],
    };

    let removed = window_rows(true, Some(&found), false);
    assert_eq!(
        removed
            .iter()
            .map(|row| (
                row.id,
                row.location.clone(),
                matches!(row.outcome, Removal::Planned { .. })
            ))
            .collect::<Vec<_>>(),
        vec![
            (ResidueId::WindowData, data.display().to_string(), true),
            (ResidueId::WindowCache, cache.display().to_string(), true),
        ],
        "{removed:?}"
    );

    let kept = window_rows(true, Some(&found), true);
    assert!(matches!(kept[0].outcome, Removal::Kept { .. }), "{kept:?}");
    assert!(
        matches!(kept[1].outcome, Removal::Planned { .. }),
        "{kept:?}"
    );

    assert!(
        window_rows(false, Some(&found), false).is_empty(),
        "a home the installed window does not use took the window's folders"
    );
}

/// The ledger of homes, as a test writes it.
fn homes(paths: &[&str]) -> Result<Option<mixengine_proto::privileged::HomesLedger>, String> {
    Ok(Some(mixengine_proto::privileged::HomesLedger {
        homes: paths
            .iter()
            .map(|home| mixengine_proto::privileged::LedgerEntry {
                home: std::path::PathBuf::from(home),
                caller: "uid 501".to_owned(),
                last: 0,
            })
            .collect(),
    }))
}

/// ADR 0066: the daemon plans what the helper will decide — another living home keeps the helper
/// and the log, and so does a machine with no ledger or one that cannot be read.
#[test]
fn the_helper_and_log_stay_while_another_home_uses_them() {
    let root = Path::new("/homes/a");

    let other = for_others(homes(&["/homes/a", "/homes/b"]), root, |_| true)
        .expect("another living home keeps them");
    assert!(
        other.starts_with(FOR_ANOTHER_HOME) && other.contains("/homes/b"),
        "{other}"
    );

    assert!(
        for_others(Ok(None), root, |_| true).is_some(),
        "no ledger keeps them"
    );
    assert!(
        for_others(Err("not JSON".to_owned()), root, |_| true)
            .is_some_and(|said| said.contains("not JSON")),
        "an unreadable ledger keeps them"
    );
    assert_eq!(
        for_others(homes(&["/homes/a", "/homes/gone"]), root, |home| home
            == root),
        None,
        "the last living home removes them"
    );
}

/// Only a row that would have gone is kept; one that is absent, or already kept for its package,
/// says what it said.
#[test]
fn keeping_for_others_turns_only_a_planned_row() {
    let planned = Residue {
        id: ResidueId::AuditLog,
        what: "the log".to_owned(),
        location: "/var/log/mixengine/elevate.log".to_owned(),
        outcome: Removal::Planned {
            how: "remove it".to_owned(),
        },
    };
    let absent = Residue {
        outcome: Removal::Absent {},
        ..planned.clone()
    };

    let kept = keep_for_others(planned, Some("kept for the other homes".to_owned()));
    assert!(matches!(kept.outcome, Removal::Kept { .. }), "{kept:?}");

    assert_eq!(
        keep_for_others(absent.clone(), Some("x".to_owned())),
        absent
    );
}

fn resolver_state(wired: &[&str], bypassed: bool) -> mixengine_platform::ResolverState {
    let tlds = |list: &[&str]| list.iter().map(|tld| (*tld).to_owned()).collect::<Vec<_>>();

    mixengine_platform::ResolverState {
        method: mixengine_platform::ResolverMethod::SystemdLink,
        wired: if bypassed { Vec::new() } else { tlds(wired) },
        missing: None,
        bypassed: bypassed
            .then(|| "programs on this machine do not ask systemd-resolved".to_owned()),
        declared: tlds(wired),
    }
}

/// Found running M34: a link an earlier grant wrote, on a machine whose programs never reach it,
/// is still this home's to remove.
#[test]
fn a_link_programs_never_reach_is_still_revoked() {
    let removal = machine::resolver_removal(&resolver_state(&["test"], true), 53_535);

    assert!(matches!(removal, Removal::Planned { .. }), "{removal:?}");
}

#[test]
fn a_machine_that_declares_nothing_has_nothing_to_revoke() {
    assert!(matches!(
        machine::resolver_removal(&resolver_state(&[], true), 53_535),
        Removal::Absent {}
    ));
    assert!(matches!(
        machine::resolver_removal(&resolver_state(&[], false), 53_535),
        Removal::Absent {}
    ));
}
