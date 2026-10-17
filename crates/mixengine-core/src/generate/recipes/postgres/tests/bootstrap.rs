//! The credential, the bootstrap ritual, a reset and provisioning.

use super::*;

/// A context carrying the credential the daemon would have generated.
fn initialised(secret: &str) -> Context {
    let mut context = context("{}");
    context.put_secret(SUPERUSER, secret);

    context
}

/// A recipe that declares a credential also declares what to do with it.
#[test]
fn postgres_declares_the_one_credential_its_ritual_needs() {
    let ritual = Postgres.ritual().expect("a database has a first run");

    assert_eq!(ritual.secrets.len(), 1, "{:?}", ritual.secrets);
    assert_eq!(ritual.secrets[0].key, SUPERUSER);
    assert!(ritual.secrets[0].length >= 32, "{:?}", ritual.secrets);
}

/// **The superuser is readable from a provisioning, because a refusal has to be attributed** —
/// roadmap task **T127a**. A server's log says who was refused, and this is what tells a line
/// about *this home's* superuser apart from one about somebody's application account.
#[test]
fn a_provisioning_says_which_account_its_superuser_is() {
    let context = context("{}");
    let admin = Postgres.databases().expect("a database vocabulary");

    let provisioning = crate::generate::databases::Provisioning::new(&context, admin);

    assert_eq!(provisioning.root_user(), SUPERUSER);
    // The two answers are about one account, which is what makes a log line attributable to the
    // credential this home holds rather than merely to somebody.
    assert!(
        provisioning.root_address().ends_with(SUPERUSER),
        "{}",
        provisioning.root_address()
    );
}

/// The keyring entry the spec names and the one the ritual is stored under are one address.
///
/// Composed twice, and the failure when they disagree is a server that starts and a client that
/// cannot authenticate against it — reported as a service that never became ready.
#[test]
fn the_spec_and_the_ritual_name_one_credential() {
    let context = context("{}");
    let spec = built("{}");

    let named = spec
        .env()
        .get(PASSWORD_VARIABLE)
        .expect("the spec names a credential");

    assert!(
        matches!(named, mixengine_proto::EnvValue::Keyring { key, .. }
            if key == &context.secret_address(SUPERUSER)),
        "{named:?}"
    );
    // The home is in front of the service — roadmap task **T126**, and MariaDB's own test
    // says why.
    assert_eq!(
        context.secret_address(SUPERUSER),
        format!(
            "{}/postgres@main/postgres",
            crate::generate::context::TEST_HOME
        )
    );
}

/// **`initdb` is told its locale and its encoding, because otherwise it reads the machine's.**
///
/// Measured in `mixengine-packages`: on a machine whose system locale it does not recognise it
/// reports *could not find suitable text search configuration*, sets the default to `simple`,
/// and **exits zero**. Two developers, two databases that answer differently.
///
/// And `reject` rather than `scram-sha-256`, which `initdb` refuses without a password to go
/// with it — see [`create_the_cluster`].
#[test]
fn the_cluster_is_created_with_a_locale_this_recipe_chose() {
    let steps = steps(&initialised("hunter2")).expect("steps");
    let initdb = &steps[0];

    assert!(
        initdb.args.iter().any(|arg| arg == "--locale=C"),
        "{:?}",
        initdb.args
    );
    assert!(
        initdb.args.iter().any(|arg| arg == "--encoding=UTF8"),
        "{:?}",
        initdb.args
    );
    assert!(
        initdb.args.iter().any(|arg| arg == "--auth-local=reject"),
        "{:?}",
        initdb.args
    );
    assert!(
        initdb.args.iter().any(|arg| arg == "--auth-host=reject"),
        "{:?}",
        initdb.args
    );
}

/// And an override of either reaches it, because a user who wants their production collation
/// says so — and owns the per-OS spelling when they do.
#[test]
fn a_stated_locale_reaches_initdb() {
    let mut context = context(r#"{"locale": "en_US.UTF-8"}"#);
    context.put_secret(SUPERUSER, "hunter2");

    let steps = steps(&context).expect("steps");

    assert!(
        steps[0]
            .args
            .iter()
            .any(|arg| arg == "--locale=en_US.UTF-8"),
        "{:?}",
        steps[0].args
    );
}

/// **No `--pwfile`, and the reason is the whole shape of this ritual.**
///
/// `initdb` will only take a password from a file, and a file is a plaintext superuser
/// credential on disk for the whole of a bootstrap that can take minutes — and one a half-failed
/// ritual leaves behind. So the role is created with **no** password and the password is set
/// through single-user mode, which opens no port and no socket: there is no instant at which a
/// password-less superuser is reachable by anybody.
#[test]
fn the_password_is_set_through_a_server_listening_on_nothing() {
    let steps = steps(&initialised("hunter2")).expect("steps");

    assert!(
        !steps
            .iter()
            .any(|step| step.args.iter().any(|arg| arg.starts_with("--pwfile"))),
        "a plaintext credential on disk: {steps:?}"
    );

    let single = steps
        .iter()
        .find(|step| step.stdin.is_some())
        .expect("one step speaks SQL");

    assert!(
        single.args.iter().any(|arg| arg == "--single"),
        "{:?}",
        single.args
    );
    assert!(
        single
            .program
            .ends_with(format!("postgres{}", std::env::consts::EXE_SUFFIX)),
        "{:?}",
        single.program
    );

    let sql = single.stdin.as_deref().expect("the SQL is on stdin");
    assert!(
        sql.contains("ALTER ROLE postgres PASSWORD 'hunter2';"),
        "{sql}"
    );

    // Single-user mode reads one statement per line, so a statement wrapped over two would be
    // two statements and neither of them valid.
    for line in sql.lines().filter(|line| !line.trim().is_empty()) {
        assert!(
            line.trim_end().ends_with(';'),
            "a wrapped statement: {line}"
        );
    }
}

/// A credential that would need escaping is refused rather than escaped.
///
/// `mixengine_platform::generate_secret` restricts its alphabet for exactly the interpolation
/// above; this is the other end of that arrangement.
#[test]
fn a_credential_that_would_need_escaping_is_refused() {
    let error = steps(&initialised("a'b")).expect_err("that cannot go in a SQL literal");

    assert!(
        matches!(&error, Error::Service(ServiceError::SettingValue { value, .. }) if !value.contains("a'b")),
        "the refusal quoted the credential: {error:?}"
    );
}

/// And so is no credential at all, which is a recipe that declared one and never got it.
#[test]
fn no_credential_at_all_is_refused_rather_than_setting_an_empty_password() {
    let error = steps(&context("{}")).expect_err("an empty password is not a password");

    assert!(
        matches!(error, Error::Service(ServiceError::SettingValue { .. })),
        "{error:?}"
    );
}

/// Two steps, the same two, on every system — which is the difference from MariaDB, whose
/// bootstrap is two different programs and four steps on Unix.
#[test]
fn the_ritual_is_two_steps_everywhere() {
    let steps = steps(&initialised("abc123")).expect("steps");

    assert_eq!(
        steps.len(),
        2,
        "{:?}",
        steps.iter().map(|step| &step.label).collect::<Vec<_>>()
    );
    assert!(
        steps.iter().all(|step| step.program.is_absolute()),
        "a relative program is whatever the PATH says at the moment it runs: {steps:?}"
    );
}

/// An install missing `initdb` is named, rather than failing at the moment of the first run.
#[test]
fn a_postgres_that_cannot_bootstrap_is_named() {
    let mut without = provides();
    without.remove("initdb");

    let mut context = with_provides(without, "{}");
    context.put_secret(SUPERUSER, "abc123");

    let error = steps(&context).expect_err("there is nothing to create a cluster with");

    assert!(
        matches!(
            error,
            Error::Service(ServiceError::ServiceProvidesNothing { .. })
        ),
        "{error:?}"
    );
}
/// **A first run is measured before its credentials exist** — the same guarantee, for this
/// recipe's own validation of a stand-in credential. See the MariaDB test of this name for the
/// failure it exists against.
#[test]
fn a_first_run_is_measured_before_its_credentials_exist() {
    let plan = FirstRun::new(
        &context("{}"),
        Postgres.ritual().expect("postgres bootstraps"),
    );

    assert!(
        plan.budget() >= BOOTSTRAP_PATIENCE,
        "one bootstrap step alone asks for {BOOTSTRAP_PATIENCE:?}, and the plan measured {:?}",
        plan.budget()
    );
}

/// **A reset sets the password and never creates a cluster** — roadmap task **T127**.
///
/// `--single` is the assertion beside the negative one: the repair has to reach the cluster
/// through a backend that opens no port and no socket, for the reason
/// [`set_the_password`] exists at all. A reset that started the real server to run an `ALTER
/// ROLE` would put a cluster whose password nobody knows on a TCP port first.
#[test]
fn the_reset_sets_the_password_and_never_creates_a_cluster() {
    let plan = FirstRun::new(
        &context("{}"),
        Postgres.ritual().expect("postgres bootstraps"),
    );

    let steps = plan
        .reset_steps(BTreeMap::from([(SUPERUSER.to_owned(), "a".repeat(32))]))
        .expect("a reset builds")
        .expect("postgres declares one");

    assert_eq!(steps.len(), 1, "{steps:?}");
    assert!(
        steps[0].args.iter().any(|arg| arg == "--single"),
        "the reset must reach the cluster through a backend that opens no port: {:?}",
        steps[0].args
    );
    assert!(
        !steps[0].program.display().to_string().contains(INITDB),
        "the reset runs initdb: {:?}",
        steps[0].program
    );
    assert!(
        steps[0]
            .stdin
            .as_deref()
            .is_some_and(|sql| sql.contains("ALTER ROLE")),
        "{:?}",
        steps[0].stdin
    );
}

/// A provisioning of `blog` on this fixture, as [`Ask`] and [`Credentials`].
fn asked() -> (crate::generate::Ask, crate::generate::Credentials) {
    (
        crate::generate::Ask {
            database: "blog".to_owned(),
            user: "blog".to_owned(),
        },
        crate::generate::Credentials {
            root: "a".repeat(32),
            account: "b".repeat(32),
        },
    )
}

/// The SQL every step of a provisioning carries, joined.
fn provisioning_sql(found: crate::generate::Found) -> String {
    let context = context("{}");
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
    let (ask, credentials) = asked();

    (admin.steps)(&context, &ask, found, &credentials)
        .expect("statements")
        .iter()
        .filter_map(|step| step.stdin.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// **The validator's alphabet is exactly what every accepted character survives being embedded
/// as** — roadmap task **T77b**. A chosen password containing every printable-ASCII character
/// the validator accepts must still produce SQL whose `LOGIN PASSWORD '...'` literal (on the
/// `ALTER ROLE` line, the plain one and not the `\gexec`-wrapped `CREATE ROLE`) closes where it
/// should.
#[test]
fn every_accepted_password_character_survives_the_login_password_literal() {
    let chosen =
        crate::generate::databases::validated_password("aZ09!\"#$%&()*+,-./:;<=>?@[]^_`{|}~")
            .expect("every one of these is accepted");

    let context = context("{}");
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
    let (ask, _) = asked();
    let credentials = crate::generate::Credentials {
        root: "a".repeat(32),
        account: chosen.clone(),
    };

    let sql = (admin.steps)(
        &context,
        &ask,
        crate::generate::Found::default(),
        &credentials,
    )
    .expect("statements")
    .iter()
    .filter_map(|step| step.stdin.clone())
    .collect::<Vec<_>>()
    .join("\n");

    assert!(
        sql.contains(&format!(
            "ALTER ROLE \"blog\" WITH LOGIN PASSWORD '{chosen}';"
        )),
        "the chosen password must appear as one intact literal: {sql}"
    );
}

/// **Design D6.** The role is created before the database, because the database is created
/// *owned by* it — and from PostgreSQL 15 `GRANT ALL ON DATABASE` no longer carries `CREATE` on
/// the `public` schema, so a role granted everything still cannot make a table.
#[test]
fn the_role_is_made_before_the_database_that_it_owns() {
    let sql = provisioning_sql(crate::generate::Found::default());

    let role = sql.find("CREATE ROLE").expect("a role is created");
    let database = sql.find("CREATE DATABASE").expect("a database is created");

    assert!(role < database, "the database is created first:\n{sql}");
    assert!(
        sql.contains(r#"CREATE DATABASE "blog" OWNER "blog""#),
        "{sql}"
    );
}

/// PostgreSQL has no `IF NOT EXISTS` for either object, so the conditional is psql's `\gexec` —
/// on a line of its own, where a meta-command is unambiguous.
#[test]
fn each_creation_is_guarded_because_neither_has_an_if_not_exists() {
    let sql = provisioning_sql(crate::generate::Found::default());

    assert!(sql.contains("pg_roles"), "{sql}");
    assert!(sql.contains("pg_database"), "{sql}");
    assert_eq!(sql.matches("\n\\gexec\n").count(), 2, "{sql}");
}

/// **A database that was already there does not change owner.** D3's deed covers the account,
/// not the database, and `ALTER DATABASE … OWNER TO` would hand somebody else's database over.
#[test]
fn an_existing_database_is_granted_and_never_taken_over() {
    let sql = provisioning_sql(crate::generate::Found {
        database: true,
        user: false,
    });

    assert!(!sql.contains("ALTER DATABASE"), "{sql}");
    assert!(
        sql.contains(r#"GRANT ALL PRIVILEGES ON DATABASE "blog" TO "blog""#),
        "{sql}"
    );
}

/// **Design D13.** The last step logs in as the new role and creates a *real* table in `public`
/// — a temporary one lives in a schema of its own and would prove nothing about the ownership
/// this recipe exists to get right.
#[test]
fn the_last_step_writes_a_real_table_as_the_new_role() {
    let context = context("{}");
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
    let (ask, credentials) = asked();

    let steps = (admin.steps)(
        &context,
        &ask,
        crate::generate::Found::default(),
        &credentials,
    )
    .expect("statements");
    let last = steps.last().expect("there is a last step");
    let sql = last.stdin.as_deref().unwrap_or_default();

    assert!(
        last.args.contains(&"--username=blog".to_owned()),
        "{:?}",
        last.args
    );
    assert!(
        last.args.contains(&"--dbname=blog".to_owned()),
        "{:?}",
        last.args
    );
    assert_eq!(
        last.env.get(PASSWORD_VARIABLE),
        Some(&credentials.account),
        "the check must authenticate as the new role and not as the superuser"
    );
    assert!(sql.contains("CREATE TABLE public."), "{sql}");
    assert!(!sql.to_ascii_uppercase().contains("TEMPORARY"), "{sql}");
    assert!(!sql.to_ascii_uppercase().contains("TEMP TABLE"), "{sql}");
}

/// No password in any argument, on the MariaDB recipe's reasoning: every process on this machine
/// can read an argument list.
#[test]
fn provisioning_puts_no_credential_in_an_argument() {
    let context = context("{}");
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
    let (ask, credentials) = asked();

    let mut steps = vec![(admin.probe)(&context, &ask, &credentials.root).expect("a probe")];
    steps.extend(
        (admin.steps)(
            &context,
            &ask,
            crate::generate::Found::default(),
            &credentials,
        )
        .expect("statements"),
    );

    for step in &steps {
        for argument in &step.args {
            assert!(
                !argument.contains(&credentials.root) && !argument.contains(&credentials.account),
                "{} put a password in an argument: {argument}",
                step.label
            );
        }
    }
}
