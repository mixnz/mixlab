use std::sync::Arc;

use mixengine_proto::{Millis, ReadyCheck, RestartPolicy, ServiceSpecBuilder, StopBehaviour};
use mixengine_testkit::FakeService;

use super::*;
use crate::config::PathOverrides;

/// A recipe over `fakeservice`, with one file and one setting.
///
/// Deliberately not the daemon's own fixture recipe: this one is about the *generator*, so it
/// renders something whose content is worth asserting and takes a setting worth overriding.
#[derive(Debug)]
struct Fake;

const GREETING: &str = "greeting";

impl Recipe for Fake {
    fn package(&self) -> &'static str {
        "fakeservice"
    }

    /// Set so that the generator carrying it onto the spec is observable — task **T71a**.
    ///
    /// The trait's default is `false`, so a fixture that took it would prove nothing about
    /// whether `generate` reads this at all.
    fn restart_over_memory_default(&self) -> bool {
        true
    }

    fn instancing(&self) -> Instancing {
        Instancing::Named
    }

    fn settings(&self) -> &'static [Setting] {
        &[Setting {
            key: GREETING,
            default: Preset::Text("hello"),
        }]
    }

    fn files(&self) -> &'static [TemplateFile] {
        &[TemplateFile {
            path: "fakeservice.conf",
            source: "say = {{ settings.greeting }}\nport = {{ service.port }}\n{% if certificate \
                         %}cert = {{ certificate.path }}\nkey = {{ certificate.key }}\n# {{ \
                         certificate.fingerprint }}\n{% endif %}{{ extra }}",
        }]
    }

    /// Measured on the port the row allocated, as every server recipe here is.
    ///
    /// Its sibling `idle_default` is left at the trait's `None`, which is what every shipped
    /// recipe answers too — so this fixture exercises the join without pretending a default
    /// exists anywhere.
    fn idle_probe(&self, context: &Context) -> Option<mixengine_proto::IdleProbe> {
        context
            .port()
            .map(|port| mixengine_proto::IdleProbe::Connections { port })
    }

    /// Forty-five minutes while the home saves resources — T167b. A number of its own so the
    /// test below can tell it from the row's.
    fn idle_when_saving(&self) -> Option<mixengine_proto::Millis> {
        Some(mixengine_proto::Millis::from_secs(45 * 60))
    }

    /// On the port the row allocated, as every database recipe answers — T70a.
    ///
    /// A fixture rather than a real recipe, because what the generator is asked here is
    /// whether it carries a recipe's answer out of the same pass that renders. *Which*
    /// addresses each database names is its own recipe's test.
    fn held_while_stopped(&self, context: &Context) -> Result<Vec<Upstream>> {
        Ok(context
            .port()
            .map(|port| {
                vec![Upstream::Tcp(std::net::SocketAddr::from((
                    std::net::Ipv4Addr::LOCALHOST,
                    port,
                )))]
            })
            .unwrap_or_default())
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        Ok(
            ServiceSpec::builder(context.service().clone(), FakeService::program())
                .cwd(context.data())
                .ready(ReadyCheck::PidAlive { settle: Millis(10) })
                .restart(RestartPolicy::Never)
                .stop(StopBehaviour::Signal { grace: Millis(500) }),
        )
    }
}

/// [`Fake`], asking for a certificate — roadmap task **T99**.
///
/// A second type rather than a flag on [`Fake`], so that every existing fixture keeps proving
/// that a recipe which does not ask gets nothing. Everything but the one answer delegates.
#[derive(Debug)]
struct CertifiedFake;

impl Recipe for CertifiedFake {
    fn package(&self) -> &'static str {
        Fake.package()
    }

    fn restart_over_memory_default(&self) -> bool {
        Fake.restart_over_memory_default()
    }

    fn instancing(&self) -> Instancing {
        Fake.instancing()
    }

    fn settings(&self) -> &'static [Setting] {
        Fake.settings()
    }

    fn files(&self) -> &'static [TemplateFile] {
        Fake.files()
    }

    fn idle_probe(&self, context: &Context) -> Option<mixengine_proto::IdleProbe> {
        Fake.idle_probe(context)
    }

    fn held_while_stopped(&self, context: &Context) -> Result<Vec<Upstream>> {
        Fake.held_while_stopped(context)
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        Fake.spec(context)
    }

    fn certificate(&self, context: &Context) -> Option<Vec<String>> {
        Some(crate::certs::service::names(context.bind()))
    }
}

/// A recipe for a package that exists once, so its id carries no `@`.
///
/// Nothing but the instancing and the working directory: what it is here to show is where the
/// generator puts a singleton's data, and a template would only be noise around that.
#[derive(Debug)]
struct Solo;

impl Recipe for Solo {
    fn package(&self) -> &'static str {
        "solo"
    }

    fn instancing(&self) -> Instancing {
        Instancing::Single
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        Ok(
            ServiceSpec::builder(context.service().clone(), FakeService::program())
                .cwd(context.data())
                .ready(ReadyCheck::PidAlive { settle: Millis(10) })
                .restart(RestartPolicy::Never)
                .stop(StopBehaviour::Signal { grace: Millis(500) }),
        )
    }
}

/// The same front end with a checker that refuses everything — for T81c's D5.
///
/// A front end whose binary says no is the whole of what `would_serve` is about, and the cheapest
/// way to have one is a validator that exits non-zero: what is under test is that the judgement
/// happens and that nothing was installed, not what a real server dislikes.
#[derive(Debug)]
struct FussyFront;

impl Recipe for FussyFront {
    fn package(&self) -> &'static str {
        "front"
    }

    fn instancing(&self) -> Instancing {
        Instancing::Single
    }

    fn role(&self) -> Role {
        Role::FrontEnd(FrontEndServer::Nginx)
    }

    fn swept(&self) -> &'static [&'static str] {
        &["sites", "extensions"]
    }

    fn files(&self) -> &'static [TemplateFile] {
        &[TemplateFile {
            path: "front.conf",
            source: "front\n",
        }]
    }

    fn fragments(&self, context: &Context) -> Result<Vec<Document>> {
        Front.fragments(context)
    }

    fn validator(&self, _context: &Context) -> Option<Validator> {
        Some(Validator::new(FakeService::program(), "front.conf").args([
            "--complain",
            "unknown directive: probe",
            "--exit-code",
            "1",
        ]))
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        Front.spec(context)
    }
}

/// A recipe that is a front end, so the generator asks it for sites.
///
/// It renders one file per site with the domain in it, and nothing else: what is under test here
/// is the *plumbing* — that a front end is asked, that the sites it is given are the enabled ones
/// with their upstreams resolved, and that what it returns joins the set the validator judges.
/// What a real Caddyfile or `server` block has to say is judged by the real server, in
/// `crates/mixengine-cli/tests/{caddy,nginx}.rs`.
#[derive(Debug)]
struct Front;

impl Recipe for Front {
    fn package(&self) -> &'static str {
        "front"
    }

    fn instancing(&self) -> Instancing {
        Instancing::Single
    }

    fn role(&self) -> Role {
        Role::FrontEnd(FrontEndServer::Nginx)
    }

    fn swept(&self) -> &'static [&'static str] {
        &["sites", "extensions"]
    }

    fn fragments(&self, context: &Context) -> Result<Vec<Document>> {
        Ok(context
            .fragments()
            .iter()
            .map(|addition| {
                Document::new(
                    format!("extensions/{}.conf", addition.extension),
                    format!("{}\n", addition.fragment),
                )
            })
            .collect())
    }

    fn sites(&self, _context: &Context, served: &[Served]) -> Result<Vec<Document>> {
        Ok(served
            .iter()
            .map(|site| {
                Document::new(
                    format!("sites/{}.conf", site.primary()),
                    format!(
                        "serve {} from {}\n",
                        site.primary(),
                        site.doc_root.display()
                    ),
                )
            })
            .collect())
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        Ok(
            ServiceSpec::builder(context.service().clone(), FakeService::program())
                .cwd(context.data())
                .ready(ReadyCheck::PidAlive { settle: Millis(10) })
                .restart(RestartPolicy::Never)
                .stop(StopBehaviour::Signal { grace: Millis(500) }),
        )
    }
}

/// D1 and D4 together: a site is rendered into the front end's own set, and a site that stops
/// being declared takes its file with it — on the same walk, so the same reload carries both.
#[tokio::test]
async fn a_front_end_renders_the_sites_this_home_declares_and_sweeps_the_ones_it_does_not() {
    let (home, generator) = home_of(Arc::new(Front), "front", "front", "{}").await;

    sqlx::query(
        "INSERT INTO projects (id, name, root_path, created_at)
             VALUES (1, 'blog', '/src/blog', '2026-08-23T00:00:00Z')",
    )
    .execute(generator.store.pool())
    .await
    .expect("a project");

    sqlx::query(
        "INSERT INTO sites (id, project_id, doc_root, kind, state)
             VALUES (1, 1, 'public', 'static', 'enabled')",
    )
    .execute(generator.store.pool())
    .await
    .expect("a site");

    sqlx::query(
        "INSERT INTO site_domains (site_id, domain, is_primary) VALUES (1, 'blog.test', 1)",
    )
    .execute(generator.store.pool())
    .await
    .expect("a domain");

    let first = generator.declared().await.expect("a rendering");
    assert!(first[0].changed());

    let sites = home.path().join("etc").join("front").join("sites");
    assert!(
        sites.join("blog.test.conf").is_file(),
        "the site was not rendered"
    );

    // Nothing moved, so nothing is written and nothing reloads. This is what "idempotent
    // re-runs" is: a property of the diff rather than a feature anybody implemented.
    let again = generator.declared().await.expect("a second rendering");
    assert!(!again[0].changed(), "an unchanged home wrote something");

    sqlx::query("UPDATE sites SET state = 'disabled' WHERE id = 1")
        .execute(generator.store.pool())
        .await
        .expect("the site is turned off");

    let after = generator.declared().await.expect("a third rendering");

    assert!(
        !sites.join("blog.test.conf").exists(),
        "a disabled site's file survived, so it is still being served"
    );
    assert!(
        after[0].changed(),
        "the removal did not reach the reload, so the front end went on serving it"
    );
}

/// **An extension's fragment lands in the front end's own `etc/`, and leaves with it** —
/// roadmap task **T81c**.
///
/// And only the fragment written for *this* front end: the same manifest declares one for each
/// of the two, and a home running one of them renders one file. That is what `server` is for,
/// and rendering both would be a Caddyfile inside an `nginx.conf`.
#[tokio::test]
async fn a_front_end_renders_the_fragments_its_extensions_declare_and_sweeps_them_with_it() {
    let (home, generator) = home_of(Arc::new(Front), "front", "front", "{}").await;

    let manifest = crate::extensions::manifest::read(
        std::path::Path::new("extension.toml"),
        "schema = 1\n\n[extension]\nid = \"probe\"\nname = \"Probe\"\nversion = \"1.0.0\"\nkind = \"recipe\"\n\n[[recipe.front_end]]\nserver = \"nginx\"\nfragment = \"map $a $b { default 0; }\"\n\n[[recipe.front_end]]\nserver = \"caddy\"\nfragment = \"(probe) { respond 204 }\"\n",
    )
    .expect("the manifest parses");

    let installed = crate::extensions::store::Installed {
        id: manifest.extension.id.clone(),
        install_dir: home.path().join("extensions").join("probe"),
        data_dir: home.path().join("data").join("extensions").join("probe"),
        ports: manifest.ports.clone(),
        manifest,
        source: crate::extensions::store::Source::Path,
        signed: false,
        installed_at: mixengine_proto::Timestamp::parse_rfc3339("2026-09-03T09:00:00Z")
            .expect("a timestamp"),
    };
    crate::extensions::store::remember(&generator.store, &installed)
        .await
        .expect("the row");

    let rendered = generator.declared().await.expect("a rendering");
    assert!(rendered[0].changed());

    let extensions = home.path().join("etc").join("front").join("extensions");

    assert_eq!(
        std::fs::read_to_string(extensions.join("probe.conf")).expect("the fragment"),
        "map $a $b { default 0; }\n",
        "the nginx fragment did not reach the nginx front end"
    );
    assert!(
        !extensions.join("probe.caddy").exists(),
        "the Caddy fragment was rendered by the nginx front end"
    );

    // The sweep is what makes an uninstall take the fragment with it — the design's D3.
    crate::extensions::store::forget(&generator.store, &installed.id)
        .await
        .expect("the extension is uninstalled");

    let after = generator.declared().await.expect("a second rendering");

    assert!(
        !extensions.join("probe.conf").exists(),
        "an uninstalled extension's fragment is still in the front end's configuration"
    );
    assert!(
        after[0].changed(),
        "the removal did not reach the reload, so the front end went on serving it"
    );
}

/// **A fragment the front end will not accept stops the install, and the home does not move** —
/// roadmap task **T81c**, the design's D5.
///
/// The judgement happens before anything is downloaded or written, so what this asserts beside
/// the refusal is that `etc/` is exactly as it was: no configuration directory for the front
/// end, and no staging directory left beside one.
#[tokio::test]
async fn a_fragment_the_front_end_refuses_stops_the_install_and_changes_nothing() {
    let (home, generator) = home_of(Arc::new(FussyFront), "front", "front", "{}").await;

    let pending = pending_extension(&home, "server = \"nginx\"\nfragment = \"probe on;\"\n");

    let error = generator
        .would_serve(&pending)
        .await
        .expect_err("a fragment the front end refuses");

    assert!(
        error.to_string().contains("unknown directive: probe"),
        "the front end's own complaint did not reach the caller: {error}"
    );

    let etc = home.path().join("etc");
    assert!(
        !etc.join("front").exists(),
        "the judgement installed the configuration it judged"
    );
    assert_eq!(
        std::fs::read_dir(&etc)
            .map(|entries| entries.count())
            .unwrap_or_default(),
        0,
        "the judgement left something behind in etc/"
    );
}

/// And an extension that adds nothing to the front end is not judged at all: there is nothing
/// about the front end's configuration for it to change, and a checker run per install would be
/// a process launch bought with nothing.
#[tokio::test]
async fn an_extension_with_no_fragment_is_not_put_in_front_of_the_checker() {
    let (home, generator) = home_of(Arc::new(FussyFront), "front", "front", "{}").await;

    let pending = pending_extension(&home, "");

    generator
        .would_serve(&pending)
        .await
        .expect("an extension with no fragment is nothing to judge");
}

/// An extension that is not installed, carrying whatever `[[recipe.front_end]]` entries the
/// caller spells out.
fn pending_extension(
    home: &tempfile::TempDir,
    front_end: &str,
) -> crate::extensions::store::Installed {
    let body = match front_end.is_empty() {
        true => "[[recipe.php_ini]]\nkey = \"a\"\nvalue = \"b\"\n".to_owned(),
        false => format!("[[recipe.front_end]]\n{front_end}"),
    };

    let manifest = crate::extensions::manifest::read(
        std::path::Path::new("extension.toml"),
        &format!(
            "schema = 1\n\n[extension]\nid = \"probe\"\nname = \"Probe\"\nversion = \"1.0.0\"\nkind = \"recipe\"\n\n{body}"
        ),
    )
    .expect("the manifest parses");

    crate::extensions::store::Installed {
        id: manifest.extension.id.clone(),
        install_dir: home.path().join("extensions").join("probe"),
        data_dir: home.path().join("data").join("extensions").join("probe"),
        ports: manifest.ports.clone(),
        manifest,
        source: crate::extensions::store::Source::Path,
        signed: false,
        installed_at: mixengine_proto::Timestamp::parse_rfc3339("2026-09-03T09:00:00Z")
            .expect("a timestamp"),
    }
}

/// **The daemon is told where to hold from the same pass that renders** — T70a.
///
/// Never from a query of its own, which is why `activators` goes through `declarations` and
/// why this does too: an address computed twice is an address that can be computed two ways,
/// and the two would diverge on exactly the recipe whose socket path depends on how deep this
/// home is.
#[tokio::test]
async fn a_service_reports_the_addresses_it_is_woken_at() {
    let (_home, generator) = home("{}").await;

    let held = generator
        .held_while_stopped()
        .await
        .expect("the addresses this home is woken at");

    assert_eq!(
        held.get(&ServiceId::parse("fakeservice@main").expect("an id")),
        Some(&vec![Upstream::Tcp(std::net::SocketAddr::from((
            std::net::Ipv4Addr::LOCALHOST,
            4321
        )))]),
        "the recipe's answer did not reach the caller: {held:?}"
    );
}

/// A home with a database, a package row and one service row for `recipe`'s package.
///
/// `instance` is the `instance_name` column rather than something derived here, because what a
/// row puts in it is exactly what these tests are about: an instance of a named package writes
/// the half after the `@`, and a package that exists once writes its own name.
async fn home_of(
    recipe: Arc<dyn Recipe>,
    id: &str,
    instance: &str,
    overrides: &str,
) -> (tempfile::TempDir, Generator) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let paths = Paths::new(directory.path().to_path_buf(), &PathOverrides::default());
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");
    let package = recipe.package();

    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-15T00:00:00Z', 'https://example', 'ab')",
    )
    .bind(package)
    .execute(store.pool())
    .await
    .expect("a package row");

    sqlx::query(
        "INSERT INTO services (id, package_id, instance_name, state, port,
                                   config_overrides_json)
             VALUES (?, (SELECT id FROM packages WHERE name = ?), ?, 'stopped', 4321, ?)",
    )
    .bind(id)
    .bind(package)
    .bind(instance)
    .bind(overrides)
    .execute(store.pool())
    .await
    .expect("a services row");

    let catalogue = Catalogue::builtin().with(recipe);

    (
        directory,
        Generator::new(paths, store, catalogue, Vec::new(), true),
    )
}

/// **A `services` row belonging to an extension renders the manifest it was installed from** —
/// roadmap task **T81**, the design's D7.
///
/// And it goes through the same walk everything else does, which is the point of giving an
/// extension a `services` row at all: the ports it holds, the limits its row carries and the
/// idle policy are applied by code that has no idea this one is an extension.
#[tokio::test]
async fn a_service_belonging_to_an_extension_renders_its_manifest() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let paths = Paths::new(directory.path().to_path_buf(), &PathOverrides::default());
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    let manifest = crate::extensions::manifest::read(
        std::path::Path::new("extension.toml"),
        mixengine_testkit::extension::MAILPIT,
    )
    .expect("the fixture parses");

    // The ports the *install* handed out, one of them moved off the number the manifest asked
    // for — which is what a spec rendered from the manifest's wishes would get wrong.
    let mut ports = manifest.ports.clone();
    ports.insert("ui_port".to_owned(), 18_025);

    let installed = crate::extensions::store::Installed {
        id: manifest.extension.id.clone(),
        install_dir: paths.extensions().join("mailpit"),
        data_dir: paths.data().join("extensions").join("mailpit"),
        ports,
        manifest,
        source: crate::extensions::store::Source::Registry,
        signed: true,
        installed_at: mixengine_proto::Timestamp::parse_rfc3339("2026-09-02T09:00:00Z")
            .expect("a timestamp"),
    };
    crate::extensions::store::remember(&store, &installed)
        .await
        .expect("the row");

    let data_dir = installed.data_dir.display().to_string();
    sqlx::query(
        "INSERT INTO services (id, extension_id, instance_name, state, port, data_dir)
             VALUES ('mailpit', 'mailpit', 'mailpit', 'stopped', 18025, ?)",
    )
    .bind(&data_dir)
    .execute(store.pool())
    .await
    .expect("a services row");

    let generator = Generator::new(paths, store, Catalogue::builtin(), Vec::new(), true);

    let generated = generator
        .generate(&ServiceId::parse("mailpit").expect("an id"))
        .await
        .expect("the extension's service renders");

    assert!(
        generated
            .spec
            .args()
            .contains(&"127.0.0.1:18025".to_owned()),
        "the spec was rendered from the manifest's wish rather than the allocated port: {:?}",
        generated.spec.args()
    );
    assert!(
        generated.spec.program().ends_with("mailpit"),
        "{:?}",
        generated.spec.program()
    );
}

/// A home holding one `fakeservice@main`, which is what most of these tests want.
async fn home(overrides: &str) -> (tempfile::TempDir, Generator) {
    home_of(Arc::new(Fake), "fakeservice@main", "main", overrides).await
}

/// The spec `fakeservice@main` renders to with `idle_minutes` set to `minutes`.
///
/// Writes the column rather than taking a second fixture parameter: the join under test is
/// between a *row* and a recipe, and the three states of that column are the whole question.
async fn idle_of(
    generator: &Generator,
    minutes: Option<i64>,
) -> Option<mixengine_proto::IdlePolicy> {
    sqlx::query("UPDATE services SET idle_minutes = ? WHERE id = 'fakeservice@main'")
        .bind(minutes)
        .execute(generator.store.pool())
        .await
        .expect("the column is written");

    generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a generated service")
        .spec
        .idle()
        .cloned()
}

/// The row decides how long, the recipe decides how, and `0` outranks both.
///
/// Three assertions because the column has three states, and the third is the one that exists
/// for **T70**'s sake rather than for anything reachable today: a default arriving in a later
/// release must reach the home that never touched this setting and must not reach the one whose
/// owner switched it off.
#[tokio::test]
async fn an_idle_policy_is_joined_from_the_row_and_the_recipe() {
    let (_home, generator) = home("{}").await;

    assert_eq!(
        idle_of(&generator, None).await,
        None,
        "a row that has not asked idles nothing, because no recipe here offers a default"
    );

    let policy = idle_of(&generator, Some(30))
        .await
        .expect("the row asked for a policy");

    assert_eq!(policy.after, mixengine_proto::Millis::from_secs(30 * 60));
    assert_eq!(
        policy.probe,
        mixengine_proto::IdleProbe::Connections { port: 4321 },
        "the probe is the recipe's, over the port the row allocated"
    );

    assert_eq!(
        idle_of(&generator, Some(0)).await,
        None,
        "zero minutes is never, not immediately"
    );
}

/// **T167b, ADR 0041.** A row nobody set is idle-stopped only while the home saves resources,
/// and then on the recipe's saving number; a row somebody set means what they said either way.
#[tokio::test]
async fn a_row_nobody_set_idles_only_while_the_home_saves_resources() {
    let (_home, generator) = home("{}").await;
    let minutes = |policy: Option<mixengine_proto::IdlePolicy>| policy.map(|p| p.after);

    assert_eq!(
        idle_of(&generator, None).await,
        None,
        "off: nothing is stopped"
    );

    crate::services::save_resources::set(&generator.store, true)
        .await
        .expect("the switch is written");

    assert_eq!(
        minutes(idle_of(&generator, None).await),
        Some(mixengine_proto::Millis::from_secs(45 * 60)),
        "on: the recipe's saving number"
    );
    assert_eq!(
        minutes(idle_of(&generator, Some(10)).await),
        Some(mixengine_proto::Millis::from_secs(10 * 60)),
        "a person's number outranks the switch"
    );
    assert_eq!(
        idle_of(&generator, Some(0)).await,
        None,
        "never stays never"
    );
}

#[tokio::test]
async fn a_row_becomes_a_spec_and_a_file_on_disk() {
    let (_home, generator) = home("{}").await;

    let generated = generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a generated service");

    assert_eq!(generated.spec.id().as_str(), "fakeservice@main");
    assert!(generated.changed(), "nothing was written on a first render");

    // **Roadmap task T71a**: the recipe's answer reaches the spec. This fixture says `true`
    // where the trait's default is `false`, so an assertion here fails if the generator stops
    // asking rather than passing for the wrong reason.
    assert!(
        generated.spec.restart_over_memory(),
        "the recipe's permission is carried onto the spec it renders"
    );

    let rendered = std::fs::read_to_string(
        generator
            .paths
            .etc()
            .join("fakeservice@main")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file");

    assert!(rendered.contains("say = hello"), "{rendered}");
    assert!(rendered.contains("port = 4321"), "{rendered}");
}

/// **A number read without writing anything** — roadmap task **T53**.
///
/// `mix cert status` connects to the front end's TLS port, and the two other ways to learn it
/// are both refused. [`Generator::generate`] goes through [`Generator::declared`], which
/// *installs*: a read-only status command would rewrite this home's whole configuration in
/// order to read one number, and can reload a running server as a side effect of being asked a
/// question. Parsing the rendered file back is what `CLAUDE.md` forbids outright.
#[tokio::test]
async fn a_services_settings_can_be_read_without_writing_anything() {
    let (_home, generator) = home(r##"{"greeting": "guten tag"}"##).await;
    let id = ServiceId::parse("fakeservice@main").expect("an id");

    let settings = generator.settings(&id).await.expect("its settings");

    assert_eq!(settings.text(GREETING), "guten tag");

    // The guarantee, and the whole reason this is a method rather than a field on `Generated`.
    assert!(
        !generator.paths.etc().join("fakeservice@main").exists(),
        "reading a setting installed a configuration"
    );
}

/// And a service nothing declares is `NotFound` rather than a default nobody set.
#[tokio::test]
async fn the_settings_of_a_service_that_does_not_exist_are_not_found() {
    let (_home, generator) = home("{}").await;
    let id = ServiceId::parse("fakeservice@other").expect("an id");

    assert!(matches!(
        generator.settings(&id).await,
        Err(Error::NotFound {
            kind: "service",
            ..
        })
    ));
}

#[tokio::test]
async fn an_override_reaches_the_rendered_file() {
    let (_home, generator) = home(r##"{"greeting": "guten tag", "extra": "# mine\n"}"##).await;

    generator.declared().await.expect("the declared set");

    let rendered = std::fs::read_to_string(
        generator
            .paths
            .etc()
            .join("fakeservice@main")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file");

    assert!(rendered.contains("say = guten tag"), "{rendered}");
    assert!(rendered.contains("# mine"), "{rendered}");
}

/// The second walk over an unchanged home is what the registry does on every `service.*` call.
#[tokio::test]
async fn generating_twice_changes_nothing_the_second_time() {
    let (_home, generator) = home("{}").await;

    generator.declared().await.expect("a first walk");
    let again = generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a second walk");

    assert!(!again.changed(), "an unchanged home rewrote its config");
}

/// The failure a home meets after an upgrade that dropped a recipe, and the one every home
/// meets today for a package this build does not know.
#[tokio::test]
async fn a_package_with_no_recipe_names_itself_and_what_is_known() {
    let (_home, generator) = home("{}").await;

    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES ('meilisearch', '1.0.0', '/packages/x', '2026-08-15T00:00:00Z',
                     'https://example', 'ab');
             INSERT INTO services (id, package_id, instance_name, state)
             VALUES ('meilisearch@main', (SELECT id FROM packages WHERE name = 'meilisearch'),
                     'main', 'stopped')",
    )
    .execute(generator.store.pool())
    .await
    .expect("a service belonging to something this build cannot run");

    let error = generator
        .declared()
        .await
        .expect_err("a package with no recipe");

    let message = error.to_string();
    assert!(message.contains("meilisearch"), "{message}");
}

/// A singleton's data directory is `data/<package>`, and not `data/<package>/<package>`.
///
/// The fallback was written for the case that has an instance name to spend. A recipe that
/// exists once has no such half, and repeating the package name reads as a mistake to whoever
/// meets it in a directory listing.
#[tokio::test]
async fn a_single_instance_recipe_keeps_its_data_directly_under_the_package() {
    let (_home, generator) = home_of(Arc::new(Solo), "solo", "solo", "{}").await;

    let generated = generator
        .generate(&ServiceId::parse("solo").expect("an id"))
        .await
        .expect("a generated service");

    assert_eq!(generated.spec.cwd(), generator.paths.data().join("solo"));
}

/// A named-instance recipe keeps the shape it always had: siblings under one package.
#[tokio::test]
async fn a_named_instance_recipe_keeps_its_data_under_the_instance() {
    let (_home, generator) = home("{}").await;

    let generated = generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a generated service");

    assert_eq!(
        generated.spec.cwd(),
        generator.paths.data().join("fakeservice").join("main")
    );
}

/// A row that is fine except for its overrides fails as *that service*, not as a broken home.
#[tokio::test]
async fn a_misspelled_override_names_the_service_it_is_on() {
    let (_home, generator) = home(r#"{"greting": "hello"}"#).await;

    let error = generator
        .declared()
        .await
        .expect_err("a misspelled setting");
    let message = error.to_string();

    assert!(message.contains("fakeservice@main"), "{message}");
    assert!(message.contains("greting"), "{message}");
}
/// A service whose binary comes from an installed runtime renders exactly like one whose binary
/// comes from a package.
///
/// What is being asserted is the join and nothing else: the recipe is the same [`Fake`], the
/// context it receives carries the runtime's version and install path, and **the name the recipe
/// was found under is the id's own** — a pool is `php-fpm@8.3.33` and the row beneath it says
/// `php`, which is the one place those two differ.
#[tokio::test]
async fn a_runtime_backed_row_renders_from_the_runtime_it_names() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let paths = Paths::new(directory.path().to_path_buf(), &PathOverrides::default());
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    sqlx::query(
        r#"INSERT INTO runtime_installs
                   (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, provides_json)
               VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-08-19T00:00:00Z',
                       1, 'https://example.invalid/php', 'abc', '{"php-fpm":"sbin/php-fpm"}')"#,
    )
    .execute(store.pool())
    .await
    .expect("a runtime install");

    sqlx::query(
        "INSERT INTO services (id, runtime_install_id, instance_name, state, port)
             VALUES ('fakeservice@8.3.33', (SELECT id FROM runtime_installs LIMIT 1),
                     '8.3.33', 'stopped', 9000)",
    )
    .execute(store.pool())
    .await
    .expect("a service over it");

    let generator = Generator::new(
        paths.clone(),
        store,
        Catalogue::default().with(Arc::new(Fake)),
        Vec::new(),
        true,
    );

    let generated = generator.declared().await.expect("one rendered service");

    assert_eq!(
        generated.len(),
        1,
        "a row with no packages parent was dropped by the join"
    );
    assert_eq!(generated[0].spec.id().as_str(), "fakeservice@8.3.33");

    let rendered = std::fs::read_to_string(
        paths
            .etc()
            .join("fakeservice@8.3.33")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file");

    assert!(rendered.contains("port = 9000"), "{rendered}");
}
/// The question `mix doctor`'s tenth check asks: is what is on disk what these rows render to?
///
/// **The control is the first half.** A home that has never been rendered must drift, or an
/// empty answer after the render is evidence of a blind comparison rather than of a clean home.
#[tokio::test]
async fn drift_is_something_before_a_render_and_nothing_after_one() {
    let (_home, generator) = home("{}").await;

    let before = generator.drift().await.expect("a drift");
    assert!(
        before.iter().any(|one| !one.drift.is_empty()),
        "a home that was never rendered reported no drift: {before:?}"
    );

    generator.declared().await.expect("a rendering");

    let after = generator.drift().await.expect("a second drift");
    assert!(
        after.iter().all(|one| one.drift.is_empty()),
        "a home that was just rendered still drifts: {after:?}"
    );
}

/// The id [`CertifiedFake`] renders under, and where its pair lands.
fn fakeservice() -> ServiceId {
    ServiceId::parse("fakeservice@main").expect("an id")
}

/// The one file [`Fake`] and [`CertifiedFake`] render, as installed.
fn rendered_file(home: &std::path::Path) -> String {
    std::fs::read_to_string(
        home.join("etc")
            .join("fakeservice@main")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file is on disk")
}

/// **A recipe that asks gets a pair this home's authority signed, and the file names it** —
/// roadmap task **T99**, the design's D4 and D5.
#[tokio::test]
async fn a_recipe_that_wants_a_certificate_gets_one_on_a_home_with_an_authority() {
    let (home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    generator.declared().await.expect("a rendering");

    let key = crate::certs::service::key_path(&certs, &fakeservice());
    let certificate = crate::certs::service::certificate_path(&certs, &fakeservice());
    assert!(key.is_file(), "{}", key.display());
    assert!(certificate.is_file(), "{}", certificate.display());

    let mixengine_proto::CertState::Present { cert } =
        crate::certs::service::read(&certs, &fakeservice(), std::time::SystemTime::now())
    else {
        panic!("the pair on disk is not usable");
    };

    let file = rendered_file(home.path());
    assert!(
        file.contains(&format!("cert = {}", certificate.display())),
        "{file}"
    );
    assert!(file.contains(&format!("key = {}", key.display())), "{file}");
    assert!(file.contains(&format!("# {}", cert.fingerprint)), "{file}");
}

/// **No authority, no certificate, and still a rendering** — the design's D4: the failure mode
/// is the status quo, never a service that will not start.
#[tokio::test]
async fn a_recipe_that_wants_a_certificate_still_renders_on_a_home_with_none() {
    let (home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;

    generator.declared().await.expect("a rendering");

    let file = rendered_file(home.path());
    assert!(!file.contains("cert ="), "{file}");
    assert!(
        !generator.paths.certs().join("services").exists(),
        "something was written under certs/services without an authority"
    );
}

/// A recipe that does not ask is never issued one, authority or not.
#[tokio::test]
async fn a_recipe_that_wants_none_gets_none() {
    let (home, generator) = home("{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    generator.declared().await.expect("a rendering");

    assert!(!certs.join("services").exists());
    assert!(!rendered_file(home.path()).contains("cert ="));
}

/// **Reading in `prepare` is what keeps `drift` honest** — the design's D4. A drift check
/// renders without issuing, so it has to see the pair the install saw, or every home with a
/// database would drift for ever.
#[tokio::test]
async fn drift_is_nothing_after_a_render_that_issued() {
    let (_home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    let before = generator.drift().await.expect("a drift");
    assert!(before.iter().any(|one| !one.drift.is_empty()), "{before:?}");

    generator.declared().await.expect("a rendering");

    let after = generator.drift().await.expect("a second drift");
    assert!(after.iter().all(|one| one.drift.is_empty()), "{after:?}");
}

/// The four questions hold across renders: a second one changes neither the pair nor the file.
#[tokio::test]
async fn a_second_generate_reuses_the_pair() {
    let (home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    generator.declared().await.expect("a rendering");
    let first = rendered_file(home.path());

    let again = generator.declared().await.expect("a second rendering");
    assert!(!again[0].changed(), "a reused pair rewrote the file");
    assert_eq!(first, rendered_file(home.path()));
}

/// Asking must not install, or the check would repair what it was sent to report.
#[tokio::test]
async fn asking_for_drift_installs_nothing() {
    let (_home, generator) = home("{}").await;

    generator.drift().await.expect("a drift");

    let rendering = generator.declared().await.expect("a rendering");

    assert!(
        rendering[0].changed(),
        "the drift call had already written the configuration"
    );
}

/// A generated file somebody edited by hand is drift, and names the service it belongs to.
#[tokio::test]
async fn a_generated_file_edited_by_hand_drifts_under_its_own_service() {
    let (home_dir, generator) = home("{}").await;

    let rendered = generator.declared().await.expect("a rendering");
    let (file, _) = rendered[0]
        .files
        .first()
        .expect("the recipe renders at least one file");

    let quiet = generator.drift().await.expect("a drift");
    let service = quiet[0].service.clone();
    assert!(quiet[0].drift.is_empty(), "{quiet:?}");

    std::fs::write(
        home_dir
            .path()
            .join("etc")
            .join(service.as_str())
            .join(file),
        "tampered\n",
    )
    .expect("the generated file is writable");

    let after = generator.drift().await.expect("a second drift");

    assert_eq!(after[0].service, service);
    assert_eq!(after[0].drift.changed, vec![file.clone()]);
}
