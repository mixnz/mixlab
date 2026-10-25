//! `mix database client` and `mix database open` against a real daemon.
//!
//! A daemon out of `target/debug` has no MixLab beside it, so what answers here is the install with
//! no window. What the methods do when the window *is* there is proved on a mock host in
//! `crates/mixengine-daemon/src/databases.rs`, and once against a real MariaDB and a real
//! credential in `mariadb.rs`.

mod harness;

use harness::{Home, json, stderr, stdout};

/// A directory holding one `extension.toml`.
fn extension(body: &str) -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("mixengine-extension")
        .tempdir()
        .expect("a temporary directory");

    std::fs::write(directory.path().join("extension.toml"), body).expect("a manifest");

    directory
}

/// An install with no window is a state both commands print, and `open` says which installs have
/// one.
#[test]
fn with_no_window_both_commands_say_no_client() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    mixengine_testkit::declare::database_blocking(
        &home.database_file(),
        "redis@main",
        "redis",
        6379,
    );

    let report = json(&home.mix(&["database", "client", "redis@main", "--json"]));
    assert_eq!(report["protocol"], "redis", "{report}");
    assert_eq!(report["client"]["state"], "no_client", "{report}");

    let opened = home.mix(&["database", "open", "redis@main"]);
    assert_eq!(opened.status.code(), Some(1), "{}", stderr(&opened));
    let said = stdout(&opened);
    assert!(said.contains("has no MixLab window"), "{said}");
    assert!(!said.contains("mix extension install"), "{said}");
}

/// A plan says where an extension is from, before anybody agrees to anything.
#[test]
fn a_plan_names_where_an_extension_is_from() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let directory = extension(mixengine_testkit::extension::MAILPIT);
    let path = directory.path().display().to_string();

    let plan = json(&home.mix(&["extension", "plan", "--path", &path, "--json"]));
    assert!(plan.get("client").is_none(), "{plan}");
    assert_eq!(plan["homepage"], "https://mailpit.axllent.org", "{plan}");
}

/// A service no client opens is a state to `client` and a refusal to `open`.
#[test]
fn a_service_no_client_opens_is_said_in_those_words() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    mixengine_testkit::declare::database_blocking(
        &home.database_file(),
        "memcached@main",
        "memcached",
        11211,
    );

    let report = json(&home.mix(&["database", "client", "memcached@main", "--json"]));
    assert!(report.get("protocol").is_none(), "{report}");

    let human = stdout(&home.mix(&["database", "client", "memcached@main"]));
    assert!(
        human.contains("not a database a desktop client opens"),
        "{human}"
    );

    let refused = home.mix(&["database", "open", "memcached@main"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("memcached@main"),
        "{}",
        stderr(&refused)
    );
}

/// **`--password` with no value and closed standard input refuses before anything is sent.**
/// `Home::mix` gives its child a closed stdin, which is exactly the cron job this refusal exists
/// for: nobody was there to answer, and the daemon is never asked.
#[test]
fn password_flag_with_no_value_and_closed_stdin_refuses() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    mixengine_testkit::declare::database_blocking(
        &home.database_file(),
        "mariadb@main",
        "mariadb",
        3306,
    );

    let refused = home.mix(&[
        "database",
        "create",
        "mariadb@main",
        "--name",
        "blog",
        "--password",
    ]);

    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains("nobody to ask"),
        "{}",
        stderr(&refused)
    );
}
