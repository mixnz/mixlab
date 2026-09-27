//! `mix storage` — where a home's growing directories are, asked without a daemon.
//!
//! **The claim these tests hold is that no daemon is involved.** `mix` links neither
//! `mixengine-core` nor `sqlx`, so the answer comes from running `mixengined` once; what has to be
//! true is that asking starts nothing, creates nothing, and renders what the other binary said
//! rather than a second opinion. Roadmap task **T145**.

use std::process::Command;

mod harness;

/// Run `mix` against `home`, with the daemon binary built beside it.
///
/// `MIXENGINE_DAEMON_BIN` is how a `mix` in `target/debug` is told which `mixengined` to run — the
/// variable `autostart.rs` reads first, and the reason it exists.
fn mix(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mix"))
        .env("MIXENGINE_DAEMON_BIN", harness::daemon_binary())
        .args(args)
        .arg("--home")
        .arg(home)
        .output()
        .expect("the mix binary runs")
}

/// A home nobody has started answers, and is still not there afterwards.
#[test]
fn a_home_that_does_not_exist_is_described_and_not_created() {
    let parent = tempfile::tempdir().expect("a temporary directory");
    let absent = parent.path().join("never-started");

    let output = mix(&absent, &["storage"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(printed.contains("runtimes"), "{printed}");
    assert!(printed.contains("data"), "{printed}");
    assert!(
        printed.contains("nothing is installed yet"),
        "a home with nothing in it may still choose: {printed}"
    );

    assert!(
        !absent.exists(),
        "`mix storage` created the home it was asked about"
    );
}

/// **`--json` is the daemon's document, unchanged.** A client that re-serialised what it had only
/// parsed in order to print would be a second chance to render the same facts differently.
#[test]
fn json_is_what_the_daemon_printed() {
    let parent = tempfile::tempdir().expect("a temporary directory");
    let absent = parent.path().join("never-started");

    let through_mix = mix(&absent, &["--json", "storage"]);
    assert!(through_mix.status.success());

    let direct = Command::new(harness::daemon_binary())
        .arg("--storage")
        .arg("--home")
        .arg(&absent)
        .output()
        .expect("the daemon binary runs");
    assert!(direct.status.success());

    assert_eq!(
        String::from_utf8_lossy(&through_mix.stdout).trim(),
        String::from_utf8_lossy(&direct.stdout).trim()
    );

    // And it is one document, so a caller may parse it without finding the end of a stream first.
    let parsed: mixengine_proto::StorageReport =
        serde_json::from_slice(&through_mix.stdout).expect("one JSON document");
    assert!(parsed.changeable.is_free());
}

/// A `mix` that cannot find a daemon binary says so as a missing dependency, not as a crash.
#[test]
fn a_missing_daemon_binary_is_reported_as_one() {
    let parent = tempfile::tempdir().expect("a temporary directory");

    let output = Command::new(env!("CARGO_BIN_EXE_mix"))
        .env("MIXENGINE_DAEMON_BIN", parent.path().join("not-a-binary"))
        .args(["--json", "storage", "--home"])
        .arg(parent.path())
        .output()
        .expect("the mix binary runs");

    assert!(!output.status.success());

    let error: serde_json::Value = serde_json::from_slice(&output.stdout)
        .or_else(|_| serde_json::from_slice(&output.stderr))
        .expect("a wire error as JSON");

    assert_eq!(error["code"], "dependency_missing", "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("cannot run ")),
        "a question asked of the daemon binary is not reported as a failed start: {error}"
    );
}

// ---------------------------------------------------------------------------------------------
// A home whose four directories are elsewhere — roadmap task T147.
//
// **The assertion is that nothing restated the layout twice.** `[paths]` is read in one place and
// the rest of the workspace is supposed to ask `Paths` rather than compose its own answer; these
// walk the two paths where a second copy would show — the uninstall inventory, which has to name a
// directory it will not find under the root, and the shim, which deliberately does not read
// `config.toml` at all.
// ---------------------------------------------------------------------------------------------

mod relocated {
    use super::harness;
    use harness::{Home, json};

    /// A home with all four keys pointing somewhere else, and the daemon that made it so.
    ///
    /// Returned together because the directory has to outlive the home: dropping it first would
    /// take the four directories out from under a daemon that is still running.
    fn a_relocated_home() -> (Home, tempfile::TempDir) {
        let home = Home::new();
        let bulk = tempfile::tempdir().expect("somewhere to move things to");

        let at = |name: &str| bulk.path().join(name).display().to_string();
        let daemon = home.start_daemon_with(&[
            "--runtimes",
            &at("runtimes"),
            "--packages",
            &at("packages"),
            "--data",
            &at("data"),
            "--logs",
            &at("logs"),
        ]);
        drop(daemon);

        (home, bulk)
    }

    /// Every relocated directory is a row of its own in the plan, named where it actually is.
    ///
    /// **The plan is what a client reads back once the daemon is gone**, so a directory the
    /// inventory failed to notice is one nothing would ever remove — and `mix uninstall` would
    /// report a machine as clean while a database sat on another disk.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_uninstall_plan_names_all_four_where_they_are() {
        let (home, bulk) = a_relocated_home();
        let _daemon = home.start_daemon_with(&[]);

        let report = json(&home.mix(&["uninstall", "--dry-run", "--json"]));
        let items = report["items"].as_array().expect("a list of rows");

        let relocated: Vec<&serde_json::Value> = items
            .iter()
            .filter(|row| row["id"] == "relocated_directory")
            .collect();

        assert_eq!(relocated.len(), 4, "{report}");

        for name in ["runtimes", "packages", "data", "logs"] {
            let expected = bulk.path().join(name).display().to_string();
            assert!(
                relocated
                    .iter()
                    .any(|row| row["location"].as_str() == Some(expected.as_str())),
                "{name} is not in the plan at {expected}: {report}"
            );
        }
    }

    /// The shim still resolves against a home whose directories moved.
    ///
    /// It reads neither key — `mixengine-shim` builds its `Paths` from `PathOverrides::default()`
    /// twice, deliberately, and uses only the database and `etc/`, neither of which `[paths]` can
    /// move. This is what keeps that comment true rather than merely written down: a shim that
    /// started reading `config.toml` would still pass its own tests and fail here.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_shim_still_answers_for_a_relocated_home() {
        let (home, bulk) = a_relocated_home();

        // A PHP inside the relocated `runtimes/`, recorded, and a start to put its name in `bin/` —
        // since T185b a language with nothing installed has no command there to run.
        let program = match cfg!(windows) {
            true => "php.exe",
            false => "bin/php",
        };
        mixengine_testkit::declare::runtime(
            &home.database_file(),
            "php",
            "8.3.33",
            &bulk.path().join("runtimes").join("php").join("8.3.33"),
            program,
        )
        .await;
        drop(home.start_daemon_with(&[]));

        let php = home
            .path()
            .join("bin")
            .join(format!("php{}", std::env::consts::EXE_SUFFIX));
        assert!(
            php.is_file(),
            "a start with a PHP recorded left no php in bin/\n--- daemon ---\n{}",
            home.daemon_log()
        );

        let output = std::process::Command::new(&php)
            .env("MIXENGINE_HOME", home.path())
            .arg("--version")
            .output()
            .expect("the shim runs");

        // The answer is the installed program's: the shim read this home's database, found the PHP
        // on the other disk, and ran it. A shim that had tripped over the relocation would fail
        // about a directory instead.
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success() && said.contains("fakeservice"),
            "the shim did not run the PHP in the relocated runtimes/: {said}"
        );
    }
}
