//! A first run: bootstrapped, started, queried, stopped, and the root credential's path.

use super::*;

/// **The whole of the recipe, in the order a user meets it.**
///
/// One test rather than eight, deliberately: each step is the previous one's precondition, and eight
/// tests would be eight real bootstraps performed to re-reach the state this one is already in.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real MariaDB — see the module note, and the `mariadb` step in _services.yml"]
async fn a_database_is_bootstrapped_started_queried_stopped_and_not_bootstrapped_twice() {
    let (home, _daemon, _registry, installed_at, port) = created().await;
    let data = data_directory(&home, "main");

    // --- started, which is where the bootstrap happens -------------------------------------------
    //
    // Nothing here asked for a first run. The start did, because the data directory is empty —
    // which is the half of a first run that is invisible from inside the daemon and obvious from
    // out here.
    at("starting the service, which is where the first run bootstraps");
    let started = expect(&home, &["service", "start", SERVICE, "--json"]);
    assert_eq!(
        started["complete"],
        true,
        "{started}\n{}",
        home.daemon_log()
    );

    let jobs = first_runs(&home);
    assert_eq!(
        jobs.len(),
        1,
        "a start with an empty data directory ran {} first-run jobs\n{}",
        jobs.len(),
        home.daemon_log()
    );
    assert_eq!(
        jobs[0]["state"],
        "succeeded",
        "{}\n{}",
        jobs[0],
        home.daemon_log()
    );

    // --- and left a data directory that says who made it -----------------------------------------
    let marker = std::fs::read_to_string(data.join(".mixengine-ready"))
        .expect("a finished ritual leaves its marker");
    assert_eq!(marker.trim(), VERSION, "the marker names another version");
    assert!(
        data.join("mysql").is_dir(),
        "the bootstrap left no system schema in {}",
        data.display()
    );

    // --- running, and proved so by a query rather than by an accept -------------------------------
    let up = status(&home, SERVICE);
    assert_eq!(up["state"], "running", "{up}\n{}", home.daemon_log());

    // **`running` is the assertion this whole suite exists for**, and it is worth saying why in the
    // place somebody will look for the query that used to be here. A TCP accept proves nothing — it
    // stays true for the whole of InnoDB's recovery, while the server refuses every statement. This
    // service's ready check is `mariadb-admin ping`, run by the daemon with the root password it
    // resolved out of the keyring at spawn. A store holding nothing, a store holding the wrong
    // value, a bootstrap that never set the password: none of them reach the line above.
    //
    // And the server says which build answered, in its own log.
    let said = server_log(&home);
    assert!(
        said.contains(VERSION),
        "the server that came up is not the one installed:\n{said}"
    );

    // **Root without a password is refused**, which is the other half of the same claim: the ping
    // above proves a password works, and this proves it is a password rather than a formality.
    at("offering the server a root login with no password");
    let refused = without_a_password(&installed_at, port, "root");
    assert!(
        refused.contains("'root'@"),
        "the server refused somebody other than root: {refused}"
    );

    // **The anonymous accounts are not asked about here, and the reason is worth writing down** —
    // an assertion was written, it passed, and it was measuring nothing. `mariadb-install-db`
    // creates `''@localhost` and `''@<hostname>`, and an anonymous row matches any user name that
    // is not otherwise defined, so the obvious test is to connect as somebody made up and expect a
    // refusal. Two things defeat it. The client will not send an empty user name — `--user=` falls
    // back to the login name, which is how the first version of this came to be refusing
    // `'haiqu'@'127.0.0.1'` and calling that proof. And this configuration says
    // `skip-name-resolve`, so a TCP connection's host is the literal `127.0.0.1`, which matches
    // neither `localhost` nor the hostname: those accounts could not let anybody in over this port
    // whether the `DELETE` ran or not.
    //
    // What would see them is a socket connection, which two of the three systems have. Reading
    // `mysql.global_priv` would too, and needs the credential this suite deliberately cannot read.
    // So the statement is covered where it is written instead — see the bootstrap test in
    // `recipes::mariadb` — and this suite claims only what a port can show.

    // --- a database and an account for it ---------------------------------------------------------
    //
    // **Nothing here reads a credential**, which is the whole reason the proof that the account's
    // password *works* is a step inside the daemon rather than an assertion here. See this file's
    // module note for what reading one costs on macOS.
    at("creating a database and an account for it");
    let created = expect(
        &home,
        &["database", "create", SERVICE, "--name", "blog", "--json"],
    );
    assert_eq!(created["made"]["database"], "created", "{created}");
    assert_eq!(created["made"]["user"], "created", "{created}");
    // **Both halves of the address.** The namespace is on the wire so that an application reading
    // this entry does not have to hardcode MixEngine's constant.
    assert_eq!(created["secret"]["service"], "mixengine", "{created}");

    // **And the key begins with this home's id** (ADR 0032). Asserted by shape rather than by
    // value, because the id is minted per home by `0001_initial.sql` and a suite that knew it would
    // be reading the home's database to find out. What matters here is that it is there: without it
    // this entry is the same one every other MIXENGINE_HOME on the machine writes to, and the run
    // before this one proved that the hard way by taking a working home's password.
    let key = created["secret"]["key"].as_str().expect("an address");
    let (home_id, rest) = key.split_once('/').unwrap_or_default();
    assert_eq!(rest, "mariadb@main/blog", "{created}");
    assert_eq!(home_id.len(), 12, "{created}");
    assert!(
        home_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{created}"
    );
    assert!(
        !created.to_string().contains("password"),
        "the answer carries the address of a credential and never the credential: {created}"
    );

    // **Asking twice changes nothing**, which is what makes a failed apply resumable rather than
    // restartable — and what `blueprint.apply` leans on.
    at("creating the same database a second time");
    let again = expect(
        &home,
        &["database", "create", SERVICE, "--name", "blog", "--json"],
    );
    assert_eq!(again["made"]["database"], "existing", "{again}");
    assert_eq!(again["made"]["user"], "existing", "{again}");

    // And the new account needs its password, the way root does: the same negative this suite makes
    // about root, aimed at the account the daemon just made.
    at("offering the server the new account with no password");
    let refused = without_a_password(&installed_at, port, "blog");
    assert!(
        refused.contains("'blog'@"),
        "the server refused somebody other than the new account: {refused}"
    );

    // A name no statement would take is refused before anything is created.
    at("asking for a database name that is not one");
    let bad = home.mix(&[
        "database",
        "create",
        SERVICE,
        "--name",
        "Blog; DROP",
        "--json",
    ]);
    assert!(
        !bad.status.success(),
        "a name outside the slug charset was accepted: {}",
        harness::stdout(&bad)
    );

    // --- a chosen password ---------------------------------------------------------------------
    //
    // Success against the real server *is* the proof the daemon stored and used exactly the
    // password given — the reasoning above, extended to a value nobody generated.
    at("creating an account with a chosen password");
    let chosen = expect(
        &home,
        &[
            "database",
            "create",
            SERVICE,
            "--name",
            "shop",
            "--user",
            "shop-app",
            "--password",
            "Ch0sen!Pw",
            "--json",
        ],
    );
    assert_eq!(chosen["made"]["user"], "created", "{chosen}");
    with_the_password(&installed_at, port, "shop-app", "Ch0sen!Pw");

    at("reading it back through mix database credentials");
    let read_back = expect(
        &home,
        &[
            "database",
            "credentials",
            SERVICE,
            "--user",
            "shop-app",
            "--json",
        ],
    );
    assert_eq!(read_back["password"], "Ch0sen!Pw", "{read_back}");

    at("changing an existing account's password");
    let changed = expect(
        &home,
        &[
            "database",
            "create",
            SERVICE,
            "--name",
            "shop",
            "--user",
            "shop-app",
            "--password",
            "N3wPassword",
            "--json",
        ],
    );
    assert_eq!(changed["made"]["user"], "existing", "{changed}");
    with_the_password(&installed_at, port, "shop-app", "N3wPassword");
    let stale = without_a_password(&installed_at, port, "shop-app");
    assert!(
        stale.contains("'shop-app'@"),
        "the old password should no longer be the one that answers: {stale}"
    );

    at("the new password reads back and MixEngine holds no memory of the old one");
    let after_change = expect(
        &home,
        &[
            "database",
            "credentials",
            SERVICE,
            "--user",
            "shop-app",
            "--json",
        ],
    );
    assert_eq!(after_change["password"], "N3wPassword", "{after_change}");

    at("a password with a character the validator refuses is rejected before anything starts");
    let refused_password = home.mix(&[
        "database",
        "create",
        SERVICE,
        "--name",
        "nope",
        "--password",
        "has'quote",
    ]);
    assert!(
        !refused_password.status.success(),
        "a password outside the accepted alphabet was accepted: {}",
        harness::stdout(&refused_password)
    );

    at("the administrator's own password reads back too");
    let root = expect(&home, &["database", "credentials", SERVICE, "--json"]);
    assert_eq!(root["user"], "root", "{root}");
    assert!(
        !root["password"].as_str().unwrap_or_default().is_empty(),
        "{root}"
    );

    // --- stopped, cleanly ------------------------------------------------------------------------
    at("stopping the service");
    let stopped = expect(&home, &["service", "stop", SERVICE, "--json"]);
    assert_eq!(
        stopped["complete"],
        true,
        "{stopped}\n{}",
        home.daemon_log()
    );

    // **Proof, rather than a belief about an exit code.** A `mariadbd` that was terminated exits
    // zero and leaves a dirty buffer pool; only the server's own log says the shutdown finished.
    let said = server_log(&home);
    assert!(
        said.contains("Shutdown complete"),
        "the server was stopped rather than asked to shut down:\n{said}"
    );

    // --- started a second time, and not bootstrapped again ----------------------------------------
    at("starting it a second time, which must not bootstrap again");
    let again = expect(&home, &["service", "start", SERVICE, "--json"]);
    assert_eq!(again["complete"], true, "{again}\n{}", home.daemon_log());

    assert_eq!(
        first_runs(&home).len(),
        1,
        "a second start bootstrapped the data directory again\n{}",
        home.daemon_log()
    );

    // The credential survived the restart, said the way the first start said it: a service that
    // reaches `running` has answered an authenticated ping, and this one was never bootstrapped a
    // second time to re-write the password it answered with.
    let again = status(&home, SERVICE);
    assert_eq!(
        again["state"],
        "running",
        "the credential did not survive a restart\n{again}\n{}",
        home.daemon_log()
    );

    // --- and a data directory that is not ours is refused, not cleaned ----------------------------
    //
    // The markers are what make a directory ours, so removing them is exactly what a user pointing
    // this service at a database they already had looks like from in here.
    let stopped = expect(&home, &["service", "stop", SERVICE, "--json"]);
    assert_eq!(stopped["complete"], true, "{stopped}");

    std::fs::remove_file(data.join(".mixengine-ready")).expect("the marker is there");
    // Beside the directory rather than inside it — see `first_run::STARTED_MARKER`, and the Windows
    // bootstrapper that will not touch a datadir with anything at all in it.
    let _ = std::fs::remove_file(data.with_file_name("main.mixengine-init-started"));

    // `expect` would be wrong here: this call is *meant* to fail, and what is asserted is the exit
    // status rather than a field — the walk answers `"complete": true` for a plan it finished
    // walking, and names the service it could not start under `failed`.
    at("starting over a data directory MixEngine did not create");
    let refused = home.mix(&["service", "start", SERVICE, "--json"]);
    let said = harness::stdout(&refused);
    assert!(
        !refused.status.success(),
        "a database MixEngine did not create was bootstrapped over\n{said}\n{}",
        home.daemon_log()
    );
    assert!(
        said.contains("was not created by MixEngine"),
        "the start failed for some other reason: {said}"
    );
    assert!(
        data.join("mysql").is_dir(),
        "the refusal removed somebody's database"
    );
}

/// **The only test in the workspace in which a real credential reaches a real process through the
/// handoff**, on the window since it became the only desktop client. A copy of the daemon runs out
/// of a directory of this test's own, with a script named as MixLab's window beside it; the script
/// records its first argument and *whether* the variable is set — never its value, which is the
/// module note's rule at one more address.
///
/// Linux only: a script can be an executable named `mixlab` there, where Windows needs an `.exe`
/// and macOS a bundle.
///
/// # Why this service is not `mariadb@main`
///
/// A name of its own, so this test and the one above, which run at once in two homes, read apart in
/// a log. It used to carry weight. A Unix bootstrap named two things after the service id alone:
/// the keyring entry the root password lives in, until addresses named their home, and the
/// space-free view `/tmp/mixengine-init-<id>`. Two homes bootstrapping one id shared the view, and
/// the second ritual's first step, `rm -rf` on it, killed the first with `[ERROR] Aborting`. Both
/// names now carry the home id.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real MariaDB — see the module note, and the `mariadb` step in _services.yml"]
async fn the_root_credential_reaches_the_client_through_its_environment_and_not_the_url() {
    use std::os::unix::fs::PermissionsExt as _;

    /// The service this test drives — see the note above for why it is not [`SERVICE`].
    const HANDOFF: &str = "mariadb@handoff";

    // The fake window: a script named as this install's window, beside a copy of the daemon — the
    // one place `locate_window` looks on Linux — and the file it reports into.
    let data = tempfile::tempdir().expect("a directory for the fake install");
    let record = data.path().join("received.txt");
    let install = data.path().join("bin");
    std::fs::create_dir_all(&install).expect("the install directory");
    let window = install.join("mixlab");
    std::fs::write(
        &window,
        format!(
            "#!/bin/sh\nprintf 'url=%s\\n' \"$1\" > '{0}'\n\
             if [ -n \"$MIXENGINE_DB_PASSWORD\" ]; then echo 'password=present' >> '{0}'; \
             else echo 'password=absent' >> '{0}'; fi\n",
            record.display()
        ),
    )
    .expect("the script");
    std::fs::set_permissions(&window, std::fs::Permissions::from_mode(0o755)).expect("executable");
    let daemon = harness::daemon_installed_in(&install);

    // `created()`'s steps, with a daemon run from the fake install.
    let root = package();
    let port = free_port();
    let packed = packed();
    let registry = MockRegistry::start(&serde_json::json!({
        "schema": 1, "generated_at": "2026-08-19T06:55:12Z", "packages": []
    }))
    .await;
    let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
    registry.publish(&index(&packed, &url, provides(&root)));

    let home = Home::new();
    let _daemon =
        home.start_daemon_from_reading_index(&daemon, &registry.url(), registry.public_key());
    watch(&home);

    at("installing the package");
    expect(&home, &["package", "install", "mariadb", VERSION, "--json"]);

    at("creating the service");
    expect(
        &home,
        &[
            "service",
            "create",
            HANDOFF,
            VERSION,
            "--port",
            &port.to_string(),
            "--json",
        ],
    );

    at("opening the database in the fake window, which starts the server and its first run");
    let opened = expect(&home, &["database", "open", HANDOFF, "--json"]);
    assert_eq!(opened["launched"]["launch"], "handed_on", "{opened}");
    assert_eq!(opened["secret"]["service"], "mixengine", "{opened}");
    let handed = opened["secret"]["key"].as_str().expect("an address");
    assert!(
        is_this_homes_address(handed, "mariadb@handoff/root"),
        "{opened}"
    );
    assert_eq!(opened["client"]["state"], "installed", "{opened}");

    let received = std::fs::read_to_string(&record).expect("the script ran and wrote");
    assert!(
        received.contains("url=mixlab://connect?kind=mysql&host=127.0.0.1&port="),
        "{received}"
    );
    assert!(
        received.contains(&format!(
            "port={port}&user=root&label=mariadb%40handoff&password_env=MIXENGINE_DB_PASSWORD\
             &secret_key={encoded}",
            encoded = handed.replace('@', "%40").replace('/', "%2F")
        )),
        "{received}"
    );
    // The key half is on the wire so a saved connection can point at MixEngine's entry; the
    // namespace never is, because a `mixlab://` URL is something a web page can produce and a
    // namespace on it would name any secret on the machine.
    assert!(!received.contains("secret_service"), "{received}");
    assert!(received.contains("password=present"), "{received}");
    assert!(!received.contains("password=absent"), "{received}");

    at("stopping the service");
    expect(&home, &["service", "stop", HANDOFF, "--json"]);
}
