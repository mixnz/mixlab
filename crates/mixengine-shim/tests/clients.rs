//! The client commands of an installed service package — roadmap task **T130**.
//!
//! The first of the three complaints this phase answers, end to end: `bin` has no `mysql`,
//! `mysqldump`, … — a home with a database in it had no way to open one from a terminal, because
//! `<root>/bin` was a projection of a compile-time constant that named four languages and nothing
//! else.
//!
//! What is proved here is the whole path a person walks: the daemon's own composition fills `bin/`
//! from the installed rows, a copy of the shim lands under `mariadb-dump`, and running it reaches
//! the file MariaDB's `provides` map names — carrying the port of the instance it belongs to,
//! which is the half that stops a bare `mysql` from opening a session on the *other* product's
//! server on a home that has both.

mod harness;

use std::collections::BTreeMap;

use harness::Home;

/// What a MariaDB archive publishes, as the real one does on Windows: `mariadb` and not `mysql`.
const MARIADB: &[(&str, &str)] = &[
    ("mariadb", "bin/mariadb"),
    ("mariadb-admin", "bin/mariadb-admin"),
    ("mariadb-dump", "bin/mariadb-dump"),
    ("mariadbd", "bin/mariadbd"),
];

/// **The complaint, answered.** A home with MariaDB installed can dump a database from a terminal.
#[test]
fn a_database_client_is_a_command() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, Some(("mariadb@main", 3306)));

    let recorded = home.record_command("mariadb-dump", home.path(), &BTreeMap::new(), 0);

    assert!(
        recorded.reached,
        "mariadb-dump did not run: {}",
        recorded.run.stderr()
    );
}

/// **And the supervisor's own programs are not.** A shim in front of `mariadbd` would be a second
/// way to start a server nothing is watching, which is [`shims::COMMANDS`]' rule for `php-fpm` and
/// holds for every recipe.
#[test]
fn the_server_itself_is_not_a_command() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, Some(("mariadb@main", 3306)));

    let server = home
        .path()
        .join("bin")
        .join(format!("mariadbd{}", std::env::consts::EXE_SUFFIX));

    assert!(!server.exists(), "{} was fronted", server.display());
}

/// **MariaDB answers to `mysqldump` too**, because every tutorial, script and habit in the world
/// says so while the archive at 12.3 publishes `mariadb-dump` and no `mysqldump` at all.
#[test]
fn mariadb_answers_to_the_spelling_people_have_in_their_fingers() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, Some(("mariadb@main", 3306)));

    let recorded = home.record_command("mysqldump", home.path(), &BTreeMap::new(), 0);

    assert!(
        recorded.reached,
        "mysqldump did not run: {}",
        recorded.run.stderr()
    );
}

/// **A client is told where its own instance listens** — the design's D4, and the reason it is not
/// a nicety: the port allocator gives 3306 to whichever database was created first and the next
/// free port above to the other, so a client told nothing would connect to somebody else's server
/// and report success.
#[test]
fn a_client_is_told_the_port_its_instance_holds() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, Some(("mariadb@legacy", 3307)));

    let recorded = home.record_command("mariadb", home.path(), &BTreeMap::new(), 0);

    assert_eq!(recorded.recorded("MYSQL_TCP_PORT"), Some("3307"));
    assert_eq!(recorded.recorded("MYSQL_HOST"), Some("127.0.0.1"));
}

/// **And a variable the person set is theirs.** Somebody who exported `MYSQL_TCP_PORT` for a tunnel
/// meant it, and a tool that overrode it would be one that cannot be used against anything but
/// itself.
#[test]
fn a_port_somebody_exported_is_left_alone() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, Some(("mariadb@main", 3306)));

    let session = [("MYSQL_TCP_PORT", "9999".to_owned())]
        .into_iter()
        .collect();
    let recorded = home.record_command("mariadb", home.path(), &session, 0);

    assert_eq!(recorded.recorded("MYSQL_TCP_PORT"), Some("9999"));
}

/// **`MIXENGINE_MARIADB` names an instance outright**, which is the first rule of the order and the
/// only one a person can use to disagree with it.
#[test]
fn an_instance_can_be_named_for_one_command() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, Some(("mariadb@main", 3306)));
    home.instantiate("mariadb", "12.3.2", "mariadb@legacy", 3307);

    let session = [("MIXENGINE_MARIADB", "mariadb@legacy".to_owned())]
        .into_iter()
        .collect();
    let recorded = home.record_command("mariadb", home.path(), &session, 0);

    assert_eq!(recorded.recorded("MYSQL_TCP_PORT"), Some("3307"));
}

/// A variable naming no instance of this package is refused with the value in it, rather than
/// quietly ignored — a variable that does nothing is the confusion this mechanism exists to end.
#[test]
fn an_instance_that_is_not_there_is_refused_by_name() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, Some(("mariadb@main", 3306)));

    let session = [("MIXENGINE_MARIADB", "mariadb@nowhere".to_owned())]
        .into_iter()
        .collect();
    let recorded = home.record_command("mariadb", home.path(), &session, 0);

    assert!(!recorded.reached);
    assert!(
        recorded.run.stderr().contains("mariadb@nowhere"),
        "{}",
        recorded.run.stderr()
    );
}

/// **`mysqldump -h db.example.com` is a real use of a client.** A home with the package installed
/// and no instance of it still runs one, and is told nothing about an endpoint.
#[test]
fn a_package_with_no_instance_still_has_clients() {
    let home = Home::with(&["8.3.33"]);
    home.install_package("mariadb", "12.3.2", MARIADB, None);

    let recorded = home.record_command("mariadb-dump", home.path(), &BTreeMap::new(), 0);

    assert!(
        recorded.reached,
        "mariadb-dump did not run: {}",
        recorded.run.stderr()
    );
    assert_eq!(recorded.recorded("MYSQL_TCP_PORT"), None);
}

/// A name `bin/` holds and nothing claims any more says so, rather than announcing that it is a
/// MixEngine shim and listing nineteen runtime commands — which is what a person meets in the
/// moment between uninstalling a database and the next refresh.
#[test]
fn a_command_nothing_claims_any_more_says_so() {
    let home = Home::with(&["8.3.33"]);

    // Never installed at all, which from the shim's side is the same state an uninstall leaves.
    home.front("redis-cli");

    let recorded = home.record_command("redis-cli", home.path(), &BTreeMap::new(), 0);

    assert!(!recorded.reached);
    assert!(
        recorded.run.stderr().contains("nothing installed here"),
        "{}",
        recorded.run.stderr()
    );
}
