use super::*;

/// **Nothing the supervisor runs is a command anybody can type** — roadmap task **T130**.
///
/// [`crate::shims::COMMANDS`] states this for `php-fpm`, and a recipe is the other place it can
/// be broken: a shim in front of `mariadbd` would be a second way to start a server nothing is
/// watching, and one in front of `initdb` would be a way to write over a live cluster.
#[test]
fn no_recipe_fronts_a_server_or_a_bootstrapper() {
    let owned = [
        "mariadbd",
        "mysqld",
        "postgres",
        "redis-server",
        "caddy",
        "nginx",
        "memcached",
        "mongod",
        "mongos",
        "php-fpm",
        "initdb",
        "pg_ctl",
        "pg_upgrade",
        "mariadb-install-db",
        "mysql_install_db",
    ];

    let catalogue = super::super::Catalogue::builtin();

    for package in catalogue.packages() {
        let recipe = catalogue.recipe(package).expect("a listed recipe");

        for client in recipe.clients() {
            assert!(
                !owned.contains(&client.executable),
                "{package} fronts {}, which the supervisor owns",
                client.executable
            );
        }
    }
}

/// **MariaDB is the only recipe with a compatibility spelling**, and each of its three stands
/// for a program MariaDB itself publishes — an alias for an executable no artifact contains
/// would be a name in `bin/` that resolves to nothing.
#[test]
fn mariadb_is_the_only_recipe_that_stands_in_for_another_product() {
    let catalogue = super::super::Catalogue::builtin();

    for package in catalogue.packages() {
        let recipe = catalogue.recipe(package).expect("a listed recipe");

        let aliases: Vec<(&str, &str)> = recipe
            .clients()
            .iter()
            .filter(|client| client.claim == Claim::Alias)
            .map(|client| (client.name, client.executable))
            .collect();

        if package != "mariadb" {
            assert!(
                aliases.is_empty(),
                "{package} claims a name it does not own"
            );
            continue;
        }

        assert_eq!(
            aliases,
            vec![
                ("mysql", "mariadb"),
                ("mysqladmin", "mariadb-admin"),
                ("mysqldump", "mariadb-dump"),
            ]
        );

        let own: Vec<&str> = recipe
            .clients()
            .iter()
            .filter(|client| client.claim == Claim::Own)
            .map(|client| client.executable)
            .collect();

        for (name, executable) in aliases {
            assert!(
                own.contains(&executable),
                "{name} stands for {executable}, which mariadb does not publish"
            );
        }
    }
}

/// One package must not claim one name twice, and a client's name must be something a file can
/// be called: both would be found by `bin/` rather than here, and much later.
#[test]
fn a_package_claims_each_name_once() {
    let catalogue = super::super::Catalogue::builtin();

    for package in catalogue.packages() {
        let recipe = catalogue.recipe(package).expect("a listed recipe");
        let mut seen = std::collections::BTreeSet::new();

        for client in recipe.clients() {
            assert!(
                seen.insert(client.name),
                "{package} claims {} twice",
                client.name
            );
            assert!(
                !client.name.contains(['/', '\\', '.']),
                "{} is not a file name",
                client.name
            );
        }
    }
}

/// **A client is told where its own instance listens** — roadmap task **T130**, the design's
/// D4, which exists because the port allocator gives 3306 to whichever of the two MySQL-family
/// products asks first and the next free port above to the other.
#[test]
fn a_client_is_told_where_its_instance_listens() {
    let catalogue = super::super::Catalogue::builtin();
    let listen = Upstream::Tcp("127.0.0.1:3307".parse().expect("a socket address"));

    let mariadb = catalogue
        .recipe("mariadb")
        .expect("a builtin recipe")
        .client_env(&listen);
    assert_eq!(
        mariadb.get("MYSQL_HOST").map(String::as_str),
        Some("127.0.0.1")
    );
    assert_eq!(
        mariadb.get("MYSQL_TCP_PORT").map(String::as_str),
        Some("3307")
    );

    let mysql = catalogue
        .recipe("mysql")
        .expect("a builtin recipe")
        .client_env(&listen);
    assert_eq!(
        mysql, mariadb,
        "two products on one wire protocol read one pair of variables"
    );

    let postgres = catalogue
        .recipe("postgres")
        .expect("a builtin recipe")
        .client_env(&listen);
    assert_eq!(postgres.get("PGPORT").map(String::as_str), Some("3307"));

    // Redis publishes a client and no environment for it to read. An empty map is the honest
    // answer; a variable of our own invention would be one `redis-cli` ignores.
    assert!(
        catalogue
            .recipe("redis")
            .expect("a builtin recipe")
            .client_env(&listen)
            .is_empty()
    );
}

/// A socket is told to nobody: no client family here has a variable for one, and a recipe that
/// answered with a TCP port for a socket would be pointing a client at a listener that is not
/// there.
#[test]
fn a_socket_is_told_to_nobody() {
    let catalogue = super::super::Catalogue::builtin();
    let listen = Upstream::Socket(std::path::PathBuf::from("/run/mariadb.sock"));

    for package in catalogue.packages() {
        let recipe = catalogue.recipe(package).expect("a listed recipe");
        assert!(
            recipe.client_env(&listen).is_empty(),
            "{package} named a variable for a socket"
        );
    }
}
