//! `mix service front-end` and `mix service set-front-end` against a daemon that is really running.
//!
//! **A fresh home has no front end**, because nothing installs one — `docs/features/services.md`
//! says so — and that is exactly the state worth proving these two commands in. What is asserted
//! here and nowhere else is that the pair reads the same fact the API answers with: the reading
//! comes off `service.list`'s `role` and never off a package name, and the switch refuses to install
//! anything to make itself possible.

mod harness;

use harness::Home;

/// A home with no front end says so, and says it as an answer rather than as a failure.
///
/// The `--json` half is the shape a client parses: `null`, which is the same *there is none* the
/// prose says, rather than an empty object a caller would have to interpret.
#[test]
fn mix_service_front_end_answers_a_home_that_has_none() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let output = home.mix(&["--json", "service", "front-end"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "null");

    let human = home.mix(&["service", "front-end"]);
    assert!(human.status.success());
    assert!(
        String::from_utf8_lossy(&human.stdout).contains("no front end"),
        "{}",
        String::from_utf8_lossy(&human.stdout)
    );
}

/// **A switch installs nothing.** A server that is not on this machine is a refusal naming the
/// command that would put it there, not a job that downloads it.
#[test]
fn mix_service_set_front_end_refuses_a_server_that_is_not_installed() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let output = home.mix(&["service", "set-front-end", "nginx", "--yes"]);

    assert!(!output.status.success(), "a switch to nothing is a failure");

    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("not installed") && said.contains("mix package install nginx"),
        "{said}"
    );
}

/// **`--json` never asks, and never acts instead of asking.**
///
/// `mix cleanup`'s rule: a machine-readable run that silently answered its own question would stop a
/// web server on behalf of a script that never said yes.
#[test]
fn mix_service_set_front_end_refuses_to_answer_its_own_question_under_json() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let output = home.mix(&["--json", "service", "set-front-end", "caddy"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--yes"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The two commands are one vocabulary: what `--help` offers is what the API takes.
#[test]
fn mix_service_set_front_end_offers_the_two_servers_and_nothing_else() {
    let home = Home::new();

    let output = home.mix(&["service", "set-front-end", "--help"]);
    let said = String::from_utf8_lossy(&output.stdout);

    assert!(said.contains("caddy"), "{said}");
    assert!(said.contains("nginx"), "{said}");

    let refused = home.mix(&["service", "set-front-end", "apache", "--yes"]);
    assert!(
        !refused.status.success(),
        "a third front end is a recipe, not a word a client may type"
    );
}
