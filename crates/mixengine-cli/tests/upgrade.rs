//! A daemon started on a database an older build wrote.
//!
//! `crates/mixengine-core/tests/upgrade.rs` proves the schema migrates and the rows survive, inside
//! one process calling `Store::open`. What it cannot answer is whether the **product** starts on
//! the result: whether the daemon's readers cope with rows whose newer columns hold defaults, and
//! whether `mix` can list what was in the old file.
//!
//! One test and not a suite. Every schema claim is cheaper and clearer one layer down, which is
//! `docs/standards/testing.md`'s rule about which layer owns a behaviour; what only this can
//! prove is that the daemon starts and reads.

mod harness;

use harness::{Home, json, stdout};
use mixengine_testkit::upgrade::Fixture;

/// The oldest fixture, which is the longest upgrade a release performs. Until a release after v0.0.7
/// adds a migration it is also `Current`, so nothing migrates and no backup is taken — the core
/// suite owns *when* a backup appears; this one asks only that the product starts and reads.
#[test]
fn a_daemon_starts_on_a_database_an_older_build_wrote_and_mix_lists_what_was_in_it() {
    let home = Home::new();

    let fixture = Fixture::all()
        .into_iter()
        .next()
        .expect("a fixture — see the testkit's own suite");
    fixture.copy_into(&home.database_file());

    // The upgrade happens here: the daemon's first act is `Store::open`.
    let _daemon = home.start_daemon();

    let status = json(&home.mix(&["status", "--json"]));
    assert!(
        status["daemon"]["pid"].as_u64().is_some(),
        "the daemon did not come up on a migrated database: {status}\n{}",
        home.daemon_log()
    );

    let output = home.mix(&["site", "list"]);
    let listed = stdout(&output);
    assert!(
        listed.contains("blog.test"),
        "the site that was in the old database is gone: {listed}\n{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        home.daemon_log()
    );
}
