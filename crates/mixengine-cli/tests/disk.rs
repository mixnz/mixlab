//! `mix disk` and `mix cleanup` against a daemon that is really running.
//!
//! What is worth proving here and nowhere else is that the two halves agree: what `mix disk` says
//! would be taken back is what `mix cleanup` takes, measured on a real home rather than on a
//! fixture — and that the file the daemon is logging into right now is still there afterwards.

mod harness;

use harness::Home;
use mixengine_proto::{CleanupReport, DiskCategory, DiskUsage, Reclaim};

/// Five rows, in one order, and a `--json` document a client can parse.
#[test]
fn mix_disk_answers_five_categories() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let output = home.mix(&["--json", "disk"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let usage: DiskUsage =
        serde_json::from_slice(&output.stdout).expect("mix disk --json is a DiskUsage");

    assert_eq!(
        usage
            .categories
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        DiskCategory::ALL.to_vec()
    );
    assert_eq!(usage.root, home.path().display().to_string());
}

/// What the table offered is what the cleanup took, and the live log is still there afterwards.
#[test]
fn mix_cleanup_takes_the_rotated_logs_and_leaves_the_live_one() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let logs = home.path().join("logs");
    let rotated = logs.join("daemon.log.1");
    std::fs::write(&rotated, vec![0u8; 3 << 20]).expect("a rotated log");

    let before: DiskUsage = serde_json::from_slice(&home.mix(&["--json", "disk"]).stdout)
        .expect("mix disk --json is a DiskUsage");
    let offered = before
        .categories
        .iter()
        .find(|row| row.id == DiskCategory::Logs)
        .map(|row| row.reclaim.clone())
        .expect("a logs row");
    assert!(
        matches!(offered, Reclaim::ByCleanup { bytes, .. } if bytes >= 3 << 20),
        "{offered:?}"
    );

    let output = home.mix(&["--json", "cleanup", "--yes", "--keep-cache"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: CleanupReport =
        serde_json::from_slice(&output.stdout).expect("mix cleanup --json is a CleanupReport");

    assert!(report.reclaimed_bytes() >= 3 << 20, "{report:?}");
    assert!(!report.left_behind(), "{report:?}");

    assert!(!rotated.exists(), "the rotated copy is still there");
    assert!(
        logs.join("daemon.log").exists(),
        "the live log went with it"
    );
}

/// A confirmation nobody can answer removes nothing — `mix uninstall`'s rule, and its reason: a
/// script with nobody at the keyboard says yes with `--yes` or it does not say yes at all.
#[test]
fn a_cleanup_nobody_can_answer_removes_nothing() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let rotated = home.path().join("logs/daemon.log.1");
    std::fs::write(&rotated, vec![0u8; 1024]).expect("a rotated log");

    let output = home.mix(&["cleanup"]);

    assert!(
        rotated.exists(),
        "a question nobody answered removed a file"
    );
    assert!(
        !output.status.success(),
        "a question nobody answered is not a cleanup that happened"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--yes"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The table a person reads names all five categories and ends on the one number that leads to a
/// command — the half of this feature `--json` says nothing about.
#[test]
fn the_table_names_every_category_and_ends_on_what_cleanup_would_take() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    std::fs::write(home.path().join("logs/daemon.log.1"), vec![0u8; 2 << 20])
        .expect("a rotated log");

    let output = home.mix(&["disk"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8(output.stdout).expect("utf-8");

    for category in DiskCategory::ALL {
        assert!(text.contains(category.as_str()), "{text}");
    }

    assert!(text.contains("other"), "{text}");
    assert!(text.contains("reclaimed by"), "{text}");
    assert!(
        text.contains("`mix cleanup` would take back 2 MiB"),
        "{text}"
    );
}
