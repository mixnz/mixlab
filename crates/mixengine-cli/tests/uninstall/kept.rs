//! A kept home's passwords, and a package row in a dry run.

use super::*;

/// T182d: this home's passwords are a row of the plan, and a kept home keeps them.
///
/// A development daemon keeps its credentials in `<home>/credentials.json` (ADR 0052), which is what
/// lets this run on any workstation: the file is seeded the way the daemon would have written it,
/// with one entry of this home's and one of another's.
#[tokio::test(flavor = "multi_thread")]
async fn a_kept_home_keeps_its_passwords() {
    let home = Home::new();
    let mut daemon = home.start_daemon();

    let id = mixengine_testkit::declare::home_id(&home.database_file()).await;
    let ours = format!("{id}/mariadb@main/root");
    let file = home.path().join("credentials.json");
    let mut entries = serde_json::Map::new();
    entries.insert(ours.clone(), "x".into());
    entries.insert("ffffffffffff/other/user".to_owned(), "y".into());
    let seeded = serde_json::json!({ "version": 1, "entries": { "mixengine": entries } });
    std::fs::write(&file, seeded.to_string()).expect("the credential file");

    let plan = json(&home.mix(&["uninstall", "--dry-run", "--json"]));
    let row = plan["items"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["id"] == "credentials")
        .unwrap_or_else(|| panic!("no credentials row: {plan}"))
        .clone();
    assert_eq!(row["outcome"]["removal"], "planned", "{plan}");
    assert!(
        row["what"]
            .as_str()
            .is_some_and(|what| what.starts_with("1 password")),
        "another home's entry was counted as this one's: {plan}"
    );

    // The plan is proved on every machine; the run only where nothing else needs an administrator.
    if needs_an_administrator(&home) {
        return;
    }

    let report = json(&home.mix(&["uninstall", "--keep-home", "--yes", "--json"]));
    let kept = report["items"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["id"] == "credentials")
        .unwrap_or_else(|| panic!("no credentials row: {report}"));
    assert_eq!(kept["outcome"]["removal"], "kept", "{report}");
    assert!(daemon.wait_until_gone(), "{report}");

    let left: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).expect("the file is still there"))
            .expect("still JSON");
    assert_eq!(left["entries"]["mixengine"][&ours], "x", "{left}");
    assert_eq!(
        left["entries"]["mixengine"]["ffffffffffff/other/user"], "y",
        "{left}"
    );
}

/// T182a: `--package` is a flag of the plan too. A development daemon was placed by no installer,
/// so here the row is absent (or, on a Windows checkout, kept); on a `.pkg` install it is `planned`.
/// What must not happen is an unknown flag, or a dry run that claims a removal.
#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_with_package_shows_the_package_row_and_removes_nothing() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let output = home.mix(&["uninstall", "--dry-run", "--package", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    let report = json(&output);
    let items = report["items"].as_array().expect("a list of rows");
    if let Some(package) = items.iter().find(|row| row["id"] == "package") {
        assert!(
            matches!(
                package["outcome"]["removal"].as_str(),
                Some("planned" | "absent" | "kept")
            ),
            "{package}"
        );
    }
}
