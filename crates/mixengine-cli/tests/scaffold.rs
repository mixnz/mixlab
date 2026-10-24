//! Importing a blueprint somebody else wrote, and running its own command.
//!
//! What is proved here is the whole of the task end to end and what no unit test can: that a
//! hand-written blueprint arrives *untrusted*, that its `[scaffold]` command does not run because
//! the apply was sent, and that it does run — in the new project's directory — when somebody says
//! so with the gesture an unsigned blueprint takes.
//!
//! **Offline by construction**, as `tests/blueprint.rs` is: the fixture is a static site with no
//! runtime and no services, so nothing here reaches the package index. The command the blueprint
//! carries is written per operating system by this file, which is the honest consequence of running
//! it through the OS shell.

mod harness;

use harness::{Home, json, stderr, stdout};

/// A directory for a new project.
fn repository() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("mixengine-scaffold")
        .tempdir()
        .expect("a temporary directory")
}

/// A command that writes a file, on this system.
fn a_command_that_writes_a_file() -> &'static str {
    if cfg!(windows) {
        "echo hello> made.txt"
    } else {
        "printf hello > made.txt"
    }
}

/// A blueprint written by hand, carrying the one section a capture never writes.
fn a_blueprint_with_a_command(command: &str) -> String {
    format!(
        r#"schema = 1

[blueprint]
name = "borrowed"
description = "somebody else's stack"
created_at = "2026-09-01T00:00:00Z"
created_on = {{ os = "linux", version = "0.1.0" }}

[site]
kind = "static"
doc_root = "public"
https = false
domain_pattern = "{{project}}.test"

[scaffold]
command = "{command}"
"#
    )
}

/// Write one into the home under `stem` and import it, answering with the summary the daemon wrote
/// down. The slug is the stem, so two blueprints in one home take two stems.
fn imported_running(home: &Home, stem: &str, command: &str) -> serde_json::Value {
    let file = home.path().join(format!("{stem}.toml"));
    std::fs::write(&file, a_blueprint_with_a_command(command)).expect("a blueprint to import");

    json(&home.mix(&["blueprint", "import", &file.display().to_string(), "--json"]))
}

/// The usual one: `borrowed`, running the command that writes a file.
fn imported(home: &Home) -> serde_json::Value {
    imported_running(home, "borrowed", a_command_that_writes_a_file())
}

/// **Nothing vouched for it, so it is untrusted** — and it stays that way, because no method in this
/// build raises the flag once the row is written.
#[tokio::test(flavor = "multi_thread")]
async fn a_hand_written_blueprint_arrives_untrusted() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let summary = imported(&home);

    assert_eq!(summary["source"], "imported", "{summary}");
    assert_eq!(summary["trusted"], false, "{summary}");

    // **Found by slug rather than by position**: the gallery puts built-in blueprints in every
    // home, so index zero is whichever gallery slug sorts first and not this one.
    let listed = json(&home.mix(&["blueprint", "list", "--json"]));
    let slug = summary["slug"].as_str().expect("a slug");
    let found = listed["blueprints"]
        .as_array()
        .expect("a listing")
        .iter()
        .find(|one| one["slug"] == slug)
        .unwrap_or_else(|| panic!("the imported blueprint is not listed: {listed}"));

    assert_eq!(found["trusted"], false, "a listing says it too: {listed}");
}

/// **The command runs only when somebody agrees to it, and then it really runs** — the task's whole
/// subject. Both halves in one test, because what makes the second meaningful is the first: a build
/// that ran the command either way would pass half of this.
#[tokio::test(flavor = "multi_thread")]
async fn a_blueprints_own_command_runs_only_when_asked_for() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    imported(&home);

    // Without the gesture: everything else is applied, and the command is left as a sentence.
    let first = repository();
    let one = first.path().join("one");
    let said = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "borrowed",
        "--project",
        "one",
        "--path",
        &one.display().to_string(),
    ]));

    assert!(said.contains("not run"), "{said}");
    assert!(
        !one.join("made.txt").exists(),
        "nobody agreed to it, so it did not run"
    );

    // With it: the command runs, in the project's own directory.
    let second = repository();
    let two = second.path().join("two");
    let ran = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "borrowed",
        "--project",
        "two",
        "--path",
        &two.display().to_string(),
        "--run-untrusted-scaffold",
    ]));

    assert!(ran.contains("done"), "{ran}");
    assert!(
        two.join("made.txt").is_file(),
        "the command ran in the new project's directory: {ran}"
    );
}

/// **`--run-scaffold` is not the gesture an unsigned blueprint takes**, and the refusal names the
/// one that is rather than leaving somebody guessing.
#[tokio::test(flavor = "multi_thread")]
async fn the_flag_for_a_signed_blueprint_does_not_answer_for_an_unsigned_one() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    imported(&home);

    let directory = repository();
    let root = directory.path().join("three");
    let output = home.mix(&[
        "blueprint",
        "apply",
        "borrowed",
        "--project",
        "three",
        "--path",
        &root.display().to_string(),
        "--run-scaffold",
    ]);

    let complaint = stderr(&output);

    assert!(
        complaint.contains("--run-untrusted-scaffold"),
        "{complaint}"
    );
    assert!(
        !root.join("made.txt").exists(),
        "and nothing ran while it was being refused"
    );
}

/// **A closed standard input is not agreement.** A person who declines gets the project without the
/// command rather than no project — the same outcome as sending no consent at all.
#[tokio::test(flavor = "multi_thread")]
async fn declining_the_question_leaves_the_command_and_keeps_the_project() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    imported(&home);

    let directory = repository();
    let root = directory.path().join("four");
    let said = stdout(&home.mix_answering(
        "n",
        &[
            "blueprint",
            "apply",
            "borrowed",
            "--project",
            "four",
            "--path",
            &root.display().to_string(),
        ],
    ));

    assert!(
        said.contains(a_command_that_writes_a_file()),
        "the question shows the command exactly as it would run: {said}"
    );
    assert!(!root.join("made.txt").exists(), "{said}");

    // And the project it made is there, which is what "declining is not a failure" means.
    let shown = json(&home.mix(&["project", "show", "four", "--json"]));
    assert_eq!(shown["project"]["name"], "four", "{shown}");
}

/// A program no machine has, spelled so that a shell would have looked it up on `PATH`.
const A_PROGRAM_NOBODY_HAS: &str = "mixengine-no-such-program-t78b";

/// **A command whose program is missing is blocked in the plan, refused when agreed to, and left
/// when it is not — and the project is there either way.** End to end: the plan says it, the
/// refusal says it in the plan's words, and an apply without the gesture still applies everything
/// else.
#[tokio::test(flavor = "multi_thread")]
async fn a_command_whose_program_is_missing_is_blocked_before_anything_runs() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    imported_running(
        &home,
        "missing",
        &format!("{A_PROGRAM_NOBODY_HAS} --into ."),
    );

    // The plan says it, naming the program and both halves of the PATH.
    let directory = repository();
    let root = directory.path().join("five");
    let planned = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "missing",
        "--project",
        "five",
        "--path",
        &root.display().to_string(),
        "--dry-run",
    ]));
    assert!(planned.contains("blocked"), "{planned}");
    assert!(planned.contains(A_PROGRAM_NOBODY_HAS), "{planned}");
    assert!(planned.contains("daemon's own PATH"), "{planned}");

    // Agreed to: refused up front, in the plan's words, and nothing was made.
    let refused = home.mix(&[
        "blueprint",
        "apply",
        "missing",
        "--project",
        "five",
        "--path",
        &root.display().to_string(),
        "--run-untrusted-scaffold",
    ]);
    assert!(!refused.status.success(), "{}", stdout(&refused));
    let complaint = stderr(&refused);
    assert!(complaint.contains(A_PROGRAM_NOBODY_HAS), "{complaint}");
    assert!(complaint.contains("restart"), "{complaint}");
    assert!(
        !home
            .mix(&["project", "show", "five", "--json"])
            .status
            .success(),
        "a refused apply registers nothing"
    );

    // Not agreed to: everything else is applied, and the step says why it was left.
    let applied = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "missing",
        "--project",
        "five",
        "--path",
        &root.display().to_string(),
    ]));
    assert!(applied.contains("not run"), "{applied}");
    assert!(applied.contains(A_PROGRAM_NOBODY_HAS), "{applied}");

    let shown = json(&home.mix(&["project", "show", "five", "--json"]));
    assert_eq!(shown["project"]["name"], "five", "{shown}");
}

/// **A project named the way people name things gets a directory a tool will accept.**
///
/// The whole rule in one apply: no `--path`, a name carrying a capital, a space and a dot, and the
/// directory that appears is the *handle*. The scaffold's own file is what proves the command ran
/// **in** that directory rather than somewhere a plan merely mentioned.
///
/// Offline, and deliberately not a `create-next-app` run: what is under test is which directory
/// MixEngine composes, and a test that downloaded Next.js to find that out would be measuring npm.
/// The real thing is measured by hand — the design's acceptance list.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_name_that_is_not_a_slug_still_gets_a_directory_a_tool_accepts() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    imported(&home);

    // **From a directory of its own**, because what is under test is the default `mix` sends when
    // nobody names a path, and that default is relative to where `mix` was run.
    let repository = repository();
    let ran = stdout(&home.mix_in(
        repository.path(),
        &[],
        &[
            "blueprint",
            "apply",
            "borrowed",
            "--project",
            "Next.js 1",
            "--run-untrusted-scaffold",
        ],
    ));

    assert!(ran.contains("done"), "{ran}");

    let root = repository.path().join("next-js-1");
    assert!(root.is_dir(), "the apply made {}: {ran}", root.display());
    assert!(
        !repository.path().join("Next.js 1").exists(),
        "a project's name must not become a directory name: {ran}"
    );

    // And the command ran inside it, which is the half a plan cannot promise.
    assert!(
        root.join("made.txt").is_file(),
        "the command ran somewhere else: {:?}",
        std::fs::read_dir(repository.path())
            .expect("the parent directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>()
    );
}

/// **A path somebody typed is used as typed** — the other half of the decision above. A folder a
/// person named is a folder a person named.
#[tokio::test(flavor = "multi_thread")]
async fn a_path_somebody_named_is_not_respelled() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    imported(&home);

    let repository = repository();
    let named = repository.path().join("Next.js 2");

    let ran = stdout(&home.mix_in(
        repository.path(),
        &[],
        &[
            "blueprint",
            "apply",
            "borrowed",
            "--project",
            "Next.js 2",
            "--path",
            &named.display().to_string(),
            "--run-untrusted-scaffold",
        ],
    ));

    assert!(ran.contains("done"), "{ran}");
    assert!(named.is_dir(), "the directory keeps its spelling: {ran}");
    assert!(
        !repository.path().join("next-js-2").exists(),
        "nothing composes a handle when a path was given: {ran}"
    );
}
