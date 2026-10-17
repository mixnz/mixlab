//! `mix uninstall` against a real daemon — roadmap task **T87**.
//!
//! **The unignored half of this file may never remove anything from the machine running it.** Every
//! test here runs on somebody's own workstation, so what is proved without `--ignored` is the plan,
//! the refusals and the `--keep-home` path. The round trip that actually takes MixEngine off a
//! machine is `#[ignore]`d and runs on a fresh runner in CI's `system` job, which is the clean VM
//! the task asks for.
//!
//! **And nothing here may assume the machine running it is clean.** A developer's machine has a
//! helper installed, an audit log, a `PATH` entry and a hosts block of its own home's; every
//! assertion below is therefore about *this* home's plan and about what the command did not do,
//! never about the machine being empty to begin with.

#[path = "../harness/mod.rs"]
mod harness;

use harness::{Home, json, stderr, stdout};

mod kept;
/// What this machine holds of MixEngine's, read with the machine's own tools.
///
/// **Its own module, and it knows nothing about `mixengine_platform`.** Every path here is written
/// out a second time on purpose: the whole value of this suite is that it can notice the code under
/// test looking in the wrong place, and it cannot do that if it asks that code where to look.
mod machine;
mod plan;
mod removal;

/// Is there anything on this machine an uninstall would need an administrator for?
///
/// **The gate on the two tests below, and it is about the machine rather than about MixEngine.** A
/// developer's workstation has a privileged helper and an audit log from its own earlier work, and
/// they belong to the machine rather than to the temporary home a test just made — so `--yes` there
/// would raise a real elevation dialog, wait on a person, and then take away something the rest of
/// that machine is using. A clean runner has none of it, which is where these two run in full.
///
/// Printed rather than silent: a test that skipped without saying so is a test that stops running
/// and nobody notices.
fn needs_an_administrator(home: &Home) -> bool {
    let plan = json(&home.mix(&["uninstall", "--dry-run", "--json"]));

    let waiting: Vec<String> = plan["items"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| row["outcome"]["removal"] == "planned")
        // T182d: this user's credential store needs no administrator either.
        .filter(|row| {
            !matches!(
                row["id"].as_str(),
                Some("home" | "relocated_directory" | "credentials" | "window_credentials")
            )
        })
        .map(|row| row["what"].as_str().unwrap_or_default().to_owned())
        .collect();

    if !waiting.is_empty() {
        println!(
            "skipped: this machine holds {} thing(s) an uninstall would need an administrator for, \
             and none of them belong to this test's home: {}",
            waiting.len(),
            waiting.join(", ")
        );
    }

    !waiting.is_empty()
}
