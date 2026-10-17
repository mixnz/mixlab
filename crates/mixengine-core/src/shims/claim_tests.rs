use super::*;

use crate::generate::recipe::Claim;

fn claimed(name: &str, package: &str, claim: Claim) -> Claimed {
    Claimed {
        name: name.to_owned(),
        package: package.to_owned(),
        claim,
        has_instance: false,
        on_preferred_port: false,
    }
}

fn running(name: &str, package: &str, claim: Claim, on_preferred_port: bool) -> Claimed {
    Claimed {
        has_instance: true,
        on_preferred_port,
        ..claimed(name, package, claim)
    }
}

fn won(claims: &[Claimed], name: &str) -> String {
    let (_, conflicts) = resolve_claims(claims);

    conflicts
        .iter()
        .find(|conflict| conflict.name == name)
        .map(|conflict| conflict.won.clone())
        .expect("a conflict over this name")
}

/// **A real name beats a compatibility one**, which is the only conflict this build can produce:
/// MariaDB's `mysql` disappears the moment somebody installs the product that owns the name.
#[test]
fn a_real_name_beats_a_compatibility_one() {
    // MariaDB is running on 3306 and MySQL is not installed anywhere near a port, so every
    // other term of the order points the other way. The claim still decides.
    let claims = [
        running("mysql", "mariadb", Claim::Alias, true),
        claimed("mysql", "mysql", Claim::Own),
    ];

    assert_eq!(won(&claims, "mysql"), "mysql");
}

/// Two real claims — a MariaDB 10.x archive still ships a `mysql` of its own — are settled by
/// the instance: the product somebody is actually running is the one they meant.
#[test]
fn two_real_claims_are_settled_by_the_instance() {
    let claims = [
        claimed("mysql", "mariadb", Claim::Own),
        running("mysql", "mysql", Claim::Own, false),
    ];

    assert_eq!(won(&claims, "mysql"), "mysql");
}

/// Both running, and the product's own documented port decides — the same rule
/// [`crate::services::client`] uses, for the same reason.
#[test]
fn both_running_and_the_documented_port_decides() {
    let claims = [
        running("mysql", "mariadb", Claim::Own, true),
        running("mysql", "mysql", Claim::Own, false),
    ];

    assert_eq!(won(&claims, "mysql"), "mariadb");
}

/// Still tied, and the answer is the package name ascending: arbitrary, and the same on every
/// machine and on every run, which is the only property left that matters.
#[test]
fn a_total_tie_ends_in_the_package_name() {
    let claims = [
        running("mysql", "mysql", Claim::Own, true),
        running("mysql", "mariadb", Claim::Own, true),
    ];

    assert_eq!(won(&claims, "mysql"), "mariadb");
}

/// **The losers are named rather than dropped.** Somebody who typed `mysql` and reached the
/// other product's client has to be able to find that out without reading `--version`.
#[test]
fn the_losers_are_named() {
    let claims = [
        claimed("mysql", "mariadb", Claim::Alias),
        claimed("mysql", "mysql", Claim::Own),
    ];

    let (extras, conflicts) = resolve_claims(&claims);

    assert_eq!(
        extras,
        vec![Extra {
            name: "mysql".to_owned(),
            origin: Origin::Client {
                package: "mysql".to_owned()
            },
        }]
    );
    assert_eq!(
        conflicts,
        vec![Conflict {
            name: "mysql".to_owned(),
            won: "mysql".to_owned(),
            lost: vec!["mariadb".to_owned()],
        }]
    );
}

/// A name only one package claims is not a conflict, and saying so would put every command in
/// `bin/` into a report meant for the two that are contested.
#[test]
fn an_uncontested_name_is_not_a_conflict() {
    let claims = [
        claimed("psql", "postgres", Claim::Own),
        claimed("redis-cli", "redis", Claim::Own),
    ];

    let (extras, conflicts) = resolve_claims(&claims);

    assert_eq!(extras.len(), 2);
    assert!(conflicts.is_empty());
}
