use super::*;

/// Every staging directory still sitting beside `directory`, by name.
///
/// Named rather than counted, so a failure says which render left what behind. There is no
/// longer one path to test for: a name belongs to a render and not to a service.
fn staging_left(directory: &std::path::Path) -> Vec<String> {
    let name = directory
        .file_name()
        .expect("a service directory has a name")
        .to_string_lossy()
        .into_owned();
    let prefix = format!(".{name}.staging");

    let Some(parent) = directory.parent() else {
        return Vec::new();
    };

    let mut left: Vec<String> = std::fs::read_dir(parent)
        .expect("the directory holding the service")
        .map(|entry| entry.expect("a readable entry").file_name())
        .map(|entry| entry.to_string_lossy().into_owned())
        .filter(|entry| entry.starts_with(&prefix))
        .collect();

    left.sort();
    left
}

fn documents() -> Vec<Document> {
    vec![
        Document::new("mixengine.conf", "port = 3306\n"),
        Document::new("conf.d/tuning.conf", "innodb_buffer_pool_size = 256M\n"),
    ]
}

/// **Two renders of one service overlap without corrupting each other.**
///
/// A Windows CI leg produced this twice, on two unrelated branches, as
/// `.caddy.staging\Caddyfile: The system cannot find the file specified` out of `caddy
/// validate`. Nothing serialises `Registry::graph`, which renders the whole home, and it is
/// reached from about fifteen places — every `service.*` and `site.*` handler, each spawned and
/// joined by nobody, plus the idle sweeper, the certificate renewal task and `mix doctor`. Two
/// of those overlapping is ordinary, not exotic.
///
/// **Staged so that the first render is still inside its validator when the second arrives**,
/// which is the window the defect lives in and the only one worth a test. The slow validator
/// holds for 600 ms; the second render starts 150 ms in and runs to completion, and what it used
/// to do on the way out was delete the one directory the first was about to commit from.
///
/// The two renders write *different* content on purpose. Identical content is what made this so
/// hard to see: the second render refills the staging directory with the same bytes, so the
/// first commits successfully and nothing is ever wrong except in the microseconds between the
/// delete and the rewrite — which is exactly how often CI caught it.
#[tokio::test]
async fn a_render_that_overlaps_another_is_not_corrupted_by_it() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("caddy");

    let slow = Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf").args([
        "--touch",
        CONFIG,
        "--exit-after",
        "600",
        "--exit-code",
        "0",
    ]);
    let quick = Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf").args([
        "--touch",
        CONFIG,
        "--exit-code",
        "0",
    ]);

    let second = vec![
        Document::new(
            "mixengine.conf",
            "port = 5432
",
        ),
        Document::new(
            "conf.d/tuning.conf",
            "innodb_buffer_pool_size = 512M
",
        ),
    ];

    let first_documents = documents();

    let (first, overlapping) = tokio::join!(
        install(&directory, &first_documents, &[], Some(&slow)),
        async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            install(&directory, &second, &[], Some(&quick)).await
        }
    );

    overlapping.expect("the render that arrived second");
    first.expect("the render that was already staging when the second arrived");
}

#[tokio::test]
async fn a_first_render_creates_every_file_including_the_directories_under_it() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");

    let written = install(&directory, &documents(), &[], None)
        .await
        .expect("a first render")
        .written;

    assert_eq!(written, [Written::Created, Written::Created]);
    assert_eq!(
        std::fs::read_to_string(directory.join("conf.d/tuning.conf")).expect("the nested file"),
        "innodb_buffer_pool_size = 256M\n"
    );
}

/// The rule a reload hangs off: rendering the same state twice must not look like a change.
#[tokio::test]
async fn rendering_the_same_thing_again_writes_nothing() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");

    install(&directory, &documents(), &[], None)
        .await
        .expect("a first render");

    let file = directory.join("mixengine.conf");
    let before = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .expect("a modification time");

    let written = install(&directory, &documents(), &[], None)
        .await
        .expect("a second render")
        .written;

    assert_eq!(written, [Written::Unchanged, Written::Unchanged]);

    let after = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .expect("a modification time");
    assert_eq!(before, after, "an unchanged file was rewritten");
}

#[tokio::test]
async fn a_changed_file_is_updated_and_its_neighbour_is_left_alone() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");

    install(&directory, &documents(), &[], None)
        .await
        .expect("a first render");

    let mut second = documents();
    second[0] = Document::new("mixengine.conf", "port = 3307\n");

    let written = install(&directory, &second, &[], None)
        .await
        .expect("a second render")
        .written;

    assert_eq!(written, [Written::Updated, Written::Unchanged]);
    assert_eq!(
        std::fs::read_to_string(directory.join("mixengine.conf")).expect("the file"),
        "port = 3307\n"
    );
}

/// Nothing installed, and the previous configuration still in place — which is the whole
/// promise the staging directory exists to keep.
#[tokio::test]
async fn a_refused_configuration_leaves_the_last_good_one_live() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");

    install(&directory, &documents(), &[], None)
        .await
        .expect("a first render");

    let refuse = Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf")
        .args(["--touch", CONFIG, "--exit-code", "1"]);

    let mut second = documents();
    second[0] = Document::new("mixengine.conf", "port = nonsense\n");

    let error = install(&directory, &second, &[], Some(&refuse))
        .await
        .expect_err("a configuration the checker refuses");

    assert!(error.to_string().contains("mixengine.conf"), "{error}");
    assert_eq!(
        std::fs::read_to_string(directory.join("mixengine.conf")).expect("the file"),
        "port = 3306\n",
        "the refused rendering was installed anyway"
    );
    assert_eq!(
        staging_left(&directory),
        Vec::<String>::new(),
        "the staging directory outlived the failure"
    );
}

/// **A judgement installs nothing** — roadmap task **T81c**, the design's D5. It is what lets an
/// extension whose fragment the front end refuses be refused before the home changes, so the
/// thing it must not do is change the home.
#[tokio::test]
async fn judging_writes_nothing_and_leaves_nothing_behind() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("etc").join("caddy");

    let seen = home.path().join("seen");
    let accept = Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf")
        .args(["--touch", seen.to_string_lossy().as_ref()]);

    judge(&directory, &documents(), Some(&accept))
        .await
        .expect("a set the checker accepts");

    assert!(seen.exists(), "the checker never ran");
    assert!(
        !directory.exists(),
        "judging created the directory it judged for"
    );
    assert_eq!(
        staging_left(&directory),
        Vec::<String>::new(),
        "a judgement left its staging directory behind"
    );
}

/// And a refusal is the validator's own words, with the same nothing left behind.
#[tokio::test]
async fn a_judgement_the_checker_refuses_is_reported_and_still_installs_nothing() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("etc").join("caddy");

    let refuse = Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf")
        .args([
            "--complain",
            "unrecognized directive: notadirective",
            "--exit-code",
            "1",
        ])
        .reason(Reason::First);

    let error = judge(&directory, &documents(), Some(&refuse))
        .await
        .expect_err("a set the checker refuses");

    assert!(
        error.to_string().contains("unrecognized directive"),
        "{error}"
    );
    assert!(
        !directory.exists(),
        "a refused judgement created a directory"
    );
    assert_eq!(
        staging_left(&directory),
        Vec::<String>::new(),
        "a refused judgement left its staging directory behind"
    );
}

/// **Which line of a refusal a person is shown, when the program puts the reason first.**
///
/// `nginx -t` opens with `nginx: [emerg] ...` and closes with a line that names the file and
/// says the test failed. Reporting the last line of that is a message telling somebody their
/// configuration is wrong and nothing whatsoever about why.
#[tokio::test]
async fn a_refusal_is_reported_by_its_reason_and_not_by_its_summary() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("nginx");

    let refuse = Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf")
        .args([
            "--complain",
            "the reason nobody could guess",
            "--complain",
            "mixengine.conf test failed",
            "--exit-code",
            "1",
        ])
        .reason(Reason::First);

    let error = install(&directory, &documents(), &[], Some(&refuse))
        .await
        .expect_err("a configuration the checker refuses");

    assert!(
        error.to_string().contains("the reason nobody could guess"),
        "the summary was reported and the reason above it was dropped: {error}"
    );
}

/// And the other way round for every program that ends with the reason, which is most of them.
#[tokio::test]
async fn a_refusal_that_ends_with_its_reason_is_reported_by_its_last_line() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("caddy");

    let refuse =
        Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf").args([
            "--complain",
            "loading the configuration",
            "--complain",
            "the reason nobody could guess",
            "--exit-code",
            "1",
        ]);

    let error = install(&directory, &documents(), &[], Some(&refuse))
        .await
        .expect_err("a configuration the checker refuses");

    assert!(
        error.to_string().contains("the reason nobody could guess"),
        "the banner above the reason was reported instead of the reason: {error}"
    );
}

/// The validator sees the *staged* set, complete, and not the directory it is going into.
#[tokio::test]
async fn the_checker_is_shown_the_whole_rendering_before_any_of_it_is_installed() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");
    let seen = home.path().join("seen");

    // `--touch` writes the file it is given, so the path it was handed is provable rather than
    // asserted: what lands at `seen` says the placeholder was substituted.
    let accept = Validator::new(mixengine_testkit::FakeService::program(), "mixengine.conf")
        .args(["--touch", seen.to_string_lossy().as_ref()]);

    install(&directory, &documents(), &[], Some(&accept))
        .await
        .expect("a rendering the checker accepts");

    assert!(seen.exists(), "the checker never ran");
    assert!(directory.join("mixengine.conf").exists());
}

/// D4's first half: a file in a swept directory that no document owns is gone after a render.
#[tokio::test]
async fn a_swept_directory_holds_exactly_what_was_rendered_into_it() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("caddy");

    std::fs::create_dir_all(directory.join("sites")).expect("a sites directory");
    std::fs::write(directory.join("sites").join("gone.caddy"), "old").expect("a stale site");

    let documents = vec![
        Document::new(
            "Caddyfile",
            "import sites/*.caddy
",
        ),
        Document::new(
            "sites/blog.test.caddy",
            "http://blog.test {
}
",
        ),
    ];

    let installed = install(&directory, &documents, &["sites"], None)
        .await
        .expect("the set installs");

    assert!(
        !directory.join("sites").join("gone.caddy").exists(),
        "a site nothing declares any more is still being served"
    );
    assert!(directory.join("sites").join("blog.test.caddy").exists());
    assert_eq!(installed.removed.len(), 1);
}

/// D4's second half, and the one that matters to a running server: a walk whose *only*
/// difference is a removal still counts as a change, or `mix site delete` leaves the old site
/// being served until something else happens to move.
#[tokio::test]
async fn a_removal_on_its_own_is_a_change() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("caddy");

    let documents = vec![Document::new(
        "Caddyfile",
        "import sites/*.caddy
",
    )];

    let first = install(&directory, &documents, &["sites"], None)
        .await
        .expect("a first render");
    assert!(first.changed());

    std::fs::create_dir_all(directory.join("sites")).expect("a sites directory");
    std::fs::write(directory.join("sites").join("gone.caddy"), "old").expect("a stale site");

    let second = install(&directory, &documents, &["sites"], None)
        .await
        .expect("a second render");

    assert!(
        second.changed(),
        "nothing was reloaded, so the removed site went on being served"
    );
    assert!(
        second.written.iter().all(|one| !one.changed()),
        "the Caddyfile itself did not move; only the sweep did"
    );
}

/// Nothing outside a swept directory is touched, which is what keeps `etc/<service-id>/` a
/// service's own and a deleted service `service.delete`'s problem rather than this one's.
#[tokio::test]
async fn a_file_outside_a_swept_directory_is_left_alone() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("caddy");

    std::fs::create_dir_all(&directory).expect("the configuration directory");
    std::fs::write(directory.join("notes.txt"), "somebody's").expect("a file nobody renders");

    install(
        &directory,
        &[Document::new(
            "Caddyfile",
            "import sites/*.caddy
",
        )],
        &["sites"],
        None,
    )
    .await
    .expect("the set installs");

    assert!(directory.join("notes.txt").exists());
}

/// A front end with no sites at all: the directory is not there, and a sweep over one that is
/// not there is not an error — it is the first render.
#[tokio::test]
async fn sweeping_a_directory_that_is_not_there_is_quiet() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("caddy");

    let installed = install(
        &directory,
        &[Document::new(
            "Caddyfile",
            "import sites/*.caddy
",
        )],
        &["sites"],
        None,
    )
    .await
    .expect("the set installs");

    assert!(installed.removed.is_empty());
}
/// Asking what would change must not change anything — the whole reason this is separate from
/// `install`. A directory nobody has installed into yet is entirely drift, and asking twice
/// gives the same answer because the first ask wrote nothing.
#[tokio::test]
async fn asking_what_would_change_changes_nothing() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");

    let first = drift(&directory, &documents(), &[]).await.expect("a drift");

    assert_eq!(
        first.changed,
        vec![
            PathBuf::from("mixengine.conf"),
            PathBuf::from("conf.d/tuning.conf"),
        ]
    );
    assert!(first.removable.is_empty());
    assert!(!first.is_empty());

    assert!(
        !directory.exists(),
        "asking what would change created the directory it was asked about"
    );

    let again = drift(&directory, &documents(), &[])
        .await
        .expect("a second drift");

    assert_eq!(
        again.changed, first.changed,
        "the first ask was not a read: the second saw a different disk"
    );
}

/// After an install there is no drift, which is the property `mix doctor`'s check reads.
#[tokio::test]
async fn a_directory_that_was_just_installed_has_no_drift() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");

    install(&directory, &documents(), &[], None)
        .await
        .expect("a first render");

    let after = drift(&directory, &documents(), &[]).await.expect("a drift");

    assert!(after.is_empty(), "{after:?}");
}

/// A file whose contents moved is drift even though every file is present.
#[tokio::test]
async fn a_file_somebody_edited_by_hand_is_drift() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("mariadb@main");

    install(&directory, &documents(), &[], None)
        .await
        .expect("a first render");

    std::fs::write(directory.join("mixengine.conf"), "port = 9999\n")
        .expect("the generated file is writable");

    let after = drift(&directory, &documents(), &[]).await.expect("a drift");

    assert_eq!(after.changed, vec![PathBuf::from("mixengine.conf")]);
    assert!(after.removable.is_empty());
}

/// A file the recipe no longer renders is drift even though every rendered file matches — the
/// half a per-document comparison cannot see.
#[tokio::test]
async fn a_file_the_recipe_stopped_rendering_is_drift() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let directory = home.path().join("front@main");

    let with = vec![
        Document::new("caddy.conf", "one\n"),
        Document::new("sites/blog.test.conf", "two\n"),
    ];

    install(&directory, &with, &["sites"], None)
        .await
        .expect("a first render");

    let without = vec![Document::new("caddy.conf", "one\n")];
    let after = drift(&directory, &without, &["sites"])
        .await
        .expect("a drift");

    assert!(after.changed.is_empty(), "{after:?}");
    assert_eq!(
        after.removable,
        vec![directory.join("sites").join("blog.test.conf")],
        "a site that is no longer served was not noticed"
    );
}
