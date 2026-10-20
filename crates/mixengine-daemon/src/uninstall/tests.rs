use super::*;

fn row(id: ResidueId, location: &str, outcome: Removal) -> Residue {
    Residue {
        id,
        what: String::new(),
        location: location.to_owned(),
        outcome,
    }
}

/// **T182h.** A copy of the state goes into each relocated folder that is kept, and only there:
/// not a relocated folder that goes, and not the home.
#[test]
fn a_copy_goes_into_each_kept_relocated_folder_only() {
    let kept = || Removal::Kept {
        because: "asked".to_owned(),
    };
    let items = vec![
        row(ResidueId::RelocatedDirectory, "/disk/runtimes", kept()),
        row(
            ResidueId::RelocatedDirectory,
            "/disk/logs",
            Removal::Planned {
                how: "remove".to_owned(),
            },
        ),
        row(ResidueId::Home, "/home/me/.mixengine", kept()),
    ];

    assert_eq!(kept_relocated(&items), [0]);
}

#[test]
fn nothing_left_is_a_removal() {
    assert!(matches!(forgotten(3, Ok(0)), Removal::Removed { .. }));
}

#[test]
fn something_left_is_a_failure_naming_how_many() {
    let Removal::Failed { because } = forgotten(3, Ok(1)) else {
        panic!("not a failure")
    };
    assert!(because.contains('1'), "{because}");
}

#[test]
fn a_store_that_cannot_be_read_back_is_a_failure() {
    let refused = Err(mixengine_platform::Error::UnsupportedPlatform {
        capability: "Keyring",
        reason: "gone".to_owned(),
    });
    assert!(matches!(forgotten(3, refused), Removal::Failed { .. }));
}

/// Review focus 5: something outside the home failed, so the home stays and so do its
/// passwords.
#[test]
fn a_home_that_stays_keeps_its_passwords() {
    let mut items = vec![Residue {
        id: ResidueId::Credentials,
        what: "1 password".to_owned(),
        location: "mixengine".to_owned(),
        outcome: Removal::Planned { how: String::new() },
    }];

    keep_credentials(&mut items);

    assert!(
        matches!(items[0].outcome, Removal::Kept { .. }),
        "{items:?}"
    );
}

/// T185a: the files are removed, and what is reported is read back off the disk.
#[test]
fn removing_what_the_install_added_reads_the_disk_back() {
    let directory = tempfile::tempdir().expect("a directory");
    let exe = std::env::consts::EXE_SUFFIX;
    let file = directory.path().join(format!("mixengine-trampoline{exe}"));
    std::fs::write(&file, b"x").expect("a file");

    let outcome = remove_completed(directory.path(), &["mixengine-trampoline".to_owned()]);

    assert!(matches!(outcome, Removal::Removed { .. }), "{outcome:?}");
    assert!(!file.exists());
}

/// **The home is not the helper's business, and this is the assertion that says so.**
///
/// `settle` reads a row that is still `Planned` as one the helper was asked about and did not
/// manage — which is right for the seven it answers for and wrong for everything else. The home
/// is `Planned` for two statements longer than the fold, because `arm_the_home` rewrites it
/// afterwards; settled in between, every complete uninstall reported the home as waiting for a
/// prompt and then kept it for that reason. Found by CI on 2026-09-04.
#[test]
fn the_helper_answers_for_eight_rows_and_the_home_is_not_one_of_them() {
    for id in [
        ResidueId::HostsBlock,
        ResidueId::ResolverWiring,
        ResidueId::PortAccess,
        ResidueId::FirewallRules,
        ResidueId::TrustStore,
        ResidueId::Package,
        ResidueId::PrivilegedHelper,
        ResidueId::AuditLog,
    ] {
        assert!(needs_the_helper(id), "{id:?}");
    }

    for id in [
        // The four that need no token, answered where they are acted on.
        ResidueId::BrowserTrust,
        ResidueId::AutostartEntry,
        ResidueId::PathEntry,
        ResidueId::CompletedBinary,
        // And the four the daemon removes itself, answered by `arm_the_home`.
        ResidueId::Home,
        ResidueId::RelocatedDirectory,
        ResidueId::WindowData,
        ResidueId::WindowCache,
        // And the two credential rows, answered by `forget_credentials`.
        ResidueId::Credentials,
        ResidueId::WindowCredentials,
    ] {
        assert!(!needs_the_helper(id), "{id:?}");
    }

    assert_eq!(
        ResidueId::ALL
            .iter()
            .filter(|id| needs_the_helper(**id))
            .count(),
        7,
        "every id is on exactly one side of this, and a new one has to choose"
    );
}

/// What `settle` does to a row that is still there and was never granted — the behaviour that is
/// correct for a privileged row and catastrophic for the home.
#[test]
fn a_row_that_was_never_granted_is_still_waiting() {
    let settled = settle(planned(), planned(), false, &[]);

    assert!(
        matches!(settled.outcome, Removal::Enqueued { .. }),
        "{settled:?}"
    );
}

/// And one the machine no longer holds is a removal, whatever was attempted: the second reading
/// decides, not the helper's own account of itself (the T87 design, D3).
#[test]
fn a_row_the_machine_no_longer_holds_is_a_removal() {
    let mut gone = planned();
    gone.outcome = Removal::Absent {};

    let settled = settle(planned(), gone, true, &[]);

    assert!(
        matches!(settled.outcome, Removal::Removed { .. }),
        "{settled:?}"
    );
}

/// T182b, D7. A row that was absent before the act and is present after it is something the
/// act left on the machine — put back by a grant that carried more than it should, or by
/// anything else — and it is a failure, never the `Planned` the second reading spells it as.
#[test]
fn a_row_absent_before_and_present_after_is_a_failure() {
    let mut absent = planned();
    absent.outcome = Removal::Absent {};

    for granted in [true, false] {
        let settled = settle(absent.clone(), planned(), granted, &[]);

        assert!(
            matches!(settled.outcome, Removal::Failed { .. }),
            "{settled:?}"
        );
    }
}

/// T182a: a package row whose operation is still in the queue is waiting for permission, which
/// is a different sentence from one the helper applied and left behind.
#[test]
fn a_package_row_whose_removal_is_still_queued_says_so() {
    let package = Residue {
        id: ResidueId::Package,
        what: "the program itself".to_owned(),
        location: "dev.mixengine.cli".to_owned(),
        outcome: Removal::Planned {
            how: "remove it".to_owned(),
        },
    };
    let waiting = [mixengine_proto::PendingOp {
        id: mixengine_proto::PendingOpId(1),
        op: PrivilegedOp::PackageRemove {},
        description: String::new(),
        requested_at: mixengine_proto::Timestamp(0),
    }];

    let settled = settle(package.clone(), package, true, &waiting);

    assert!(
        matches!(&settled.outcome, Removal::Failed { because } if because.contains("waiting for permission")),
        "{settled:?}"
    );
}

/// One row, planned, for the two above to work on.
fn planned() -> Residue {
    Residue {
        id: ResidueId::PrivilegedHelper,
        what: "MixEngine's privileged helper".to_owned(),
        location: "somewhere only an administrator can write".to_owned(),
        outcome: Removal::Planned {
            how: "remove it".to_owned(),
        },
    }
}

/// **A kept helper asks the helper for nothing** — the T88e design, D3. Only a `Planned` row
/// becomes an operation, so the batch carries no `helper-remove`.
#[test]
fn a_kept_helper_is_not_a_row_to_act_on() {
    let kept = Residue {
        id: ResidueId::PrivilegedHelper,
        what: "MixEngine's privileged helper".to_owned(),
        location: "/usr/local/libexec/mixengine/mixengine-elevate".to_owned(),
        outcome: Removal::Kept {
            because: "it came with the mixengine package, and removing that package removes it"
                .to_owned(),
        },
    };

    assert!(!planned_row(&[kept], ResidueId::PrivilegedHelper));
}

fn autostart(enabled: bool, for_this_home: bool) -> mixengine_proto::AutostartReport {
    mixengine_proto::AutostartReport {
        mechanism: mixengine_proto::AutostartMechanism::SystemdUser,
        location: "/home/me/.config/systemd/user/mixengined.service".to_owned(),
        enabled,
        changed: false,
        command: Vec::new(),
        for_this_home,
        at_boot: mixengine_proto::AtBoot::Off,
    }
}

#[test]
fn a_linger_this_home_turned_on_is_residue_even_with_no_entry_left() {
    let Removal::Planned { how } = inventory::autostart_outcome(&autostart(false, false), true)
    else {
        panic!("a linger MixEngine turned on has to be planned for removal");
    };

    assert!(how.contains("linger"), "{how}");
}

#[test]
fn the_login_entry_is_residue_only_when_it_is_this_home_s() {
    assert!(matches!(
        inventory::autostart_outcome(&autostart(true, true), false),
        Removal::Planned { .. }
    ));
    assert!(matches!(
        inventory::autostart_outcome(&autostart(true, false), false),
        Removal::Absent {}
    ));
    assert!(matches!(
        inventory::autostart_outcome(&autostart(false, false), false),
        Removal::Absent {}
    ));
}
