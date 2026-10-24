use super::*;

fn php(bundle: Option<PathBuf>) -> State {
    State {
        kind: RuntimeKind::Php,
        version: PackageVersion::parse("8.4.24").expect("a version"),
        install_path: PathBuf::from("/home/runtimes/php/8.4.24"),
        directory: Some("ext".to_owned()),
        offered: crate::index::Extensions::default(),
        choices: BTreeMap::new(),
        additions: Vec::new(),
        ca_bundle: bundle,
    }
}

fn mixengine_ini(state: &State) -> String {
    state
        .documents()
        .into_iter()
        .find(|document| document.relative() == std::path::Path::new(MIXENGINE_INI))
        .expect("every PHP renders 00-mixengine.ini")
        .contents()
        .to_owned()
}

/// **PHP is told through the ini set rather than through the environment.** `PHP_INI_SCAN_DIR`
/// names this directory for a `php` in a terminal *and* is set on the php-fpm spec, so `php -r` and
/// `curl_exec()` in a browser get the same answer. That is what makes a site of this home reachable
/// over HTTPS from another site of this home.
#[test]
fn the_generated_ini_names_the_bundle() {
    let rendered = mixengine_ini(&php(Some(PathBuf::from("/home/etc/ca/bundle.pem"))));

    assert!(rendered.contains("openssl.cafile"), "{rendered}");
    assert!(rendered.contains("curl.cainfo"), "{rendered}");
    assert_eq!(
        rendered.matches("bundle.pem").count(),
        2,
        "PHP has two TLS stacks and each reads its own directive: {rendered}"
    );
}

/// And says nothing when there is no bundle, rather than naming a file nothing wrote — which on
/// Windows would be worse than silence, since the artifact ships no CA file and PHP would be
/// left pointing at an absence instead of falling back to the machine's own stack.
#[test]
fn no_bundle_writes_no_certificate_lines() {
    let rendered = mixengine_ini(&php(None));

    assert!(!rendered.contains("openssl.cafile"), "{rendered}");
    assert!(!rendered.contains("curl.cainfo"), "{rendered}");
}

/// A runtime that declares no extension directory renders no ini at all, bundle or not: this is
/// the rule the whole module keys off, and a certificate setting must not be the thing that
/// starts giving Node a `conf.d`.
#[test]
fn a_runtime_with_no_extension_directory_still_renders_nothing() {
    let node = State {
        kind: RuntimeKind::Node,
        directory: None,
        ..php(Some(PathBuf::from("/home/etc/ca/bundle.pem")))
    };

    assert!(node.documents().is_empty());
}
