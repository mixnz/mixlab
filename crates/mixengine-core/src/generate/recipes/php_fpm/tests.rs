use std::collections::BTreeMap;
use std::path::Path;

use mixengine_proto::ServiceId;

use super::*;
use crate::generate::recipe;
use crate::generate::recipe::Upstream;
use crate::generate::settings::Settings;

/// D5: the pool's address is one expression, and the spec is built on top of it rather than
/// beside it. A site's `fastcgi_pass` asks the same method, so the file a pool writes and the
/// file that points at it cannot disagree.
#[test]
fn the_pool_names_one_address_and_its_spec_is_built_on_it() {
    let context = context("{}");

    let upstream = PhpFpm
        .upstream(&context)
        .expect("a pool has an address")
        .expect("and it is not None");

    let spec = PhpFpm
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("it builds");

    assert_eq!(
        matches!(upstream, Upstream::Tcp(_)),
        cfg!(windows),
        "the address is the wrong shape for the system the pool would run on"
    );

    match upstream {
        Upstream::Socket(socket) => {
            assert_eq!(
                socket,
                socket_path(&context).expect("a socket path"),
                "the site would point somewhere the pool is not listening"
            );
            assert!(
                spec.ports().is_empty(),
                "a Unix pool listens on a socket, not a port"
            );
        }
        Upstream::Tcp(address) => assert_eq!(spec.ports(), [address.port()]),
    }
}

/// What a failed start is diagnosed against — roadmap task **T38**.
///
/// One number on Windows and none anywhere else, which is this recipe's whole shape: a pool on
/// Unix listens on a socket, and a path is not something another program can be holding.
#[test]
fn the_spec_declares_the_port_the_pool_will_bind() {
    let context = context("{}");
    let spec = PhpFpm
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");

    if cfg!(windows) {
        assert_eq!(spec.ports(), [9000]);
    } else {
        assert!(spec.ports().is_empty(), "a Unix pool listens on a socket");
    }
}

/// **D2 and D3: the activator is a *second* address, never the pool's own.**
///
/// A site file names both, the pool first, so a request arriving while the pool is idle-stopped
/// is retried against the activator instead of answered with a 502. If the two were ever equal
/// the retry would be aimed at the address that is already refusing, and the fallback would be
/// decoration.
#[test]
fn a_pools_activator_is_a_second_address_and_not_the_pools_own() {
    let context = context("{}").with_activation_port(Some(9500));

    let pool = PhpFpm
        .upstream(&context)
        .expect("a pool has an address")
        .expect("and it is not None");
    let activator = PhpFpm
        .activator(&context)
        .expect("a pool has an activator")
        .expect("and it is not None");

    assert_ne!(
        pool, activator,
        "the fallback points at the address that is down"
    );

    assert_eq!(
        matches!(activator, Upstream::Tcp(_)),
        cfg!(windows),
        "the activator's address is the wrong shape for the system it would run on"
    );
}

/// **The one line T72a is about**: a pool has an idle probe whichever way this system runs it.
///
/// Before this, a pool on a socket answered [`None`] — an `IdleProbe` counted TCP and a socket
/// has no port — so `generate` attached no `IdlePolicy`, the pool ran for ever, and there was no
/// *stopped site* on two systems of three for a cold path to be measured against.
#[test]
fn a_pool_is_measurable_whichever_way_this_system_runs_it() {
    let probe = PhpFpm
        .idle_probe(&context("{}"))
        .expect("a pool can be told whether anybody is using it");

    match probe {
        mixengine_proto::IdleProbe::Connections { .. } => assert!(
            listens_on_tcp(),
            "a pool on a socket has no port whose connections could be counted"
        ),

        mixengine_proto::IdleProbe::FastCgiStatus { path, .. } => {
            assert!(
                !listens_on_tcp(),
                "a pool on a port is counted rather than asked; `php-cgi.exe` has no status page"
            );
            assert!(
                !path.ends_with(".php"),
                "a status path ending in .php is one a front end can be made to ask for: {path}"
            );
        }

        other => panic!("a pool grew a third kind of probe: {other:?}"),
    }
}

/// **The probe asks the address the pool is actually listening on.**
///
/// Both come off `listen`, which is what makes that true by construction rather than by
/// coincidence — a probe derived from the row's port would count a port a socket pool never
/// bound, and report a busy pool as idle for ever.
#[test]
fn the_probe_asks_the_address_the_pool_listens_on() {
    let context = context("{}");
    let listening = PhpFpm
        .upstream(&context)
        .expect("a pool has an address")
        .expect("and it is not None");

    match PhpFpm.idle_probe(&context).expect("a pool has a probe") {
        mixengine_proto::IdleProbe::FastCgiStatus { socket, .. } => {
            assert_eq!(Upstream::Socket(socket), listening);
        }

        mixengine_proto::IdleProbe::Connections { port } => {
            assert_eq!(
                Upstream::Tcp(address(&context).expect("an address")),
                listening
            );
            assert_eq!(
                Some(port),
                context.port(),
                "the probe counts a port the pool is not on"
            );
        }

        other => panic!("a pool grew a third kind of probe: {other:?}"),
    }
}

/// **The pool file offers a status page, and offers it at a name no front end can ask for.**
///
/// `pm.status_listen` would be the cleaner arithmetic and is deliberately absent: it exists only
/// from PHP 8.0, this product offers PHP from 7.0 upwards on purpose, and php-fpm refuses a file
/// carrying a directive it does not know — so a 7.4 pool would not start at all.
#[test]
fn the_pool_file_offers_a_status_page_no_front_end_can_ask_for() {
    let rendered = recipe::render(&PhpFpm, &context("{}")).expect("a rendering")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains(&format!("pm.status_path = {STATUS_PATH}")),
        "the pool is not offering the page the probe asks for\n{rendered}"
    );
    assert!(
        !STATUS_PATH.ends_with(".php"),
        "a status path ending in .php is one a front end can be made to ask for"
    );
    // **By line and not by substring**: the comment above the directive names
    // `pm.status_listen` in order to say why it is absent, and a test that could not tell an
    // explanation from a setting would have to choose between the two.
    assert!(
        !rendered
            .lines()
            .any(|line| line.trim_start().starts_with("pm.status_listen")),
        "a directive PHP 7.x does not know refuses the whole file\n{rendered}"
    );
}

/// **A row that predates T70 renders a site with no fallback rather than a broken one.**
///
/// On Windows the activator's port comes from the row, and every row written before the column
/// existed carries none. That home simply has no on-demand activation — which is exactly what it
/// had yesterday — where inventing a port here would render a site file pointing at something
/// nothing ever binds, and turn a missing feature into a broken one.
#[test]
fn a_pool_whose_row_carries_no_activation_port_has_no_activator_on_windows() {
    let context = context("{}").with_activation_port(None);

    let activator = PhpFpm.activator(&context).expect("asking is not an error");

    assert_eq!(
        activator.is_none(),
        cfg!(windows),
        "a socket pool derives its activator and needs no row; a TCP pool cannot"
    );
}

/// **The first recipe in this build to answer anything but `None`** — T70, D9.
///
/// A pool is stopped after half an hour of nobody using it *because something now starts it
/// again*: the site names its activator, and the request that finds the pool down is what wakes
/// it. The number is the one `features/resource-isolation.md` already publishes.
///
/// The databases and the caches stay `None` until **T70a**, which is what can start them again,
/// and the two front ends stay `None` for ever — the thing that starts everything else back up
/// cannot be the thing that gets stopped.
#[test]
fn a_pool_is_idle_stopped_after_half_an_hour() {
    // Only while the home saves resources (T167b); by default nothing is idle-stopped.
    assert_eq!(PhpFpm.idle_when_saving(), Some(Millis::from_secs(30 * 60)));
    assert_eq!(PhpFpm.idle_default(), None);
}

/// A pool is the one service a memory watchdog may restart — roadmap task **T71a**.
///
/// **Asserted against its neighbours rather than alone**, because the interesting half is what
/// stays `false`: a database restarted for its size loses a transaction, and a cache loses
/// everything it was holding. A leaking pool is the case the watchdog was built for.
#[test]
fn a_pool_is_the_one_service_a_watchdog_may_restart() {
    assert!(PhpFpm.restart_over_memory_default());

    assert!(
        !super::super::mariadb::Mariadb.restart_over_memory_default(),
        "a restart mid-transaction is a data question, not a reading"
    );

    assert!(
        !super::super::redis::Redis.restart_over_memory_default(),
        "restarting a cache is deleting data somebody believes is still there"
    );

    assert!(
        !super::super::caddy::Caddy.restart_over_memory_default(),
        "the thing that starts everything else back up is not restarted on a measurement"
    );
}

/// A pool for PHP 8.3.33 in a home at [`root`], with `overrides` applied.
fn context(overrides: &str) -> Context {
    let service = ServiceId::parse("php-fpm@8.3.33").expect("an id");
    let settings =
        Settings::merge(PhpFpm.settings(), overrides, &service).expect("usable overrides");

    Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        provides(),
        Some(9000),
        settings,
    )
}

/// **Both SAPIs are told the same thing.** A pool that reads the generated set while `php -m`
/// does not is two answers to one question — and on Windows the terminal's answer is a PHP with
/// no `curl`, no `mbstring` and no `intl`, because there those are shared modules that only an
/// ini switches on.
///
/// Both arms directly, for the reason the socket test gives: the claim is worth checking on the
/// machine that does not take that branch.
#[test]
fn a_pool_reads_the_ini_set_its_runtime_carries() {
    let context = context("{}");

    for builder in [
        PhpFpm::unix(&context, &socket_path(&context).expect("a socket path")).expect("a spec"),
        PhpFpm::windows(&context, address(&context).expect("an address")).expect("a spec"),
    ] {
        let spec = builder.build().expect("a valid spec");
        let scan = match spec
            .env()
            .get(crate::runtimes::extensions::SCAN_DIR_ENV)
            .expect("a pool that is told where its ini set is")
        {
            mixengine_proto::EnvValue::Literal { value } => value.clone(),
            other => panic!("the ini set is not a secret: {other:?}"),
        };

        assert!(
            scan.contains("conf.d"),
            "the pool is pointed somewhere that is not a conf.d: {scan}"
        );
        assert!(
            scan.contains(context.version()),
            "the pool is reading another version's extensions: {scan}"
        );
    }
}

/// A PHP that publishes every executable this recipe might ask for, on either system.
///
/// Both SAPIs at once, which no real artifact has: what these tests exercise is the branch this
/// machine is *not*, so the map has to answer for both of them.
fn provides() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("php".to_owned(), "bin/php".to_owned()),
        ("php-fpm".to_owned(), "sbin/php-fpm".to_owned()),
        ("php-cgi".to_owned(), "php-cgi.exe".to_owned()),
    ])
}

/// An absolute path on whichever system this is compiled for.
const fn root() -> &'static str {
    if cfg!(windows) {
        r"C:\MixEngine"
    } else {
        "/opt/mixengine"
    }
}

/// One pool per installed PHP, named by the version it runs — so its id carries an `@`.
#[test]
fn a_pool_is_named_after_the_php_it_runs() {
    assert_eq!(PhpFpm.instancing(), Instancing::Named);
}

/// The recipe says where its binary comes from, and it is not the package table.
///
/// This is what `service.create` refuses on and what the install hook keys off, so it is
/// asserted rather than assumed: a recipe that answered `Package` here would be one a user could
/// declare against a `packages` row that does not exist.
#[test]
fn a_pool_comes_out_of_an_installed_php() {
    assert_eq!(PhpFpm.source(), Source::Runtime(RuntimeKind::Php));
}

/// The rendered file carries the values the row and the overrides gave it.
///
/// Rendered through [`recipe::render`] rather than through a generator, for `caddy.rs`' reason:
/// what is being checked is the *template*, and running the real validator would need fifty
/// megabytes of PHP to find out whether a variable name is misspelled.
#[test]
fn the_pool_file_says_what_the_overrides_said() {
    let context = context(r#"{"max_children": 12}"#);
    let documents = recipe::render(&PhpFpm, &context).expect("a rendering");

    assert_eq!(documents.len(), 1, "php-fpm renders one file");
    assert_eq!(documents[0].relative(), Path::new(POOL_FILE));

    let rendered = documents[0].contents();
    assert!(rendered.contains("pm.max_children = 12"), "{rendered}");
    assert!(rendered.contains("pm = static"), "{rendered}");
    assert!(
        rendered.contains(&format!(
            "php-fpm-{}.sock",
            context.service().instance().expect("a pool is instanced")
        )),
        "the socket is named after the instance, which for a shared pool is the PHP it runs and \
             for an extension's pool is the extension — roadmap task T82a\n{rendered}"
    );
}

/// **The template and the spec must name the same socket.**
///
/// They are computed twice — once in Jinja for the file php-fpm reads, once in Rust for the
/// readiness check the daemon makes — and the failure when they disagree is a service that
/// starts perfectly and is reported as never having come up. Nothing else in this recipe is
/// worth a test as much as this.
///
/// [`PhpFpm::unix`] directly rather than through [`Recipe::spec`], which is the point of that
/// branch being chosen by a `cfg!` value: the claim is about the Unix shape and is worth
/// checking on the machine that does not run it.
#[test]
fn the_file_and_the_readiness_check_name_one_socket() {
    let context = context("{}");
    let rendered = recipe::render(&PhpFpm, &context).expect("a rendering")[0]
        .contents()
        .to_owned();

    let spec = PhpFpm::unix(&context, &socket_path(&context).expect("a socket path"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    let ReadyCheck::UnixSocket { path, .. } = spec.ready() else {
        panic!("a pool on this system is proved up by its socket");
    };

    // Both sides normalised to forward slashes, which is a no-op on the systems this branch
    // actually runs on. It matters only here, on the machine that takes the other branch: Jinja
    // joins `{{ paths.run }}/…` with a literal slash while `Path::join` uses the host's
    // separator, so a Windows run of this test would fail on a difference that cannot exist
    // where the code is used.
    let slashes = |text: &str| text.replace('\\', "/");

    assert!(
        slashes(&rendered).contains(&slashes(&path.display().to_string())),
        "the file says one socket and the readiness check waits on another\n{rendered}"
    );
}

/// **The password is named, never carried** — roadmap task **T82a**, its design's D4.
///
/// The same assertion the three database recipes make about their own credential, arriving at
/// the process that reads one of theirs. The edge beside it is D7: a pool started before its
/// database has ever run would find no keyring entry, because the entry is written by that
/// database's first run.
///
/// Both arms directly, for the reason the socket tests give: the claim is worth checking on the
/// machine that does not take that branch.
#[test]
fn a_pool_with_a_credential_names_it_and_depends_on_the_server_it_opens() {
    let context = with_credential(context("{}"));

    for builder in [
        PhpFpm::unix(&context, &socket_path(&context).expect("a socket path")).expect("a spec"),
        PhpFpm::windows(&context, address(&context).expect("an address")).expect("a spec"),
    ] {
        let spec = builder.build().expect("a valid spec");

        assert!(
            matches!(
                spec.env().get(crate::extensions::pools::CREDENTIAL_ENV),
                Some(mixengine_proto::EnvValue::Keyring { service, key })
                    if service == mixengine_platform::KEYRING_SERVICE
                        && key == "mariadb@main/root"
            ),
            "a password reached the spec, or its address did not: {:?}",
            spec.env()
        );

        assert_eq!(
            spec.depends_on()
                .iter()
                .map(ServiceId::as_str)
                .collect::<Vec<_>>(),
            ["mariadb@main"],
            "a pool started before its database finds no credential at all"
        );
    }
}

/// And a pool that carries none names none, which is every pool but one on a machine.
#[test]
fn a_pool_without_one_carries_nothing_new() {
    let context = context("{}");

    for builder in [
        PhpFpm::unix(&context, &socket_path(&context).expect("a socket path")).expect("a spec"),
        PhpFpm::windows(&context, address(&context).expect("an address")).expect("a spec"),
    ] {
        let spec = builder.build().expect("a valid spec");

        assert!(spec.depends_on().is_empty(), "{:?}", spec.depends_on());
        assert!(
            !spec
                .env()
                .contains_key(crate::extensions::pools::CREDENTIAL_ENV),
            "{:?}",
            spec.env()
        );
    }
}

/// **`clear_env = no` reaches exactly the pool that has a credential** — the design's D3.
///
/// This is the assertion that would catch a database superuser's password being handed to every
/// project's PHP, and it is worth more than anything else in this task: the directive is what
/// makes the value reachable at all, so a rendering that carried it everywhere would be the
/// whole feature inverted.
#[test]
fn only_the_pool_with_a_credential_passes_its_environment_on() {
    let plain = recipe::render(&PhpFpm, &context("{}")).expect("a rendering")[0]
        .contents()
        .to_owned();
    let carrying = recipe::render(&PhpFpm, &with_credential(context("{}"))).expect("a rendering")
        [0]
    .contents()
    .to_owned();

    assert!(
        !plain.contains("clear_env"),
        "every other pool in a home leaves php-fpm's own `clear_env = yes`\n{plain}"
    );
    assert!(carrying.contains("clear_env = no"), "{carrying}");
}

/// A context carrying the credential a `web-app` extension's pool would be given.
fn with_credential(context: Context) -> Context {
    context.with_credential(Some(crate::extensions::pools::Credential {
        env: crate::extensions::pools::CREDENTIAL_ENV.to_owned(),
        keyring_service: mixengine_platform::KEYRING_SERVICE.to_owned(),
        keyring_key: "mariadb@main/root".to_owned(),
        database: ServiceId::parse("mariadb@main").expect("a service id"),
    }))
}

/// **Two pools on one PHP are two sockets** — roadmap task **T82a**.
///
/// The path used to be spelled from the runtime's version, which is the same string as the
/// instance for every pool [`pools::ensure`](crate::services::pools::ensure) makes and is *not*
/// the same string for the pool a `web-app` extension owns. Both halves are asserted together,
/// because the second is only worth having if the first still holds: an existing home's socket
/// must not move.
///
/// [`socket_path`] directly, for the reason the two tests around it give: this is the Unix
/// shape, and it is worth checking on the machine that does not run it.
#[test]
fn a_pools_socket_is_named_after_its_instance() {
    let shared = socket_path(&context("{}")).expect("a socket path");
    let owned = socket_path(&pool_for("php-fpm@phpmyadmin")).expect("a socket path");

    assert!(
        shared.ends_with("php-fpm-8.3.33.sock"),
        "the path every existing home already has: {}",
        shared.display()
    );
    assert!(
        owned.ends_with("php-fpm-phpmyadmin.sock"),
        "an extension's pool answers on a socket of its own: {}",
        owned.display()
    );
}

/// A pool with a different id, on the same home and the same PHP as [`context`].
fn pool_for(service: &str) -> Context {
    let service = ServiceId::parse(service).expect("an id");
    let settings = Settings::merge(PhpFpm.settings(), "{}", &service).expect("defaults");

    Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        provides(),
        Some(9001),
        settings,
    )
}

/// A socket path `sockaddr_un` cannot hold is refused here, by name.
///
/// T33a measured the cap at 103 characters against a real server, and what it costs to find out
/// the hard way is the reason this is a check: php-fpm aborts *after* it has started, in a way
/// that reads like a different failure entirely.
///
/// Asked of [`PhpFpm::unix`] rather than of [`Recipe::spec`], for the reason the socket
/// agreement above is: [`PhpFpm::windows`] computes no socket path at all, and the check is
/// still worth running on a machine that would take that branch.
#[test]
fn a_socket_path_too_long_for_the_kernel_is_refused_by_name() {
    let deep = format!("/{}", "nested/".repeat(20));
    let service = ServiceId::parse("php-fpm@8.3.33").expect("an id");
    let settings = Settings::merge(PhpFpm.settings(), "{}", &service).expect("defaults");
    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(&deep),
        provides(),
        None,
        settings,
    );

    let error = socket_path(&context).expect_err("a path no kernel accepts");

    assert!(
        error.to_string().contains("103"),
        "the measurement is what makes this message useful: {error}"
    );
}

/// A PHP packed without the SAPI this recipe needs is named as such.
#[test]
fn a_php_without_the_right_sapi_is_named() {
    let service = ServiceId::parse("php-fpm@8.3.33").expect("an id");
    let settings = Settings::merge(PhpFpm.settings(), "{}", &service).expect("defaults");
    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        BTreeMap::from([("php".to_owned(), "bin/php".to_owned())]),
        Some(9000),
        settings,
    );

    let error = PhpFpm.spec(&context).expect_err("no SAPI to run");

    assert!(
        matches!(
            error,
            Error::Service(ServiceError::ServiceProvidesNothing { .. })
        ),
        "{error:?}"
    );
}
