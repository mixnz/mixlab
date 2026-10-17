use super::*;

#[test]
fn staging_sits_beside_the_destination_and_keeps_the_whole_version_in_its_name() {
    let staging = staging_for(Path::new("/home/runtimes/php/8.3.33")).expect("a target");

    assert_eq!(
        staging,
        Path::new("/home/runtimes/php/.8.3.33.staging"),
        "a rename is atomic only within one filesystem, and `8.3` would be another version"
    );
}

/// The last segment of a URL becomes a file name — roadmap task **T82**, the design's D3.
///
/// A query string or a fragment is not part of it, the way [`archive::Format::of`] already
/// treats them: a mirror that appends one still installs rather than failing to classify.
#[test]
fn a_one_file_artifact_takes_its_name_from_the_url() {
    assert_eq!(
        one_file_name("https://example.invalid/adminer-6.0.1.php").expect("a name"),
        "adminer-6.0.1.php"
    );
    assert_eq!(
        one_file_name("https://example.invalid/a/b/adminer.php?v=2#x").expect("a name"),
        "adminer.php"
    );
}

/// **A name that came out of a document can never be a path**, which is [`Installer::part_file`]'s
/// own rule read in the other direction: there it is why a `.part` file is named after a hash,
/// and here it is why nothing whose last segment is not a bare file name may be installed.
#[test]
fn a_one_file_artifact_whose_url_names_no_file_is_refused() {
    for url in [
        "https://example.invalid/",
        "https://example.invalid/adminer/",
        "https://example.invalid/..",
        "https://example.invalid/.",
        "https://example.invalid/a\\b",
    ] {
        assert!(one_file_name(url).is_none(), "{url} is not a file name");
    }
}

#[test]
fn the_bar_is_weighted_towards_the_part_that_takes_the_time() {
    assert_eq!(downloaded(0, 1_000), 0);
    assert_eq!(downloaded(500, 1_000), DOWNLOADED_AT / 2);
    assert_eq!(downloaded(1_000, 1_000), DOWNLOADED_AT);
    // An index that declares nothing is not a division by zero.
    assert_eq!(downloaded(0, 0), DOWNLOADED_AT);
    // And a body longer than declared cannot push the bar past its share on its way to being
    // refused.
    assert_eq!(downloaded(4_000, 1_000), DOWNLOADED_AT);
}

/// The ordinary case is unchanged: whatever the operating system said, and nothing added.
#[test]
fn a_program_that_is_simply_missing_says_only_what_the_os_said() {
    let source = std::io::Error::from(std::io::ErrorKind::NotFound);

    assert_eq!(why_it_would_not_start(&source), source.to_string());
}

/// **A refused image load is a machine-wide condition and no amount of re-running fixes it** —
/// roadmap task **T94** — so the detail carries the reason nobody would guess from `4551`.
///
/// **Both arms in one test, through `cfg!` as a value.** The same number means nothing on macOS
/// or Linux, and a `#[cfg(windows)]` here would be this crate compiling a line away by
/// operating system — which `workspace_layering` refuses, and rightly: the classifier's own
/// `cfg!(windows)` is the thing under test, so asserting it from the caller's side is what
/// keeps the two from drifting.
#[test]
fn a_refused_image_load_is_named_exactly_where_that_number_can_mean_it() {
    let source = std::io::Error::from_raw_os_error(mixengine_platform::APPLICATION_CONTROL_BLOCKED);

    let detail = why_it_would_not_start(&source);

    assert_eq!(
        detail.contains("application control policy"),
        cfg!(windows),
        "{detail}"
    );
}

#[test]
fn a_hash_is_lowercase_hex_of_the_file_and_not_of_its_name() {
    let file = tempfile::NamedTempFile::new().expect("a temporary file");
    std::fs::write(file.path(), b"abc").expect("write");

    assert_eq!(
        sha256(file.path()).expect("hash it"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "the published vector for SHA-256 of \"abc\""
    );
}

/// A payload row, of the shape `packaging/feed.sh` writes one.
fn a_payload() -> Artifact {
    Artifact {
        os: crate::index::Os::Macos,
        arch: crate::index::Arch::X86_64,
        url: "https://example.invalid/payload.tar.gz".to_owned(),
        sha256: "00".to_owned(),
        size: 1,
        provides: std::collections::BTreeMap::new(),
        requires: crate::index::Requires::default(),
        extension_dir: None,
        extensions: crate::index::Extensions::default(),
        lacks: std::collections::BTreeMap::new(),
    }
}

/// The macOS window, as that payload carries it: one entry, and it is a directory.
fn a_bundle_payload() -> Artifact {
    let mut artifact = a_payload();
    artifact.provides = [("mixlab".to_owned(), "mixengine/MixLab.app".to_owned())]
        .into_iter()
        .collect();
    artifact
}

/// T106. macOS wraps a windowed application in a directory, and `packaging/feed.sh` names that
/// directory in the payload's `provides` — so an artifact carrying one must stage rather than be
/// refused, and being refused would take the four ordinary binaries down with it.
#[test]
fn a_provides_entry_may_name_a_directory() {
    let staging = tempfile::tempdir().expect("a temporary directory");
    std::fs::create_dir_all(staging.path().join("mixengine/MixLab.app/Contents/MacOS"))
        .expect("a bundle");
    std::fs::write(
        staging
            .path()
            .join("mixengine/MixLab.app/Contents/MacOS/mixlab"),
        b"not a binary",
    )
    .expect("the executable inside it");

    present(&a_bundle_payload(), staging.path()).expect("a bundle is what this payload provides");
}

/// And the teeth are kept: an entry that resolves to an empty directory is the shape a bundle
/// copied by something that did not follow it takes, and it is not a payload anybody can use.
#[test]
fn a_provides_entry_naming_an_empty_directory_is_refused() {
    let staging = tempfile::tempdir().expect("a temporary directory");
    std::fs::create_dir_all(staging.path().join("mixengine/MixLab.app")).expect("an empty one");

    assert!(matches!(
        present(&a_bundle_payload(), staging.path()),
        Err(Error::Install(InstallError::MissingFromArtifact { .. }))
    ));
}

/// The ordinary case, unchanged: a name the payload does not carry at all is refused, which is
/// the packaging bug this check exists for.
#[test]
fn a_provides_entry_that_is_not_there_is_refused() {
    let staging = tempfile::tempdir().expect("a temporary directory");

    assert!(matches!(
        present(&a_bundle_payload(), staging.path()),
        Err(Error::Install(InstallError::MissingFromArtifact { .. }))
    ));
}
