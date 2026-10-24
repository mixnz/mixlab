use std::collections::BTreeMap;
use std::path::Path;

use mixengine_platform::PortBinding;
use mixengine_proto::ServiceId;

use super::*;
use crate::generate::recipe;
use crate::generate::recipe::FrontEndAddition;
use crate::generate::served::Shared;
use crate::generate::settings::Settings;

mod routing;
mod server;
mod sites;

/// A document root on whichever system this is compiled for.
fn doc_root() -> std::path::PathBuf {
    if cfg!(windows) {
        std::path::PathBuf::from(r"C:\src\blog\public")
    } else {
        std::path::PathBuf::from("/src/blog/public")
    }
}

/// A site shared on the LAN.
fn a_shared_site(address: [u8; 4]) -> Served {
    Served {
        shared: Some(Shared {
            address: address.into(),
            name: Some("blog-mixengine.local".to_owned()),
        }),
        domains: vec!["blog.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::Static,
        https: false,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    }
}

fn a_site_with_a_certificate() -> Served {
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
            certificate: std::path::PathBuf::from(
                "/home/someone/.mixengine/certs/sites/blog.test.crt",
            ),
            key: std::path::PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.key"),
            fingerprint: "ab".repeat(32),
        }),
        steps: Vec::new(),
    }
}

fn render_site(site: &Served) -> String {
    Caddy
        .sites(&context("{}"), std::slice::from_ref(site))
        .expect("one site file")[0]
        .contents()
        .to_owned()
}

/// A Caddy on port 80 in a home at `root`, with `overrides` applied.
///
/// The root is a plain string rather than a temporary directory: nothing here writes a file, and
/// what the assertions are about is the *text* a path becomes. On Windows that text contains
/// backslashes, which is the whole subject of one of these tests.
fn context(overrides: &str) -> Context {
    let service = ServiceId::parse("caddy").expect("an id");
    let settings =
        Settings::merge(Caddy.settings(), overrides, &service).expect("usable overrides");

    Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        BTreeMap::new(),
        Some(80),
        settings,
    )
}

/// An absolute path on whichever system this is compiled for.
const fn root() -> &'static str {
    if cfg!(windows) {
        r"C:\MixEngine"
    } else {
        "/opt/mixengine"
    }
}
