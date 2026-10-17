//! What is kept: databases across a credential reset, an adoption and an update.

use super::*;

/// Set the root password in this data directory to something MixEngine does not know.
///
/// **The T126 collision, reproduced from the server's side rather than the keyring's.** The outage
/// was the two copies of one password coming apart; which of them moved does not matter to anything
/// downstream, and moving *this* one keeps the test process out of the credential store — which the
/// module note explains at length is a thing this suite may not touch.
///
/// It is the recipe's own bootstrap statement, run the way the recipe runs it: a server that listens
/// on nothing, reading SQL on standard input.
fn set_the_root_password_behind_mixengines_back(root: &Path, data: &Path, password: &str) {
    let sql = format!(
        "USE mysql;\n\
         UPDATE global_priv SET priv = JSON_SET(priv, '$.plugin', 'mysql_native_password', \
         '$.authentication_string', PASSWORD('{password}')) WHERE User = 'root';\n"
    );
    let script = std::env::temp_dir().join("mixengine-t127-collision.sql");
    std::fs::write(&script, sql).expect("a statement to feed the server");

    let input = std::fs::File::open(&script).expect("the statement is readable");
    let ran = Command::new(root.join("bin").join("mariadbd"))
        .args([
            "--no-defaults".to_owned(),
            "--bootstrap".to_owned(),
            format!("--basedir={}", root.display()),
            format!("--datadir={}", data.display()),
        ])
        .stdin(input)
        .output()
        .expect("the server runs in bootstrap mode");

    let _ = std::fs::remove_file(&script);

    assert!(
        ran.status.success(),
        "could not change the password behind MixEngine's back: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

/// **A credential nothing can produce any more is re-set by a command, and the databases are kept**
/// — roadmap task **T127**, and the outage T126 was reported from.
///
/// The last assertion is the one that matters. Every other line here would also pass for a repair
/// that threw the data directory away and bootstrapped a new one, which is the thing this command
/// must never be.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real MariaDB — see the module note, and the `mariadb` step in _services.yml"]
async fn a_superuser_credential_is_re_set_and_the_databases_are_kept() {
    let (home, _daemon, _registry, installed_at, _port) = created_as(RESET).await;
    let data = data_directory(&home, "reset");
    watch(&home);

    at("starting the service and making a database in it");
    expect(&home, &["service", "start", RESET, "--json"]);
    let made = expect(
        &home,
        &["database", "create", RESET, "--name", "shop", "--json"],
    );
    assert_eq!(made["made"]["database"], "created", "{made}");

    at("stopping it, and moving the server's own copy of the password");
    expect(&home, &["service", "stop", RESET, "--json"]);
    set_the_root_password_behind_mixengines_back(
        &installed_at,
        &data,
        "T127T127T127T127T127T127T127T127",
    );

    // --- and now nothing on this machine can log in ----------------------------------------------
    at("asking for a database, which is refused and says what the repair is");
    let refused = home.mix(&["database", "create", RESET, "--name", "blog", "--json"]);
    assert!(
        !refused.status.success(),
        "the server accepted a password it no longer has"
    );

    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("mix service reset-credential mariadb@reset"),
        "the refusal does not name the repair: {said}"
    );

    // --- the repair ------------------------------------------------------------------------------
    at("re-setting the credential");
    let walk = expect(
        &home,
        &["service", "reset-credential", RESET, "--yes", "--json"],
    );
    assert_eq!(walk["complete"], true, "{walk}\n{}", home.daemon_log());
    assert_eq!(walk["failed"], Value::Null, "{walk}\n{}", home.daemon_log());

    // --- which is proved by the service being up, not by an exit code ----------------------------
    //
    // `running` is reached through `mariadb-admin ping`, authenticated with the password the daemon
    // resolves out of the keyring at spawn. A repair that wrote the wrong value, or none, cannot
    // reach this line.
    let up = status(&home, RESET);
    assert_eq!(up["state"], "running", "{up}\n{}", home.daemon_log());

    // --- **and `shop` is still there** ------------------------------------------------------------
    at("checking that the database made before the repair survived it");
    let again = expect(
        &home,
        &["database", "create", RESET, "--name", "shop", "--json"],
    );
    assert_eq!(
        again["made"]["database"], "existing",
        "the repair discarded the data directory: {again}"
    );
}

/// **T182f and T182g, the machine they were reported from.** A MariaDB with a database in it is
/// kept on disk while the home's database goes — an uninstall that kept its folders, then a fresh
/// install pointed at them. The next daemon records the package on its own, lists the data as
/// found, and `mix service adopt` brings it back with a new admin password: the service starts, and
/// the database made before is still there.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real MariaDB — see the module note, and the `mariadb` step in _services.yml"]
async fn service_data_an_earlier_home_left_is_adopted_and_keeps_its_databases() {
    let (home, daemon, registry, _installed_at, _port) = created_as(KEPT).await;
    watch(&home);

    at("starting the service and making a database in it");
    expect(&home, &["service", "start", KEPT, "--json"]);
    let made = expect(
        &home,
        &["database", "create", KEPT, "--name", "shop", "--json"],
    );
    assert_eq!(made["made"]["database"], "created", "{made}");
    expect(&home, &["service", "stop", KEPT, "--json"]);

    // --- the reinstall: the home's database goes, the folders stay -------------------------------
    at("stopping the daemon and removing the home's database");
    drop(daemon);
    for name in ["mixengine.db", "mixengine.db-wal", "mixengine.db-shm"] {
        let _ = std::fs::remove_file(home.path().join(name));
    }
    let _daemon = home.start_daemon_reading_index(&registry.url(), registry.public_key());
    watch(&home);

    // --- the package is recorded on its own (T182f), the data is found (T182g) -------------------
    at("listing what the earlier home left");
    let found = expect(&home, &["service", "found", "--json"]);
    let row = found["found"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["service"] == KEPT))
        .unwrap_or_else(|| panic!("{KEPT} is not listed: {found}\n{}", home.daemon_log()))
        .clone();
    assert_eq!(row["opens_with"], VERSION, "{row}");

    at("adopting it");
    let adopted = expect(&home, &["service", "adopt", KEPT, "--json"]);
    assert_eq!(
        adopted["state"],
        "stopped",
        "{adopted}\n{}",
        home.daemon_log()
    );

    // --- proved by the service being up, which `mariadb-admin ping` with the new password is -----
    at("starting it with the password the adopt set");
    expect(&home, &["service", "start", KEPT, "--json"]);
    let up = status(&home, KEPT);
    assert_eq!(up["state"], "running", "{up}\n{}", home.daemon_log());

    // Under another account: `shop`'s password was in the earlier home's credential store, which an
    // uninstall empties, so MixEngine refuses to manage that account — and saying so is itself proof
    // the account survived. The application that used it keeps its own copy of the password.
    at("checking that the database made before the reinstall survived it");
    let again = expect(
        &home,
        &[
            "database", "create", KEPT, "--name", "shop", "--user", "adopted", "--json",
        ],
    );
    assert_eq!(
        again["made"]["database"], "existing",
        "the adopt lost the data: {again}"
    );
}

/// The patch after [`VERSION`], published from the same archive — roadmap task **T193c**. A real
/// second patch of one line is not on the machine, and what is under test is the move, not the build.
const NEXT: &str = "11.4.13";

/// **T193c, added at execution.** An instance updated within its line keeps its databases: the
/// account made before the update logs in afterwards, through the new version's own client.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real MariaDB — see the module note, and the `mariadb` step in _services.yml"]
async fn an_instance_updated_within_its_line_keeps_its_databases() {
    let root = package();
    let port = free_port();
    let packed = packed();

    let registry = MockRegistry::start(&serde_json::json!({
        "schema": 1, "generated_at": "2026-09-29T06:55:12Z", "packages": []
    }))
    .await;
    let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
    let mut published = index(&packed, &url, provides(&root));
    let mut next = published["packages"][0].clone();
    next["version"] = Value::from(NEXT);
    published["packages"]
        .as_array_mut()
        .expect("the index has a package list")
        .push(next);
    registry.publish(&published);

    let home = Home::new();
    let _daemon = home.start_daemon_reading_index(&registry.url(), registry.public_key());
    watch(&home);

    at("installing the package");
    let installed = expect(&home, &["package", "install", "mariadb", VERSION, "--json"]);
    assert_eq!(
        installed["state"],
        "succeeded",
        "{installed}\n--- daemon ---\n{}",
        home.daemon_log()
    );

    at("creating and starting the service");
    expect(
        &home,
        &[
            "service",
            "create",
            SERVICE,
            VERSION,
            "--port",
            &port.to_string(),
            "--json",
        ],
    );
    let started = expect(&home, &["service", "start", SERVICE, "--json"]);
    assert_eq!(
        started["complete"],
        true,
        "{started}\n--- server ---\n{}\n--- daemon ---\n{}",
        server_log(&home),
        home.daemon_log()
    );

    at("making a database and its account");
    let made = expect(
        &home,
        &[
            "database",
            "create",
            SERVICE,
            "--name",
            "blog",
            "--user",
            "blog-app",
            "--password",
            "Ch0sen!Pw",
            "--json",
        ],
    );
    assert_eq!(made["made"]["database"], "created", "{made}");

    at("updating the package within its line");
    let finished = expect(
        &home,
        &["package", "upgrade", "mariadb", VERSION, "--yes", "--json"],
    );
    assert_eq!(
        finished["state"],
        "succeeded",
        "{finished}\n--- server ---\n{}\n--- daemon ---\n{}",
        server_log(&home),
        home.daemon_log()
    );
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "removed",
        "{finished}"
    );

    let up = status(&home, SERVICE);
    assert_eq!(
        up["state"],
        "running",
        "{up}\n--- server ---\n{}\n--- daemon ---\n{}",
        server_log(&home),
        home.daemon_log()
    );

    at("logging in with the account made before the update");
    let after = home.path().join("packages").join("mariadb").join(NEXT);
    with_the_password(&after, port, "blog-app", "Ch0sen!Pw");

    at("stopping the service");
    expect(&home, &["service", "stop", SERVICE, "--json"]);
}
