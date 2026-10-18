use std::path::{Path, PathBuf};

use mixengine_platform::PortBinding;
use mixengine_proto::{HealthProbe, ReadyCheck, ReloadBehaviour, ServiceId, StopBehaviour};

use super::*;
use crate::generate::recipe;
use crate::generate::recipe::FrontEndAddition;
use crate::generate::served::Shared;
use crate::generate::settings::Settings;

mod listening;
mod server;
mod sites;

/// An absolute path on whichever system this is compiled for.
const fn root() -> &'static str {
    if cfg!(windows) {
        r"C:\MixEngine"
    } else {
        "/opt/mixengine"
    }
}

/// Where the binary sits inside the archive, as the index publishes it.
fn nginx_binary() -> String {
    format!("nginx{}", std::env::consts::EXE_SUFFIX)
}

/// An nginx on port 80 in a home at [`root`], with `overrides` applied.
///
/// The root is a plain string rather than a temporary directory, for [`super::super::caddy`]'s
/// reason: nothing here writes a file, and what the assertions are about is the *text* a path
/// becomes. On Windows that text contains backslashes, which is the subject of one of these.
fn context(overrides: &str) -> Context {
    context_on(overrides, Some(80))
}

/// The same, with the port the row carries spelled out — what D8's half of this recipe is about.
fn context_on(overrides: &str, port: Option<u16>) -> Context {
    let service = ServiceId::parse("nginx").expect("an id");
    let settings =
        Settings::merge(Nginx.settings(), overrides, &service).expect("usable overrides");

    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        // What `mixengine-packages` publishes: the server, and the data files a generated
        // configuration includes. `mime.types` is the one nothing works without.
        [
            ("nginx".to_owned(), nginx_binary()),
            (MIME_TYPES.to_owned(), "conf/mime.types".to_owned()),
            (FASTCGI_PARAMS.to_owned(), "conf/fastcgi_params".to_owned()),
        ]
        .into_iter()
        .collect(),
        port,
        settings,
    );

    // What `Generator::render` does before it renders anything, and this template needs it: the
    // `include` of the archive's own `mime.types` is resolved here rather than joined in the
    // file.
    let endpoints = Nginx
        .endpoints(&context)
        .expect("a package publishing what a generated configuration includes");

    context.with_endpoints(endpoints)
}

/// A document root on whichever system this is compiled for.
fn doc_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\src\blog\public")
    } else {
        PathBuf::from("/src/blog/public")
    }
}

/// A php-fpm site at `blog.test`, on the static fixture's domain and certificate.
fn a_php_site() -> Served {
    Served {
        kind: ServedKind::PhpFpm {
            upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
            activator: None,
        },
        ..a_static_site()
    }
}

/// A static site at `blog.test` with a certificate, so both shapes of the template are
/// reachable from one fixture.
fn a_static_site() -> Served {
    Served {
        shared: None,
        domains: vec!["blog.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::Static,
        https: true,
        https_redirect: false,
        certificate: Some(crate::generate::served::SiteCertificate {
            certificate: std::path::PathBuf::from("/certs/blog.test.crt"),
            key: std::path::PathBuf::from("/certs/blog.test.key"),
            fingerprint: "ab".repeat(32),
        }),
        steps: Vec::new(),
    }
}

/// One site through the real recipe.
fn render_site(site: &Served) -> String {
    Nginx
        .sites(&context("{}"), std::slice::from_ref(site))
        .expect("one site file")[0]
        .contents()
        .to_owned()
}
