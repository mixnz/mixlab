//! A front end's sites, its extensions' fragments, and the addresses a service is woken at.

use super::*;

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

/// **An extension's fragment lands in the front end's own `etc/`, and leaves with it.**
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

/// **A fragment the front end will not accept stops the install, and the home does not move.**
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

/// **The daemon is told where to hold from the same pass that renders.**
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
