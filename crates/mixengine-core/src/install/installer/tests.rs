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

/// The last segment of a URL becomes a file name.
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
