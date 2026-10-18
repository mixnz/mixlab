//! The plan: dry runs, listings and what blocks an uninstall.

use super::*;

/// The plan names every row, whatever this machine holds, and changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_names_every_row_and_changes_nothing() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let before = home.contents();

    // **Read before the plan and compared after, rather than asserted empty.** A daemon that has
    // just started has already asked for whatever its first run needs — the certificate authority,
    // on this machine — and a test demanding an empty queue would be asserting that first-run setup
    // does not happen. What is being proved is that the *plan* added nothing to it.
    let queued = json(&home.mix(&["elevation", "status", "--json"]))["pending"].clone();

    let report = json(&home.mix(&["uninstall", "--dry-run", "--json"]));

    let items = report["items"].as_array().expect("a list of rows");
    assert!(items.len() >= 11, "{report}");

    for row in items {
        assert!(
            row["what"].as_str().is_some_and(|what| !what.is_empty()),
            "{row}"
        );
        assert!(
            row["location"]
                .as_str()
                .is_some_and(|place| !place.is_empty()),
            "every row says where to go and look: {row}"
        );

        // A dry run acts on nothing, so no row may claim a removal or a queue entry.
        let removal = row["outcome"]["removal"]
            .as_str()
            .expect("a tagged outcome");
        assert!(
            matches!(removal, "planned" | "absent" | "kept" | "failed"),
            "{row}"
        );
    }

    assert_eq!(home.contents(), before, "a dry run wrote something");

    // And it asked for nothing: an operation a plan left behind would be the prompt it promised not
    // to raise, arriving at whatever the person did next.
    let waiting = json(&home.mix(&["elevation", "status", "--json"]));
    assert_eq!(waiting["pending"], queued, "the plan enqueued something");
}

/// And the home row names the home, so the one irreversible thing on the list is the one a person
/// cannot miss.
#[tokio::test(flavor = "multi_thread")]
async fn the_plan_names_the_home_it_would_remove() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let printed = stdout(&home.mix(&["uninstall", "--dry-run"]));

    assert!(
        printed.contains(&home.path().display().to_string()),
        "{printed}"
    );
    assert!(printed.contains("data/"), "{printed}");
}

/// `--keep-home` says so on the home row rather than leaving it out. A person reading the plan has
/// to see that the home was considered and deliberately left.
#[tokio::test(flavor = "multi_thread")]
async fn keeping_the_home_is_a_row_and_not_a_silence() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let report = json(&home.mix(&["uninstall", "--dry-run", "--keep-home", "--json"]));

    let kept = report["items"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["id"] == "home")
        .expect("the home is always a row");

    assert_eq!(kept["outcome"]["removal"], "kept", "{report}");
}

/// One document per run under `--json`, and the plan is not it.
///
/// **The regression this pins was found by CI and not by reading**: `mix uninstall --yes --json`
/// printed the plan *and* the report, which is two objects on one stdout and therefore not JSON —
/// every caller parsing the output got a trailing-characters error rather than an answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_json_run_emits_exactly_one_object() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    // The plan is the answer here, so it is the one object.
    let printed = stdout(&home.mix(&["uninstall", "--dry-run", "--json"]));
    serde_json::from_str::<serde_json::Value>(&printed)
        .unwrap_or_else(|error| panic!("{error}: {printed}"));

    // And declining is not an object at all: nothing was asked for, so nothing is answered.
    let declined = home.mix_answering("n\n", &["uninstall"]);
    assert_eq!(declined.status.code(), Some(0), "{}", stderr(&declined));
}

/// Nobody at the keyboard is not a yes. `mix` reads end of file as *there was nobody to ask* and
/// names the flag that answers in advance — `mix elevation grant`'s standing rule, on the one
/// command where getting it wrong removes somebody's databases.
#[tokio::test(flavor = "multi_thread")]
async fn an_unattended_uninstall_without_yes_removes_nothing() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let refused = home.mix(&["uninstall"]);

    assert!(
        home.path().exists(),
        "the home was removed with nobody asked"
    );
    assert!(stderr(&refused).contains("--yes"), "{}", stderr(&refused));
    assert_ne!(refused.status.code(), Some(0), "{}", stdout(&refused));
}

/// T182, D9. On a home with nothing relocated, the listing the Windows uninstaller reads is empty
/// and the command succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn listing_relocated_directories_on_a_plain_home_prints_nothing() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let printed = home.mix(&["uninstall", "--dry-run", "--relocated"]);

    assert!(printed.status.success(), "{}", stderr(&printed));
    assert_eq!(stdout(&printed).trim(), "", "{}", stdout(&printed));
}

/// T182, D4. A program running from inside the home makes the dry run exit `3` and name it — the
/// code the Windows uninstaller reads as *close this and try again*.
#[tokio::test(flavor = "multi_thread")]
async fn a_program_running_from_the_home_blocks_with_exit_code_three() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    // **Ten minutes, not thirty seconds** (T202b, D6): under a full parallel run the daemon's start
    // and the plan once took longer than the occupant lived, and the plan found nothing in the way.
    // The occupant is killed below whatever happens.
    let (source, args): (std::path::PathBuf, &[&str]) = if cfg!(windows) {
        (
            std::path::PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot"))
                .join(r"System32\PING.EXE"),
            &["-n", "600", "127.0.0.1"],
        )
    } else {
        (std::path::PathBuf::from("/bin/sleep"), &["600"])
    };
    let directory = home.path().join("t182-occupant");
    std::fs::create_dir_all(&directory).expect("a directory in the home");
    let copy = directory.join(source.file_name().expect("a file name"));
    std::fs::copy(&source, &copy).expect("copy the program");
    let mut occupant = std::process::Command::new(&copy)
        .args(args)
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("start the occupant");

    let printed = home.mix(&["uninstall", "--dry-run"]);

    let _ = occupant.kill();
    let _ = occupant.wait();

    assert_eq!(printed.status.code(), Some(3), "{}", stdout(&printed));
    assert!(
        stdout(&printed).contains("BLOCKED"),
        "the program in the way is named: {}",
        stdout(&printed)
    );
}

/// T182e. What the Windows uninstaller reads is ASCII when it asks: the firewall row's label is
/// spelled with an em dash, which its log showed as `â€”`, and a plain dash here says the whole
/// plan went through the same door.
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_read_as_plain_text_is_ascii() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let plain = home.mix_in(
        home.path(),
        &[("MIXENGINE_PLAIN_TEXT", "1")],
        &["uninstall", "--dry-run"],
    );
    let usual = home.mix(&["uninstall", "--dry-run"]);

    assert!(
        stdout(&usual).contains("MixEngine — shared sites"),
        "{}",
        stdout(&usual)
    );
    assert!(
        stdout(&plain).contains("MixEngine - shared sites"),
        "{}",
        stdout(&plain)
    );
    assert!(stdout(&plain).is_ascii(), "{}", stdout(&plain));
}

/// T182e, D6. On a home nothing is in the way of, the listing the Windows uninstaller's "close
/// these first" page reads is empty, and the command succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn listing_what_is_in_the_way_of_a_plain_home_prints_nothing() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let printed = home.mix(&["uninstall", "--dry-run", "--blocked"]);

    assert!(printed.status.success(), "{}", stderr(&printed));
    assert_eq!(stdout(&printed).trim(), "", "{}", stdout(&printed));
}

/// T182e, D6. A program standing in the home is one line of that listing, naming its pid and the
/// folder it holds, and the listing still succeeds — it is a question, not a refusal.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn listing_what_is_in_the_way_names_a_program_standing_in_the_home() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let directory = home.path().join("t182e-listed");
    std::fs::create_dir_all(&directory).expect("a directory in the home");
    let ping = std::path::PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot"))
        .join(r"System32\PING.EXE");
    let mut occupant = std::process::Command::new(ping)
        .args(["-n", "30", "127.0.0.1"])
        .current_dir(&directory)
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("start the occupant");

    let printed = home.mix(&["uninstall", "--dry-run", "--blocked"]);

    let _ = occupant.kill();
    let _ = occupant.wait();

    let said = stdout(&printed);
    assert!(printed.status.success(), "{}", stderr(&printed));
    let line = said
        .lines()
        .find(|line| line.contains(&format!("(pid {})", occupant.id())))
        .unwrap_or_else(|| panic!("the occupant is not listed: {said}"));
    assert!(line.contains("t182e-listed"), "{line}");
}

/// T182e, D6. `--blocked` without `--dry-run` is refused by the parser: it changes nothing.
#[test]
fn listing_what_is_in_the_way_requires_a_dry_run() {
    let home = Home::new();

    let printed = home.mix(&["uninstall", "--blocked"]);

    assert_eq!(printed.status.code(), Some(2), "{}", stderr(&printed));
}

/// T182e, D4. A program *standing* in the home — a working directory, which cannot be moved — makes
/// the dry run exit `3` and name it.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn a_program_standing_in_the_home_blocks_with_exit_code_three() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let directory = home.path().join("t182e-standing");
    std::fs::create_dir_all(&directory).expect("a directory in the home");
    let ping = std::path::PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot"))
        .join(r"System32\PING.EXE");
    let mut occupant = std::process::Command::new(ping)
        .args(["-n", "30", "127.0.0.1"])
        .current_dir(&directory)
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("start the occupant");

    let printed = home.mix(&["uninstall", "--dry-run"]);

    let _ = occupant.kill();
    let _ = occupant.wait();

    let said = stdout(&printed);
    assert_eq!(printed.status.code(), Some(3), "{said}");
    assert!(said.contains("BLOCKED"), "{said}");
    assert!(said.contains(&format!("(pid {})", occupant.id())), "{said}");
}

/// T182e, D4. A directory only *watched* — what an editor does — can be moved, so it is not in the
/// way and the dry run does not block.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn a_watched_folder_in_the_home_does_not_block() {
    use std::os::windows::fs::OpenOptionsExt as _;

    let home = Home::new();
    let _daemon = home.start_daemon();

    let directory = home.path().join("t182e-watched");
    std::fs::create_dir_all(&directory).expect("a directory in the home");
    let _watch = std::fs::OpenOptions::new()
        .access_mode(0x0010_0081)
        .share_mode(0x7)
        .custom_flags(0x0200_0000)
        .open(&directory)
        .expect("the directory watched");

    let printed = home.mix(&["uninstall", "--dry-run"]);

    let said = stdout(&printed);
    assert_ne!(printed.status.code(), Some(3), "{said}");
    assert!(!said.contains("BLOCKED"), "{said}");
}

/// T182, D9. `--relocated` without `--dry-run` is refused by the parser: it changes nothing, and
/// must never be mistaken for the act.
#[test]
fn listing_requires_a_dry_run() {
    let home = Home::new();

    let printed = home.mix(&["uninstall", "--relocated"]);

    assert_eq!(printed.status.code(), Some(2), "{}", stderr(&printed));
}
