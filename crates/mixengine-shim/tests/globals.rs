//! A globally installed tool is a command — roadmap task **T131**.
//!
//! The second complaint this phase answers, end to end: `npm -g install yarn`, and then the `yarn`
//! command cannot be run. npm's global prefix is the Node install's own directory, which is on
//! nobody's PATH — so the program existed, ran perfectly well when named by its full path, and
//! could not be typed.
//!
//! What is proved here is that it becomes a command **and follows the version**, which is the half
//! that a PATH entry pointing at one install could never do: a directory pinned to another Node
//! gets that Node's Yarn, or a sentence naming the version and what to type.

mod harness;

use std::collections::BTreeMap;

use harness::Home;
use mixengine_proto::RuntimeKind;

/// What a Node artifact publishes, spelled as each system really packs it.
fn node_provides() -> BTreeMap<String, String> {
    let at = match cfg!(windows) {
        true => "node.exe",
        false => "bin/node",
    };

    [("node".to_owned(), at.to_owned())].into_iter().collect()
}

/// **The complaint, answered.** A tool installed into a runtime is a command in the same shell.
#[test]
fn a_tool_installed_into_a_runtime_is_a_command() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Node, "24.19.0", node_provides());
    home.install_globally(RuntimeKind::Node, "24.19.0", "yarn");

    let recorded = home.record_command("yarn", home.path(), &BTreeMap::new(), 0);

    assert!(
        recorded.reached,
        "yarn did not run: {}",
        recorded.run.stderr()
    );
}

/// And it runs with its own runtime's directory ahead of the PATH, so a `yarn` that shells out to
/// `node` reaches the Node it belongs to rather than whatever the machine has.
#[test]
fn a_tool_finds_the_runtime_it_belongs_to() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Node, "24.19.0", node_provides());
    home.install_globally(RuntimeKind::Node, "24.19.0", "yarn");

    let recorded = home.record_command("yarn", home.path(), &BTreeMap::new(), 0);
    let path = recorded.recorded("PATH").expect("a PATH");

    assert!(
        path.contains("24.19.0"),
        "the tool's own runtime is not ahead of the PATH: {path}"
    );
}

/// **It follows the version.** A directory that resolves to a Node without the tool is told which
/// version that is and what to type — not a bare 127 with nothing on stderr.
#[test]
fn a_version_without_the_tool_says_which_version_and_what_to_type() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Node, "24.19.0", node_provides());
    home.install_globally(RuntimeKind::Node, "24.19.0", "yarn");

    // A second Node, installed after the first, with no Yarn in it. `MIXENGINE_NODE` is step one
    // of the resolution order, which is how one command is aimed at it.
    home.install(RuntimeKind::Node, "22.14.0", node_provides());

    let session = [("MIXENGINE_NODE", "22.14.0".to_owned())]
        .into_iter()
        .collect();
    let recorded = home.record_command("yarn", home.path(), &session, 0);

    assert!(!recorded.reached);

    let said = recorded.run.stderr();
    assert!(said.contains("22.14.0"), "{said}");
    assert!(said.contains("npm install -g yarn"), "{said}");
}

/// A compiled command is never displaced by a discovered one. Somebody who runs
/// `npm install -g npm` has an `npm` in their bindir; fronting it would make `bin/npm` a shim that
/// dispatches to a file found by a shim, and the first file it would find is itself.
#[test]
fn a_globally_installed_npm_does_not_replace_the_compiled_one() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Node, "24.19.0", node_provides());
    home.install_globally(RuntimeKind::Node, "24.19.0", "npm");

    // `npm` still resolves the way the compiled table says: through the artifact's `provides`,
    // which this fixture's Node does not carry — so the failure names what it *does* publish
    // rather than looping through a bindir.
    let recorded = home.record_command("npm", home.path(), &BTreeMap::new(), 0);

    assert!(!recorded.reached);
    assert!(
        recorded.run.stderr().contains("publishes no executable"),
        "{}",
        recorded.run.stderr()
    );
}

/// A tool whose runtime was uninstalled leaves the table, and the name leaves `bin/` with it.
#[test]
fn a_tool_whose_runtime_went_away_stops_being_a_command() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Node, "24.19.0", node_provides());
    home.install_globally(RuntimeKind::Node, "24.19.0", "yarn");

    let yarn = home
        .path()
        .join("bin")
        .join(format!("yarn{}", std::env::consts::EXE_SUFFIX));
    assert!(yarn.is_file(), "the first pass wrote it");

    home.uninstall_runtime(RuntimeKind::Node, "24.19.0");

    assert!(!yarn.exists(), "{} survived the uninstall", yarn.display());
}
