use mixengine_proto::ServiceId;

use super::*;
use crate::generate::settings::Settings;

/// **`msys2` is installable and never runnable** — roadmap task **T206a**, D5.
#[test]
fn msys2_is_installable_and_never_runnable() {
    let catalogue = Catalogue::builtin();
    assert!(catalogue.installable().any(|name| name == "msys2"));
    assert!(catalogue.is_toolchain("msys2"));
    assert!(!catalogue.packages().any(|name| name == "msys2"));
    assert!(catalogue.recipe("msys2").is_none());
    assert!(!catalogue.is_toolchain("caddy"));
}

/// **Every database names its superuser, and nothing else names anything** — roadmap task
/// **T82**, the design's D5.
///
/// The account name is the recipe's rather than the manifest's: a `web-app` that wrote `root`
/// itself would be a manifest that is wrong the day a recipe changes its mind, and T83's
/// connection handoff has to read the same answer out of the same place.
#[test]
fn only_the_databases_name_an_administrator() {
    let catalogue = super::super::Catalogue::builtin();

    for (package, expected) in [
        ("mariadb", Some("root")),
        ("mysql", Some("root")),
        ("postgres", Some("postgres")),
        ("redis", None),
        ("memcached", None),
        ("mongodb", None),
        ("caddy", None),
        ("nginx", None),
        ("php-fpm", None),
    ] {
        let recipe = catalogue
            .recipe(package)
            .unwrap_or_else(|| panic!("{package} is compiled in"));

        assert_eq!(recipe.administrator(), expected, "{package}");
    }
}

/// **Only MariaDB asks for a certificate** — roadmap task **T99**.
///
/// MySQL writes its own pair once at `--initialize`, PostgreSQL ships with `ssl` off, and
/// nothing else here speaks TLS to a client. A recipe that started asking would be a decision
/// made in that recipe's own design, and this is the test that notices.
#[test]
fn only_mariadb_asks_for_a_certificate() {
    let catalogue = super::super::Catalogue::builtin();

    for (package, wants_one) in [
        ("mariadb", true),
        ("mysql", false),
        ("postgres", false),
        ("redis", false),
        ("memcached", false),
        ("mongodb", false),
        ("caddy", false),
        ("nginx", false),
        ("php-fpm", false),
    ] {
        let recipe = catalogue
            .recipe(package)
            .unwrap_or_else(|| panic!("{package} is compiled in"));
        let service = ServiceId::parse(format!("{package}@main")).expect("an id");
        let settings =
            Settings::merge(recipe.settings(), "{}", &service).expect("the defaults merge");
        let context = Context::for_test(
            service,
            package,
            Path::new(root()),
            BTreeMap::new(),
            Some(1),
            settings,
        );

        assert_eq!(
            recipe.certificate(&context).is_some(),
            wants_one,
            "{package}"
        );
    }
}

/// **A template can branch on `certificate` and on nothing else that is optional** — roadmap
/// task **T99**. `service.instance` renders the word `none`; this group renders a falsy value.
#[test]
fn a_context_with_no_certificate_renders_a_falsy_group() {
    let service = ServiceId::parse("mariadb@main").expect("an id");
    let settings = Settings::merge(&[], "{}", &service).expect("nothing to merge");
    let context = Context::for_test(
        service,
        "mariadb",
        Path::new(root()),
        BTreeMap::new(),
        Some(3306),
        settings,
    );

    let render = |context: &Context| {
        minijinja::Environment::new()
            .render_str(
                "{% if certificate %}yes {{ certificate.fingerprint }}{% else %}no{% endif %}",
                minijinja::Value::from_serialize(context.rendering()),
            )
            .expect("the template renders")
    };

    assert_eq!(render(&context), "no");

    let context = context.with_certificate(ServiceCertificate {
        certificate: Path::new(root()).join("certs/services/mariadb@main.crt"),
        key: Path::new(root()).join("certs/services/mariadb@main.key"),
        fingerprint: "abc123".to_owned(),
    });

    assert_eq!(render(&context), "yes abc123");
}

/// An absolute path on whichever system this is compiled for.
const fn root() -> &'static str {
    if cfg!(windows) {
        r"C:\MixEngine"
    } else {
        "/opt/mixengine"
    }
}

/// **While saving resources: an hour for the databases and the caches, half an hour for a pool,
/// never for a front end** — T70, T70a, and T167b which moved them behind the switch.
///
/// A number that idles a service nothing can start again is a home that broke, so each of these
/// names only a service something can wake; the two front ends stay [`None`] for ever, because
/// the thing that starts everything else back up cannot be the thing that gets stopped.
/// **No recipe idle-stops a service nobody asked about** — ADR 0041. A site that was up stays
/// up; the numbers above apply only while the home saves resources.
#[test]
fn no_recipe_idles_anything_by_default() {
    let catalogue = Catalogue::builtin();

    for package in [
        "php-fpm",
        "mariadb",
        "mysql",
        "postgres",
        "redis",
        "memcached",
        "mongodb",
        "caddy",
        "nginx",
    ] {
        let recipe = catalogue
            .recipe(package)
            .unwrap_or_else(|| panic!("{package} is a builtin recipe"));
        assert_eq!(recipe.idle_default(), None, "{package}");
    }
}

#[test]
fn a_recipe_answers_an_idle_time_for_saving_once_something_can_start_it_again() {
    let catalogue = Catalogue::builtin();

    let default_of = |package: &str| {
        catalogue
            .recipe(package)
            .unwrap_or_else(|| panic!("{package} is a builtin recipe"))
            .idle_when_saving()
    };

    for package in [
        "mariadb",
        "mysql",
        "postgres",
        "redis",
        "memcached",
        "mongodb",
    ] {
        assert_eq!(
            default_of(package),
            Some(mixengine_proto::Millis::from_secs(60 * 60)),
            "{package} was not turned on by the task that made it wakeable"
        );
    }

    assert_eq!(
        default_of("php-fpm"),
        Some(mixengine_proto::Millis::from_secs(30 * 60)),
        "the pool's own default moved"
    );

    for package in ["caddy", "nginx"] {
        assert_eq!(
            default_of(package),
            None,
            "{package} starts everything else back up and must never be stopped"
        );
    }
}

/// Every service that listens on a port names the one its product is documented under.
///
/// **A wish belongs to the recipe, not to `service.create`.** Which port MySQL would like is a
/// fact about MySQL, and a caller that had to know 3306 would be a caller that has to know
/// every number in the catalogue. Caddy is the deliberate exception: 80 and 443 are its own
/// settings, and a web server renumbered to 81 because something else answered on 80 is not a
/// web server anybody asked for.
#[test]
fn a_recipe_that_listens_on_a_port_says_which_one_it_would_like() {
    let catalogue = Catalogue::builtin();

    let preferred = |package: &str| {
        catalogue
            .recipe(package)
            .unwrap_or_else(|| panic!("{package} is in the catalogue"))
            .preferred_port()
    };

    assert_eq!(preferred("mariadb"), Some(3306));
    assert_eq!(
        preferred("mysql"),
        Some(3306),
        "the two databases name one number, which is what the allocation is for"
    );
    assert_eq!(preferred("postgres"), Some(5432));
    assert_eq!(preferred("redis"), Some(6379));
    assert_eq!(preferred("memcached"), Some(11211));
    assert_eq!(preferred("php-fpm"), Some(9000));
    assert_eq!(
        preferred("caddy"),
        None,
        "a front end's ports are its own settings"
    );
}

/// **T97.** The free function and the method are one join, and this is what keeps them one: a
/// switch writes a port-80 grant against the first and the supervisor runs the second, so a
/// suffix spelled in one place and not the other would be a capability on a file nothing runs.
#[test]
fn a_package_executable_is_spelled_once_however_it_is_asked_for() {
    let service = ServiceId::parse("nginx").expect("an id");
    let settings = Settings::merge(&[], "{}", &service).expect("no settings, no overrides");
    let context = Context::for_test(
        service,
        "nginx",
        Path::new(root()),
        BTreeMap::new(),
        None,
        settings,
    );

    let install_path = Path::new(root()).join("packages").join("nginx");

    assert_eq!(context.program("nginx"), program(&install_path, "nginx"));
    assert_eq!(
        program(&install_path, "nginx")
            .file_name()
            .and_then(std::ffi::OsStr::to_str),
        Some(if cfg!(windows) { "nginx.exe" } else { "nginx" }),
        "the suffix is this operating system's and never a recipe's"
    );
}

/// **A secret never reaches a template.**
///
/// [`Context::rendering`] is what a Jinja template sees, and the secret map is not part of it. A
/// `my.cnf` with a root password in it would be a plaintext credential on disk written by the
/// very design that refuses one, so this is a test and not a comment.
#[test]
fn a_template_cannot_see_a_secret() {
    let service = ServiceId::parse("mariadb@main").expect("an id");
    let settings = Settings::merge(&[], "{}", &service).expect("no settings, no overrides");
    let mut context = Context::for_test(
        service,
        "mariadb",
        Path::new(root()),
        BTreeMap::new(),
        Some(3306),
        settings,
    );
    context.put_secret("root", "hunter2");

    // Serialised through the very value the renderer hands to minijinja, so what is asserted is
    // what a template can reach and not what this test remembered to look at.
    let rendering = serde_json::to_string(&context.rendering()).expect("a rendering serialises");

    assert!(!rendering.contains("hunter2"), "{rendering}");
    assert!(!rendering.contains("secret"), "{rendering}");
    assert_eq!(context.secret("root"), "hunter2", "and a recipe still can");
}
