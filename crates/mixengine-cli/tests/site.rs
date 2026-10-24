//! `mix site` against a real daemon.
//!
//! The client half of `site.*`. What the daemon's own `tests/sites.rs` proves is that the methods
//! do what they say; what is proved here is the part only true of `mix` — that the flags a person
//! types reach the right method, that a command typed inside a project finds its site without being
//! told, and that an export leaves a hand-written key where it was.

mod harness;

use harness::{Home, json, stdout};

/// A directory to register, and its manifest when it has one.
fn repository(body: Option<&str>) -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("mixengine-site")
        .tempdir()
        .expect("a temporary directory");

    if let Some(body) = body {
        std::fs::write(directory.path().join("mixengine.toml"), body).expect("a manifest");
    }

    directory
}

/// The verb a person actually says, and the flag it sets. `site update --state disabled` says the
/// same thing to the daemon; this is the sentence, and a client renders what comes back rather than
/// composing an update to express it.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_is_started_and_stopped_by_name_and_from_its_own_directory() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let repository = repository(None);
    let root = repository.path().display().to_string();

    home.mix(&["project", "create", &root, "--name", "blog"]);
    home.mix_in(
        repository.path(),
        &[],
        &[
            "site",
            "create",
            "--domain",
            "blog.test",
            "--kind",
            "static",
        ],
    );

    let stopped = json(&home.mix(&["site", "stop", "blog.test", "--json"]));
    assert_eq!(stopped["site"]["state"], "disabled", "{stopped}");

    // From inside the project, with no domain typed: the same default `mix site show` has.
    let started = json(&home.mix_in(repository.path(), &[], &["site", "start", "--json"]));
    assert_eq!(started["site"]["state"], "enabled", "{started}");
}

/// The sequence a person actually types, from inside the directory they are working in.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_is_created_from_the_current_directory_and_shown_from_a_subdirectory() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let repository = repository(None);
    let root = repository.path().display().to_string();
    std::fs::create_dir(repository.path().join("public")).expect("a doc root");

    let empty = stdout(&home.mix(&["site", "list"]));
    assert!(empty.contains("no sites"), "{empty}");

    home.mix(&["project", "create", &root, "--name", "blog"]);

    let created = json(&home.mix_in(
        repository.path(),
        &[],
        &[
            "site",
            "create",
            "--domain",
            "blog.test",
            "--doc-root",
            "public",
            "--kind",
            "static",
            "--json",
        ],
    ));
    assert_eq!(created["site"]["site"]["domain"], "blog.test", "{created}");

    let listed = stdout(&home.mix(&["site", "list"]));
    assert!(
        listed.contains("blog.test") && listed.contains("static"),
        "{listed}"
    );

    // From a subdirectory, with nothing named: which site this is is the daemon's answer.
    let inside = repository.path().join("public");
    let shown = json(&home.mix_in(&inside, &[], &["site", "show", "--json"]));
    assert_eq!(shown["site"]["domain"], "blog.test", "{shown}");

    let removed = stdout(&home.mix(&["site", "delete", "blog.test"]));
    assert!(removed.contains("free for another site"), "{removed}");
}

/// **The redirect, against the real daemon.** `--https-redirect` is accepted at `site create`
/// itself, not only at `site update` — a person does not have to create a site plaintext and turn
/// the redirect on afterwards, as long as `--https` is not explicitly refused in the same breath.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_is_created_with_its_redirect_already_on() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let repository = repository(None);
    let root = repository.path().display().to_string();

    home.mix(&["project", "create", &root, "--name", "blog"]);

    // `--https` is not typed at all: it defaults to `true`, which is what lets a bare
    // `--https-redirect true` succeed on a brand-new site.
    let created = json(&home.mix(&[
        "site",
        "create",
        "--project",
        "blog",
        "--domain",
        "blog.test",
        "--kind",
        "static",
        "--https-redirect",
        "true",
        "--json",
    ]));
    assert_eq!(created["site"]["site"]["https_redirect"], true, "{created}");

    let shown = json(&home.mix(&["site", "show", "blog.test", "--json"]));
    assert_eq!(shown["site"]["https_redirect"], true, "{shown}");

    // The refusal is also live at `create`, and not only at `update` — asked for in the same
    // request that would have left the site with nothing to redirect to.
    let refused = home.mix(&[
        "site",
        "create",
        "--project",
        "blog",
        "--domain",
        "shop.test",
        "--kind",
        "static",
        "--https",
        "false",
        "--https-redirect",
        "true",
    ]);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("https_redirect needs https enabled"),
        "{refused:?}"
    );
}

/// `mix project export` writes the site beside the runtimes, and leaves everything a person
/// put in the file exactly where it was.
#[tokio::test(flavor = "multi_thread")]
async fn an_export_writes_the_site_and_keeps_a_hand_written_key() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let repository = repository(Some("# the blog\n"));
    let root = repository.path().display().to_string();

    home.mix(&["project", "create", &root, "--name", "blog"]);
    home.mix(&[
        "site",
        "create",
        "--project",
        "blog",
        "--domain",
        "blog.test",
        "--kind",
        "static",
    ]);

    // Hand-written *after* the site exists, which is the order this actually happens in: a
    // `[[services]]` entry in the file at create time is a service the daemon looks up and refuses
    // when this machine has none, and what is proved here is what an export leaves alone.
    let manifest = repository.path().join("mixengine.toml");
    let hand_edited = std::fs::read_to_string(&manifest).expect("the manifest")
        + "\n[[services]]\nname = \"mariadb\"\ndatabase = \"blog\"\n";
    std::fs::write(&manifest, hand_edited).expect("a hand edit");

    home.mix(&["project", "export", "blog"]);

    let written = std::fs::read_to_string(repository.path().join("mixengine.toml"))
        .expect("the manifest that was written");

    assert!(written.contains("# the blog"), "{written}");
    assert!(written.contains("domain = \"blog.test\""), "{written}");
    assert!(written.contains("kind = \"static\""), "{written}");
    assert!(
        written.contains("database = \"blog\""),
        "an export is a merge, not a mirror: {written}"
    );
}

/// A refusal from the daemon is printed and exits non-zero, which is what a script branches on.
#[tokio::test(flavor = "multi_thread")]
async fn a_domain_on_a_public_tld_exits_non_zero_and_offers_test() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let repository = repository(None);
    let root = repository.path().display().to_string();

    home.mix(&["project", "create", &root, "--name", "blog"]);

    let output = home.mix(&[
        "site",
        "create",
        "--project",
        "blog",
        "--domain",
        "blog.dev",
        "--kind",
        "static",
    ]);

    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("test"),
        "{output:?}"
    );
}
