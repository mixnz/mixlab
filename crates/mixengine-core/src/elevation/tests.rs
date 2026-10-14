use super::*;
use crate::ElevationError;

/// A store on a temporary file, migrated, with nothing in this table.
async fn store() -> (tempfile::TempDir, Store) {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = Store::open(&home.path().join("mixengine.db"))
        .await
        .expect("a fresh database migrates");

    (home, store)
}

const WHEN: Timestamp = Timestamp(1_760_000_000_000);
const LATER: Timestamp = Timestamp(1_760_000_900_000);

#[tokio::test]
async fn the_first_enqueue_hands_back_the_whole_queue() {
    let (_home, store) = store().await;

    let waiting = enqueue(&store, &PrivilegedOp::Probe {}, WHEN)
        .await
        .expect("the row is written")
        .expect("a first enqueue changed something");

    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].op, PrivilegedOp::Probe {});
    assert_eq!(waiting[0].description, PrivilegedOp::Probe {}.describe());
    assert_eq!(waiting[0].requested_at, WHEN);
}

/// D2, and the whole of "no code path elevates in a loop" at rest: the schema is what refuses,
/// so a producer that enqueues on every start writes one row without having to know it.
#[tokio::test]
async fn the_same_operation_asked_for_twice_is_one_row_with_the_first_moment() {
    let (_home, store) = store().await;

    enqueue(&store, &PrivilegedOp::Probe {}, WHEN)
        .await
        .unwrap();
    let again = enqueue(&store, &PrivilegedOp::Probe {}, LATER)
        .await
        .expect("a conflicting insert is not an error");

    assert!(
        again.is_none(),
        "nothing changed, so there is nothing to announce"
    );

    let waiting = pending(&store).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        waiting[0].requested_at, WHEN,
        "the surviving row keeps the moment the machine first needed this"
    );
}

#[tokio::test]
async fn dropping_one_leaves_the_others_and_dropping_all_empties_it() {
    let (_home, store) = store().await;

    enqueue(&store, &PrivilegedOp::Probe {}, WHEN)
        .await
        .unwrap();
    let only = pending(&store).await.unwrap()[0].id;

    assert_eq!(discard(&store, Some(only)).await.unwrap(), 1);
    assert!(pending(&store).await.unwrap().is_empty());

    // Dropping something that is not there is not an error: the caller wanted it gone and it is.
    assert_eq!(discard(&store, Some(only)).await.unwrap(), 0);

    enqueue(&store, &PrivilegedOp::Probe {}, LATER)
        .await
        .unwrap();
    assert_eq!(discard(&store, None).await.unwrap(), 1);
    assert!(pending(&store).await.unwrap().is_empty());
}

/// D2's last paragraph. The only way to make one of these is to downgrade the daemon underneath
/// its own database, and a row no installed build can act on is a degraded mode nobody can ever
/// clear — so it is deleted and logged rather than carried.
#[tokio::test]
async fn a_row_this_build_cannot_decode_is_removed_rather_than_carried() {
    let (_home, store) = store().await;

    enqueue(&store, &PrivilegedOp::Probe {}, WHEN)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at)
             VALUES ('{\"op\":\"trust-ca-install\",\"der\":[1,2,3]}', 'future', 1)",
    )
    .execute(store.pool())
    .await
    .expect("a row from a build that knew more than this one");

    let waiting = pending(&store).await.unwrap();

    assert_eq!(waiting.len(), 1, "only the one this build can act on");
    assert_eq!(waiting[0].op, PrivilegedOp::Probe {});

    // And it is gone rather than merely hidden — a reader that filtered would find it again on
    // every call and log the same warning for the life of the home.
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_privileged_ops")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(left, 1);
}

/// D9: beside the program that went looking, and nowhere else. The refusal is what
/// `elevation.grant` turns into `dependency_missing`.
///
/// `None` for the installed copy — not because this machine has none, which it may well if
/// somebody has run the elevated suite here, but because that is the machine this test is
/// describing. See T85's D5 for why the argument exists at all.
#[test]
fn a_helper_that_is_not_beside_the_daemon_is_named_rather_than_searched_for() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let program = directory.path().join("mixengined");

    let error = helper(&program, None).expect_err("nothing was put there");

    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateMissing { .. })
        ),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains(&directory.path().display().to_string()),
        "the message has to say where it looked: {error}"
    );
}

/// T85's D5, as its own table, with T88d's source list in place of the single fallback. Every
/// row, including the two no machine running this test could produce: an installed helper
/// somebody else owns, and one the machine will not answer about.
fn ops(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// T182b, D3. The installed helper knows every operation in the batch: it runs it.
#[test]
fn an_installed_helper_that_knows_the_batch_is_not_bypassed() {
    let installed = ops(&["hosts-apply", "trust-ca-install"]);
    let bypass = Bypass {
        installed_ops: &installed,
        shipped_version: Some(mixengine_proto::privileged::HELPER_VERSION),
        batch_ops: &["hosts-apply"],
    };

    assert!(!bypass.applies());
}

/// T182b, D3. The installed helper cannot read an operation in the batch, and the shipped copy
/// is this release's: the shipped copy runs it.
#[test]
fn an_installed_helper_that_cannot_read_the_batch_gives_way_to_the_current_one() {
    let installed = ops(&["hosts-apply"]);
    let bypass = Bypass {
        installed_ops: &installed,
        shipped_version: Some(mixengine_proto::privileged::HELPER_VERSION),
        batch_ops: &["hosts-apply", "helper-remove"],
    };

    assert!(bypass.applies());
}

/// T182b, D2's last row. A helper too old to replace itself does know `helper-install`, but run
/// by it that copies its own image onto itself; the install goes through the shipped copy. A
/// helper that can replace itself is never bypassed for an install.
#[test]
fn an_install_over_a_helper_too_old_to_replace_itself_goes_through_the_shipped_copy() {
    let too_old = ops(&["hosts-apply", "helper-install"]);
    let bypass = Bypass {
        installed_ops: &too_old,
        shipped_version: Some(mixengine_proto::privileged::HELPER_VERSION),
        batch_ops: &["helper-install", "hosts-apply"],
    };
    assert!(bypass.applies());

    let replaces_itself = ops(&["hosts-apply", "helper-install", "helper-replace"]);
    let bypass = Bypass {
        installed_ops: &replaces_itself,
        ..bypass
    };
    assert!(!bypass.applies());
}

/// T182b, D3. A shipped copy that is not this release's, or that did not answer, is never the
/// way around: that would trade a helper that cannot do the work for one nobody vouched for.
#[test]
fn a_shipped_copy_that_is_not_current_is_never_the_bypass() {
    let installed = ops(&["hosts-apply"]);

    for shipped_version in [Some("0.0.7"), None] {
        let bypass = Bypass {
            installed_ops: &installed,
            shipped_version,
            batch_ops: &["helper-remove"],
        };

        assert!(!bypass.applies(), "{shipped_version:?}");
    }
}

#[test]
fn which_helper_is_run_and_when_nothing_is() {
    let installed = PathBuf::from("/system/mixengine-elevate");
    let bundle = PathBuf::from("/Applications/MixLab.app/Contents/Resources/mixengine-elevate");
    let beside = PathBuf::from("/app/mixengine-elevate");

    let both = || vec![(bundle.clone(), true), (beside.clone(), true)];
    let only_beside = || vec![(bundle.clone(), false), (beside.clone(), true)];
    let neither = || vec![(bundle.clone(), false), (beside.clone(), false)];
    let unread = || panic!("the sources must not be built when a helper is installed");

    // Nothing installed: a development tree, a machine before its first prompt, or one whose
    // helper an uninstall removed. The platform's order decides, and nothing re-sorts it.
    assert_eq!(
        choose(None, both).expect("the first source that exists"),
        bundle
    );

    // A source that is not there is skipped for one that is.
    assert_eq!(
        choose(None, only_beside).expect("the source that exists"),
        beside
    );

    // Installed and an administrator's: that one, and no source is consulted.
    assert_eq!(
        choose(Some((installed.clone(), Trust::Administrative)), unread)
            .expect("the installed copy"),
        installed
    );

    // Installed and writable by somebody else. **Not a fall-back**: the reason that file is
    // where it is, is that nothing running as the user should be able to arrange it.
    let error = choose(
        Some((
            installed.clone(),
            Trust::Writable("/system belongs to 501".to_owned()),
        )),
        unread,
    )
    .expect_err("a helper somebody else can rewrite is not run as root");
    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateUntrusted { .. })
        ),
        "{error}"
    );
    assert!(
        error.to_string().contains("/system belongs to 501"),
        "{error}"
    );

    // Installed and unreadable: the same answer, a different sentence.
    let error = choose(
        Some((installed, Trust::Unknown("permission denied".to_owned()))),
        unread,
    )
    .expect_err("a helper whose owner cannot be read is not run as root");
    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateUntrusted { .. })
        ),
        "{error}"
    );

    // Nothing anywhere, and the message names the platform's first entry — the place this
    // system's installer is supposed to have left one.
    let error = choose(None, neither).expect_err("there is no helper at all");
    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateMissing { .. })
        ),
        "{error}"
    );
    assert!(
        error.to_string().contains(&bundle.display().to_string()),
        "{error}"
    );
}

/// A system that offered no source at all still answers, rather than indexing an empty list. No
/// shipped system does this — every `helper_sources` ends with the copy beside the program —
/// and the row exists so that one which stopped doing it fails here rather than in a prompt.
#[test]
fn a_system_with_no_sources_at_all_is_still_a_refusal() {
    let error = choose(None, Vec::new).expect_err("there is nothing to install from");

    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateMissing { .. })
        ),
        "{error}"
    );
}

/// T88d. `helper` on a directory with nothing beside it and nothing installed keeps the shape
/// every caller already handles.
#[test]
fn nothing_installed_and_nothing_shipped_is_elevate_missing() {
    let empty = tempfile::tempdir().expect("a temporary directory");

    let error =
        helper(&empty.path().join("mixengined"), None).expect_err("there is no helper anywhere");

    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateMissing { .. })
        ),
        "{error}"
    );
}

/// D5's table, one row at a time. Four outcomes delete and exactly one is kept — the only one
/// whose own type in `mixengine-proto` says retrying is meaningful.
#[tokio::test]
async fn only_the_outcome_that_says_try_again_keeps_its_row() {
    use mixengine_proto::privileged::OpOutcome;

    for (outcome, survives) in [
        (
            OpOutcome::Applied {
                detail: "wrote two lines".to_owned(),
            },
            false,
        ),
        (OpOutcome::AlreadyDone, false),
        (
            OpOutcome::Refused {
                reason: "outside the home".to_owned(),
            },
            false,
        ),
        (
            OpOutcome::Unsupported {
                reason: "this helper is older".to_owned(),
            },
            false,
        ),
        (
            OpOutcome::Failed {
                message: "the file was locked".to_owned(),
            },
            true,
        ),
    ] {
        let (_home, store) = store().await;
        enqueue(&store, &PrivilegedOp::Probe {}, WHEN)
            .await
            .unwrap();
        let only = pending(&store).await.unwrap()[0].id;

        settle(&store, &[(only, outcome.clone())]).await.unwrap();

        assert_eq!(
            !pending(&store).await.unwrap().is_empty(),
            survives,
            "{outcome:?}"
        );
    }
}

/// What the counts are for: the job's result says how many operations came back done, which of
/// them will never succeed and why, and what is left in the queue afterwards. A client cannot
/// compute the third from the first two, because refused and failed part company.
#[tokio::test]
async fn a_settlement_counts_what_a_person_is_told_afterwards() {
    use mixengine_proto::privileged::OpOutcome;

    let (_home, store) = store().await;

    // Three distinct rows: `Probe` is the only operation this build has, so the other two are
    // written directly — which is also the shape T41 will produce.
    enqueue(&store, &PrivilegedOp::Probe {}, WHEN)
        .await
        .unwrap();
    for (key, at) in [("second", 2), ("third", 3)] {
        sqlx::query(
            "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
                 VALUES ('{\"op\":\"probe\"}', ?, ?)",
        )
        .bind(key)
        .bind(at)
        .execute(store.pool())
        .await
        .expect("a second and third row");
    }

    let waiting = pending(&store).await.unwrap();
    assert_eq!(waiting.len(), 3);

    let settled = settle(
        &store,
        &[
            (waiting[0].id, OpOutcome::AlreadyDone),
            (
                waiting[1].id,
                OpOutcome::Refused {
                    reason: "outside the home".to_owned(),
                },
            ),
            (
                waiting[2].id,
                OpOutcome::Failed {
                    message: "the file was locked".to_owned(),
                },
            ),
        ],
    )
    .await
    .unwrap();

    assert_eq!(settled.applied, 1);
    assert_eq!(settled.failed.len(), 1);
    assert_eq!(settled.refused.len(), 1);
    assert_eq!(settled.refused[0].0, waiting[1].id);
    assert!(settled.refused[0].1.contains("outside the home"));

    assert_eq!(pending(&store).await.unwrap().len(), 1);
}

/// A pending operation, without a database — everything below is about the document.
fn one_waiting(id: i64) -> PendingOp {
    let op = PrivilegedOp::Probe {};

    PendingOp {
        id: PendingOpId(id),
        description: op.describe(),
        op,
        requested_at: WHEN,
    }
}

#[test]
fn a_request_is_written_where_the_helper_will_look_for_it() {
    let home = tempfile::tempdir().expect("a temporary home");
    let directory = home.path().join("run").join("elevate").join("one");

    let request = write_request(
        &directory,
        home.path(),
        &[one_waiting(1), one_waiting(2)],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .expect("the document is written");

    assert_eq!(request.path(), directory.join("request.json"));
    assert_eq!(request.ids(), [PendingOpId(1), PendingOpId(2)]);
    assert!(!request.nonce().is_empty());

    let written: mixengine_proto::privileged::PrivilegedRequest =
        serde_json::from_slice(&std::fs::read(request.path()).unwrap()).unwrap();

    assert_eq!(written.version, mixengine_proto::PROTOCOL_VERSION);
    assert_eq!(written.home, home.path());
    assert_eq!(written.ops.len(), 2);
    assert_eq!(written.ops[0]["op"], "probe");
}

/// T88a. The protocol a request is marked with is the caller's decision, and this is what says
/// so: the daemon marks one at the lower of its own and the installed helper's, which a
/// function stamping its own constant would make unreachable.
#[test]
fn a_request_carries_the_protocol_its_writer_chose() {
    let home = tempfile::tempdir().expect("a temporary home");
    let directory = home.path().join("one");

    let request = write_request(
        &directory,
        home.path(),
        &[one_waiting(1)],
        mixengine_proto::PROTOCOL_MINIMUM,
    )
    .expect("the document is written");

    let written: mixengine_proto::privileged::PrivilegedRequest =
        serde_json::from_slice(&std::fs::read(request.path()).unwrap()).unwrap();

    assert_eq!(written.version, mixengine_proto::PROTOCOL_MINIMUM);
}

/// T88a, from the other end: a report from a helper older than this daemon is still a report,
/// which is the whole of what *"an old elevate keeps serving the operations it knows"* needs
/// from this side.
#[test]
fn a_report_from_a_helper_at_the_floor_is_read() {
    let home = tempfile::tempdir().expect("a temporary home");
    let directory = home.path().join("one");
    let request = write_request(
        &directory,
        home.path(),
        &[one_waiting(1)],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .unwrap();

    std::fs::write(
        directory.join(mixengine_proto::privileged::RESPONSE_FILE_NAME),
        serde_json::to_vec(&mixengine_proto::privileged::PrivilegedResponse {
            version: mixengine_proto::PROTOCOL_MINIMUM,
            elevate_version: "0.1.0".to_owned(),
            nonce: request.nonce().to_owned(),
            elevated: true,
            supported_ops: vec!["probe".to_owned()],
            audit_log: std::path::PathBuf::from("/var/log/mixengine/elevate.log"),
            results: vec![OpOutcome::AlreadyDone],
        })
        .unwrap(),
    )
    .unwrap();

    let report = read_report(&request).expect("a helper at the floor still answers");

    assert_eq!(report.version, mixengine_proto::PROTOCOL_MINIMUM);
}

/// Two grants must never write into one directory: `response.json`'s existence is the whole of
/// the anti-replay check, so a nonce that repeated would make the second request unanswerable.
#[test]
fn two_requests_never_share_a_nonce() {
    let home = tempfile::tempdir().expect("a temporary home");

    let first = write_request(
        &home.path().join("a"),
        home.path(),
        &[one_waiting(1)],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .unwrap();
    let second = write_request(
        &home.path().join("b"),
        home.path(),
        &[one_waiting(1)],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .unwrap();

    assert_ne!(first.nonce(), second.nonce());
}

/// The helper refuses an empty batch outright — no response file, exit 65 — so this is refused
/// here, where the message can say what actually happened.
#[test]
fn a_request_with_nothing_in_it_is_refused_before_it_is_written() {
    let home = tempfile::tempdir().expect("a temporary home");

    let error = write_request(
        &home.path().join("empty"),
        home.path(),
        &[],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .expect_err("an empty batch asks for nothing");

    assert!(
        matches!(error, Error::Elevation(ElevationError::ElevateRequestEmpty)),
        "{error}"
    );
}

/// T40a is explicit that `Completed` means the helper *ran*, not that it left a report — a crash
/// is not a per-OS event. So this is a state, not an impossibility, and it has its own error.
#[test]
fn a_request_with_no_report_beside_it_says_exactly_that() {
    let home = tempfile::tempdir().expect("a temporary home");
    let request = write_request(
        &home.path().join("one"),
        home.path(),
        &[one_waiting(1)],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .unwrap();

    let error = read_report(&request).expect_err("nothing was written beside it");

    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateReportMissing { .. })
        ),
        "{error}"
    );
}

/// T166: the message ends in the helper's reason when there is one, and is today's sentence when
/// there is not.
#[test]
fn a_missing_report_carries_what_the_helper_said() {
    let path = PathBuf::from("/home/run/elevate/x/response.json");

    let silent = Error::Elevation(ElevationError::ElevateReportMissing {
        path: path.clone(),
        said: None,
    });
    assert_eq!(
        silent.to_string(),
        "the elevation helper left no report beside /home/run/elevate/x/response.json"
    );

    let said = Error::Elevation(ElevationError::ElevateReportMissing {
        path,
        said: Some("mixengine-elevate: cannot read request.json: Operation not permitted".into()),
    });
    assert_eq!(
        said.to_string(),
        "the elevation helper left no report beside /home/run/elevate/x/response.json: \
             mixengine-elevate: cannot read request.json: Operation not permitted"
    );
}

#[test]
fn a_report_answering_another_request_is_refused() {
    let home = tempfile::tempdir().expect("a temporary home");
    let directory = home.path().join("one");
    let request = write_request(
        &directory,
        home.path(),
        &[one_waiting(1)],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .unwrap();

    std::fs::write(
        directory.join(mixengine_proto::privileged::RESPONSE_FILE_NAME),
        serde_json::to_vec(&mixengine_proto::privileged::PrivilegedResponse {
            version: mixengine_proto::PROTOCOL_VERSION,
            elevate_version: "0.1.0".to_owned(),
            nonce: "somebody else's".to_owned(),
            elevated: true,
            supported_ops: vec!["probe".to_owned()],
            audit_log: std::path::PathBuf::from("/var/log/mixengine/elevate.log"),
            results: vec![OpOutcome::AlreadyDone],
        })
        .unwrap(),
    )
    .unwrap();

    let error = read_report(&request).expect_err("the nonce does not match");

    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateReportMismatched { .. })
        ),
        "{error}"
    );
}

/// One outcome per operation, at the same index, is what `settle` rests on — a short report would
/// otherwise silently leave the last row of the batch untouched.
#[test]
fn a_report_with_the_wrong_number_of_outcomes_is_refused() {
    let home = tempfile::tempdir().expect("a temporary home");
    let directory = home.path().join("one");
    let request = write_request(
        &directory,
        home.path(),
        &[one_waiting(1), one_waiting(2)],
        mixengine_proto::PROTOCOL_VERSION,
    )
    .unwrap();

    std::fs::write(
        directory.join(mixengine_proto::privileged::RESPONSE_FILE_NAME),
        serde_json::to_vec(&mixengine_proto::privileged::PrivilegedResponse {
            version: mixengine_proto::PROTOCOL_VERSION,
            elevate_version: "0.1.0".to_owned(),
            nonce: request.nonce().to_owned(),
            elevated: true,
            supported_ops: vec!["probe".to_owned()],
            audit_log: std::path::PathBuf::from("/var/log/mixengine/elevate.log"),
            results: vec![OpOutcome::AlreadyDone],
        })
        .unwrap(),
    )
    .unwrap();

    let error = read_report(&request).expect_err("two were sent and one came back");

    assert!(
        matches!(
            error,
            Error::Elevation(ElevationError::ElevateReportMismatched { .. })
        ),
        "{error}"
    );
}

/// D2: two sites created before anybody clicks Allow are **one** row, holding the second state.
///
/// Two rows would both be valid, would disagree, and would both be rendered on the one screen
/// whose whole job is to say what is about to happen.
#[tokio::test]
async fn a_newer_hosts_state_supersedes_the_one_that_was_waiting() {
    let (_directory, store) = store().await;

    let first = PrivilegedOp::hosts_apply([entry("blog.test")]);
    let second = PrivilegedOp::hosts_apply([entry("blog.test"), entry("shop.test")]);

    assert!(
        enqueue(&store, &first, Timestamp(1_000))
            .await
            .unwrap()
            .is_some()
    );

    let announced = enqueue(&store, &second, Timestamp(2_000))
        .await
        .unwrap()
        .expect("a different state is a change and is announced");

    assert_eq!(announced.len(), 1, "one row, not two: {announced:?}");
    assert_eq!(announced[0].op, second);
    assert_eq!(
        announced[0].requested_at,
        Timestamp(1_000),
        "the need started when it started; a queue that reset its own clock would report a \
             wait that never got older"
    );
}

/// The `WHERE` clause is what keeps `rows_affected` meaning what T40b's caller reads it as:
/// re-enqueueing the same desired state touches no row and announces nothing.
#[tokio::test]
async fn re_enqueueing_the_same_hosts_state_announces_nothing() {
    let (_directory, store) = store().await;
    let op = PrivilegedOp::hosts_apply([entry("blog.test")]);

    assert!(
        enqueue(&store, &op, Timestamp(1_000))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        enqueue(&store, &op, Timestamp(2_000))
            .await
            .unwrap()
            .is_none()
    );
}

/// `Probe` keeps the key it had, which is what makes this need no migration.
#[tokio::test]
async fn probes_key_is_unchanged_so_no_existing_row_moves() {
    let (_directory, store) = store().await;

    assert!(
        enqueue(&store, &PrivilegedOp::Probe {}, Timestamp(1))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        enqueue(&store, &PrivilegedOp::Probe {}, Timestamp(2))
            .await
            .unwrap()
            .is_none()
    );
}

fn entry(domain: &str) -> mixengine_proto::privileged::HostEntry {
    mixengine_proto::privileged::HostEntry {
        address: "127.0.0.1".parse().expect("a literal address"),
        domain: domain.to_owned(),
    }
}

/// **A want that is gone leaves the queue** — roadmap task **T179**. Keyed like the enqueue, so
/// one family goes and its neighbours stay.
#[tokio::test]
async fn withdrawing_takes_one_family_and_leaves_the_rest() {
    let (_home, store) = store().await;

    let hosts = PrivilegedOp::hosts_apply(vec![mixengine_proto::privileged::HostEntry {
        address: std::net::IpAddr::from([127, 0, 0, 1]),
        domain: "hd.local".to_owned(),
    }]);
    enqueue(&store, &hosts, WHEN).await.unwrap();
    enqueue(&store, &PrivilegedOp::Probe {}, WHEN)
        .await
        .unwrap();

    assert_eq!(withdraw(&store, &hosts.dedupe_key()).await.unwrap(), 1);

    let waiting = pending(&store).await.unwrap();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].op, PrivilegedOp::Probe {});
}

/// Forgetting what is not there is what the caller wanted, on `discard`'s rule.
#[tokio::test]
async fn withdrawing_what_is_not_there_is_not_an_error() {
    let (_home, store) = store().await;

    assert_eq!(withdraw(&store, "hosts-apply").await.unwrap(), 0);
}
