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

    /// Set so that the generator carrying it onto the spec is observable.
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

    /// Forty-five minutes while the home saves resources. A number of its own so the
    /// test below can tell it from the row's.
    fn idle_when_saving(&self) -> Option<mixengine_proto::Millis> {
        Some(mixengine_proto::Millis::from_secs(45 * 60))
    }

    /// On the port the row allocated, as every database recipe answers.
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

/// [`Fake`], asking for a certificate.
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

/// The same front end with a checker that refuses everything.
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

mod certificates;
mod front_end;
mod services;

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

/// A home holding one `fakeservice@main`, which is what most of these tests want.
async fn home(overrides: &str) -> (tempfile::TempDir, Generator) {
    home_of(Arc::new(Fake), "fakeservice@main", "main", overrides).await
}
