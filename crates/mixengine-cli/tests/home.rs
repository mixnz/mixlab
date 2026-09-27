//! `mix home previous` and `mix home restore` — roadmap task **T182h**.
//!
//! What a restore brings back is proved against the daemon in `mixengine-daemon`'s own suite; this
//! is only that the command line reaches both methods and says the right thing when there is no
//! copy to restore.

mod harness;

use harness::{Home, json, stdout};

#[test]
fn previous_and_restore_reach_the_daemon_from_the_command_line() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let previous = json(&home.mix(&["home", "previous", "--json"]));
    assert!(
        previous.get("copy").is_none(),
        "a fresh home has no copy: {previous}"
    );

    let table = stdout(&home.mix(&["home", "previous"]));
    assert!(table.contains("no copy"), "{table}");

    let refused = home.mix(&["home", "restore", "--yes"]);
    assert!(!refused.status.success(), "{refused:?}");
    let said = format!(
        "{}{}",
        stdout(&refused),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(said.contains("mix home previous"), "{said}");
}
