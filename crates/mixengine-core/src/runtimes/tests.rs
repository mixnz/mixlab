use super::*;

const NOW: Timestamp = Timestamp(1_760_000_000_000);

async fn store() -> (tempfile::TempDir, Store) {
    let home = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
        .await
        .expect("a database");
    (home, store)
}

fn version(text: &str) -> PackageVersion {
    PackageVersion::parse(text).expect("a valid version")
}

/// **A restored default is still the default** — T206d, D3.
#[tokio::test]
async fn restoring_a_runtime_keeps_its_default() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("recorded");
    let restored = restore(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("restored");
    assert!(restored.default);
    assert_eq!(records(&store, None).await.expect("a listing").len(), 1);
}

#[tokio::test]
async fn restoring_a_runtime_never_recorded_is_not_found() {
    let (_home, store) = store().await;
    let error = restore(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect_err("no row");
    assert!(matches!(error, Error::NotFound { .. }), "{error}");
}

fn installation(kind: RuntimeKind, text: &str) -> Installation {
    Installation {
        kind,
        version: version(text),
        channel: PackageChannel::Stable,
        path: PathBuf::from("/home/runtimes")
            .join(kind.as_str())
            .join(text),
        bytes: 41_000_000,
        url: format!("https://example.invalid/{kind}-{text}.tar.zst"),
        sha256: "00".to_owned(),
        // The shape the index publishes: a name of ours against the path the publisher shipped
        // it at, which on this side of the archive is a nested one.
        provides: [
            ("php".to_owned(), "bin/php".to_owned()),
            ("php-config".to_owned(), "bin/php-config".to_owned()),
        ]
        .into_iter()
        .collect(),
        extension_dir: Some("lib/php/extensions".to_owned()),
        extensions: crate::index::Extensions {
            compiled_in: vec!["core".to_owned(), "date".to_owned()],
            shared: vec!["redis".to_owned(), "xdebug".to_owned()],
            enabled: vec!["redis".to_owned()],
        },
    }
}

/// The index is a cache with a network behind it; whether `redis` can be enabled for a PHP that
/// is on this disk must not depend on either.
#[tokio::test]
async fn what_a_build_offers_is_written_down_beside_it() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");

    let row = sqlx::query!(
        "SELECT extension_dir, extensions_json, extension_choices_json
             FROM runtime_installs WHERE kind = 'php' AND version = '8.3.33'"
    )
    .fetch_one(store.pool())
    .await
    .expect("the row");

    assert_eq!(row.extension_dir, "lib/php/extensions");

    let offered: crate::index::Extensions =
        serde_json::from_str(&row.extensions_json).expect("what the artifact published");
    assert_eq!(offered.enabled, ["redis"]);
    assert_eq!(offered.shared, ["redis", "xdebug"]);

    assert_eq!(
        row.extension_choices_json, "{}",
        "an install has made no choices on the user's behalf"
    );
}

/// A home whose only PHP is not the default is a home where `php` resolves to nothing.
#[tokio::test]
async fn the_first_version_of_a_kind_becomes_its_default_and_the_next_does_not() {
    let (_home, store) = store().await;

    let first = remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");
    let second = remember(&store, &installation(RuntimeKind::Php, "8.2.20"), NOW)
        .await
        .expect("a row");

    assert!(first.default, "the first of its kind");
    assert!(
        !second.default,
        "an install that moved what `php` means would break a project nobody was thinking about"
    );

    // And the kind's default is untouched by the second install.
    assert!(
        record(&store, RuntimeKind::Php, &version("8.3.33"))
            .await
            .expect("the row")
            .default
    );
}

/// Every kind counts its own, so installing Node does not leave PHP without a default.
#[tokio::test]
async fn a_default_is_per_kind() {
    let (_home, store) = store().await;

    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");
    let node = remember(&store, &installation(RuntimeKind::Node, "20.11.0"), NOW)
        .await
        .expect("a row");

    assert!(node.default, "the first node, whatever php has");
}

#[tokio::test]
async fn the_same_version_is_not_recorded_twice() {
    let (_home, store) = store().await;

    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");
    let error = remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect_err("it is already there");

    assert!(
        matches!(&error, Error::Runtime(RuntimeError::AlreadyRecorded { version, .. }) if version.as_str() == "8.3.33"),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_default_moves_and_only_one_version_holds_it() {
    let (_home, store) = store().await;

    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");
    remember(&store, &installation(RuntimeKind::Php, "8.2.20"), NOW)
        .await
        .expect("a row");

    let moved = set_default(&store, RuntimeKind::Php, &version("8.2.20"))
        .await
        .expect("it is installed");
    assert!(moved.default);

    let defaults: Vec<String> = records(&store, Some(RuntimeKind::Php))
        .await
        .expect("a listing")
        .into_iter()
        .filter(|runtime| runtime.default)
        .map(|runtime| runtime.version.as_str().to_owned())
        .collect();

    assert_eq!(defaults, ["8.2.20"], "one default per kind, and it moved");
}

/// The verb has to be idempotent, per the API's own rule about verbs it makes sense for.
#[tokio::test]
async fn making_the_default_the_default_again_changes_nothing() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");

    let again = set_default(&store, RuntimeKind::Php, &version("8.3.33"))
        .await
        .expect("it is installed");

    assert!(again.default);
}

#[tokio::test]
async fn a_version_that_is_not_installed_cannot_become_the_default() {
    let (_home, store) = store().await;

    let error = set_default(&store, RuntimeKind::Php, &version("8.3.33"))
        .await
        .expect_err("nothing is installed");

    assert!(
        matches!(&error, Error::NotFound { kind: "runtime", id } if id == "php 8.3.33"),
        "{error:?}"
    );
}

/// Nothing is promoted, and the caller is told — which is the whole of what `default_cleared`
/// on the wire says.
#[tokio::test]
async fn forgetting_the_default_leaves_the_kind_without_one() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");
    remember(&store, &installation(RuntimeKind::Php, "8.2.20"), NOW)
        .await
        .expect("a row");

    assert!(
        forget(&store, RuntimeKind::Php, &version("8.3.33"))
            .await
            .expect("it is installed"),
        "the one that was the default"
    );
    assert!(
        !records(&store, Some(RuntimeKind::Php))
            .await
            .expect("a listing")
            .iter()
            .any(|runtime| runtime.default),
        "8.2.20 is not promoted: an uninstall does not get to move what `php` means"
    );
    assert!(
        !forget(&store, RuntimeKind::Php, &version("8.2.20"))
            .await
            .expect("it is installed"),
        "and the other one never was"
    );
    assert!(
        !any_installed(&store, RuntimeKind::Php)
            .await
            .expect("a count")
    );
}

#[tokio::test]
async fn forgetting_something_that_is_not_there_says_so() {
    let (_home, store) = store().await;

    let error = forget(&store, RuntimeKind::Php, &version("8.3.33"))
        .await
        .expect_err("no such row");

    assert!(
        matches!(
            &error,
            Error::NotFound {
                kind: "runtime",
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_listing_can_ask_for_one_kind_and_a_home_with_none_is_an_answer() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");
    remember(&store, &installation(RuntimeKind::Node, "20.11.0"), NOW)
        .await
        .expect("a row");

    let php = records(&store, Some(RuntimeKind::Php))
        .await
        .expect("a listing");
    assert_eq!(php.len(), 1);
    assert_eq!(php[0].kind, RuntimeKind::Php);

    assert_eq!(
        records(&store, None).await.expect("a listing").len(),
        2,
        "no filter is every kind"
    );
    assert!(
        records(&store, Some(RuntimeKind::Ruby))
            .await
            .expect("a listing")
            .is_empty(),
        "a kind nobody has installed is an empty list rather than a failure"
    );
}

/// The moment survives the text column, which is what makes `installed_at` a `Timestamp` on the
/// wire rather than whatever string happened to be stored.
#[tokio::test]
async fn the_moment_a_runtime_was_installed_survives_the_column_it_is_written_to() {
    let (_home, store) = store().await;

    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");

    let row = record(&store, RuntimeKind::Php, &version("8.3.33"))
        .await
        .expect("the row");

    assert_eq!(row.installed_at, NOW);
    assert_eq!(row.bytes, 41_000_000);
    assert_eq!(row.channel, PackageChannel::Stable);
}

/// What a hand-edited database looks like from in here, asked of the function rather than
/// through a doctored row — `jobs::tests`' reasoning, and the same three columns' worth of
/// vocabulary that no `CHECK` can hold.
#[test]
fn a_row_this_build_cannot_read_names_the_column_rather_than_the_reader() {
    for (kind, version, channel, moment, column) in [
        ("perl", "8.3.33", "stable", "2026-08-14T06:55:12Z", "kind"),
        (
            "php",
            "../escape",
            "stable",
            "2026-08-14T06:55:12Z",
            "version",
        ),
        (
            "php",
            "8.3.33",
            "nightly",
            "2026-08-14T06:55:12Z",
            "channel",
        ),
        ("php", "8.3.33", "stable", "yesterday", "installed_at"),
    ] {
        let error = summary(
            kind.to_owned(),
            version.to_owned(),
            channel.to_owned(),
            "/home/runtimes/php/8.3.33".to_owned(),
            moment,
            1,
            0,
        )
        .expect_err("the row holds something this build cannot read");

        assert!(
            matches!(&error, Error::Runtime(RuntimeError::UnreadableRuntimeRow { column: named, .. }) if *named == column),
            "{error:?} should have named {column}"
        );
    }
}

/// **A phar has nothing to start** — roadmap task **T27c**, its design's D3: the installer's
/// check runs the artifact's executable, and Composer's is a file for a PHP that may not be
/// installed yet. Every language still proves it starts.
#[test]
fn only_composer_has_no_smoke_test() {
    for kind in RuntimeKind::ALL {
        assert_eq!(
            smoke_test(kind).is_none(),
            kind == RuntimeKind::Composer,
            "{kind}"
        );
    }
}

/// `go` has no `--version` flag, and `go version` prints the release without reading a module —
/// roadmap task **T27d**, its design's D3.
#[test]
fn go_is_smoke_tested_with_its_version_subcommand() {
    let smoke = smoke_test(RuntimeKind::Go).expect("go starts something");

    assert_eq!(smoke.executable, "go");
    assert_eq!(smoke.args, vec!["version".to_owned()]);
}

/// A JDK is started with `--version`, and without the variables a JVM reads before its
/// arguments — roadmap task **T27e**, its design's D3.
#[test]
fn java_is_smoke_tested_with_version_and_a_clean_environment() {
    let smoke = smoke_test(RuntimeKind::Java).expect("java starts something");

    assert_eq!(smoke.executable, "java");
    assert_eq!(smoke.args, vec!["--version".to_owned()]);
    assert!(smoke.unset.contains(&"_JAVA_OPTIONS"), "{:?}", smoke.unset);
    assert!(smoke_test(RuntimeKind::Php).expect("php").unset.is_empty());
}

/// The `CHECK` on the column and [`RuntimeKind`] have to agree, or one of them is decoration.
#[tokio::test]
async fn the_kind_column_accepts_every_kind_and_nothing_else() {
    let (_home, store) = store().await;

    for kind in RuntimeKind::ALL {
        remember(&store, &installation(kind, "1.0.0"), NOW)
            .await
            .unwrap_or_else(|error| panic!("the column refused {kind}: {error}"));
    }

    let refused = sqlx::query(
        "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                  sha256)
             VALUES ('perl', '5.38', 'stable', '/x', '2026-08-14T06:55:12Z', 1, 'x', '00')",
    )
    .execute(store.pool())
    .await;

    assert!(
        refused.is_err(),
        "the CHECK let a word through that RuntimeKind cannot read back"
    );
}

/// The lookup the shim is made of: a name of ours becomes a path under the install directory.
#[tokio::test]
async fn an_executable_is_found_by_the_name_the_index_published_it_under() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");

    let found = program(&store, RuntimeKind::Php, &version("8.3.33"), "php")
        .await
        .expect("php is published");

    assert_eq!(
        found,
        PathBuf::from("/home/runtimes/php/8.3.33")
            .join("bin")
            .join("php"),
        "the row's own directory, joined with the path inside the archive"
    );

    // A name this build does not publish is answered with the ones it does, because "php-fpm is
    // not in this artifact" and "you have not installed PHP" are different afternoons.
    let error = program(&store, RuntimeKind::Php, &version("8.3.33"), "pecl")
        .await
        .expect_err("this artifact ships no pecl");
    assert!(
        matches!(&error, Error::Runtime(RuntimeError::RuntimeProvidesNothing { known, .. })
            if known == &["php".to_owned(), "php-config".to_owned()]),
        "{error:?}"
    );
    assert!(error.to_string().contains("php-config"), "{error}");
}

/// A row from before `provides_json` existed, and a row somebody edited. Neither may become a
/// path.
#[tokio::test]
async fn a_recorded_path_that_leaves_the_install_directory_is_refused() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");

    // The same three shapes `install::archive::safe` refuses of an archive entry, which is the
    // rule this shares rather than restates — and a document that is not a map of strings at
    // all, which is the other way a hand-edited column arrives.
    for written in [
        r#"{"php": "../../../bin/sh"}"#,
        r#"{"php": "/usr/bin/php"}"#,
        r#"{"php": ""}"#,
        r#"{"php": ["bin/php"]}"#,
    ] {
        sqlx::query("UPDATE runtime_installs SET provides_json = ? WHERE version = '8.3.33'")
            .bind(written)
            .execute(store.pool())
            .await
            .expect("the doctored row");

        match program(&store, RuntimeKind::Php, &version("8.3.33"), "php").await {
            Ok(path) => panic!("{written} should not have resolved to {}", path.display()),

            Err(error) => assert!(
                matches!(
                    &error,
                    Error::Runtime(RuntimeError::UnreadableRuntimeRow {
                        column: "provides_json",
                        ..
                    })
                ),
                "{written}: {error:?}"
            ),
        }
    }
}

/// What the `DEFAULT '{}'` in the migration means when something reads such a row.
#[tokio::test]
async fn a_runtime_installed_before_its_executables_were_recorded_says_so() {
    let (_home, store) = store().await;
    remember(&store, &installation(RuntimeKind::Php, "8.3.33"), NOW)
        .await
        .expect("a row");
    sqlx::query("UPDATE runtime_installs SET provides_json = '{}'")
        .execute(store.pool())
        .await
        .expect("a row as the migration would have left it");

    let error = program(&store, RuntimeKind::Php, &version("8.3.33"), "php")
        .await
        .expect_err("nothing was recorded");

    assert!(
        matches!(&error, Error::Runtime(RuntimeError::RuntimeProvidesNothing { known, .. }) if known.is_empty()),
        "{error:?}"
    );
    assert!(error.to_string().contains("nothing recorded"), "{error}");
}

/// Removing a directory that is not there is the outcome that was wanted, which is what keeps a
/// row whose directory somebody deleted by hand removable through the API.
#[tokio::test]
async fn discarding_a_directory_that_has_already_gone_is_not_a_failure() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let runtime = home.path().join("php").join("8.3.33");
    std::fs::create_dir_all(&runtime).expect("a directory");
    std::fs::write(runtime.join("php"), b"binary").expect("a file");

    discard(&runtime).await.expect("it is there");
    assert!(!runtime.exists());

    discard(&runtime).await.expect("and now it is not");
}
