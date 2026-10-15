use super::{split_uri, tool_uri};

/// Where a MongoDB URI comes apart, and the two characters that decide it wrongly if the
/// splitting is done from the left.
///
/// A password may hold an `@` — it is percent-encoded in a well-formed URI, but the box this
/// comes from takes what the user pastes — so the host list starts after the *last* `@`, not
/// the first. And the `?` that starts the options is found before the `/` that starts the
/// path, since an option value may contain a slash (`tlsCAFile=/etc/ca.pem`).
#[test]
fn a_mongo_uri_comes_apart_at_the_right_characters() {
    let (head, hosts, path, query) = split_uri("mongodb://db.example:27017").unwrap();
    assert_eq!(
        (head.as_str(), hosts.as_str()),
        ("mongodb://", "db.example:27017")
    );
    assert_eq!((path.as_str(), query.as_str()), ("", ""));

    let (head, hosts, path, query) =
        split_uri("mongodb://root:pw@a:27017,b:27017/shop?replicaSet=rs0").unwrap();
    assert_eq!(head, "mongodb://root:pw@");
    assert_eq!(hosts, "a:27017,b:27017");
    assert_eq!(path, "/shop");
    assert_eq!(query, "?replicaSet=rs0");

    // An `@` in the password: split from the left, `pw` would be the whole credential and
    // `me.com:pw2@host` the host list.
    let (head, hosts, ..) = split_uri("mongodb://root:p@ssw0rd@db.example/shop").unwrap();
    assert_eq!(head, "mongodb://root:p@ssw0rd@");
    assert_eq!(hosts, "db.example");

    // A `/` inside an option value belongs to the options, not to a path.
    let (_, hosts, path, query) =
        split_uri("mongodb://db.example/?tlsCAFile=/etc/ssl/ca.pem").unwrap();
    assert_eq!(hosts, "db.example");
    assert_eq!(path, "/");
    assert_eq!(query, "?tlsCAFile=/etc/ssl/ca.pem");

    // The other scheme, which is the whole reason the scheme is kept rather than rebuilt.
    let (head, ..) = split_uri("mongodb+srv://root:pw@cluster.example/shop").unwrap();
    assert_eq!(head, "mongodb+srv://root:pw@");

    assert!(split_uri("db.example:27017").is_err());
}

/// The database is dropped from the path but its `/` is not: without it the tools refuse the
/// URI outright ("must have a / before the query").
#[test]
fn drops_the_database_but_keeps_the_slash() {
    assert_eq!(
        tool_uri(
            "mongodb://user:pw@db.example:27017/shop?authSource=admin",
            None
        )
        .unwrap(),
        "mongodb://user:pw@db.example:27017/?authSource=admin"
    );
    assert_eq!(
        tool_uri("mongodb://db.example:27017", None).unwrap(),
        "mongodb://db.example:27017/"
    );
    assert_eq!(
        tool_uri("mongodb://db.example:27017/shop", None).unwrap(),
        "mongodb://db.example:27017/"
    );
}

/// A tunnel replaces the host list, and the one forwarded node has to be talked to directly —
/// discovering the replica set would hand back addresses the tunnel does not reach.
#[test]
fn points_at_the_tunnel() {
    assert_eq!(
        tool_uri(
            "mongodb://user:pw@db.example:27017/shop",
            Some(("127.0.0.1", 5001))
        )
        .unwrap(),
        "mongodb://user:pw@127.0.0.1:5001/?directConnection=true"
    );
    assert_eq!(
        tool_uri(
            "mongodb://a:27017,b:27017/?replicaSet=rs0",
            Some(("127.0.0.1", 5001))
        )
        .unwrap(),
        "mongodb://127.0.0.1:5001/?replicaSet=rs0&directConnection=true"
    );
}

/// The archive layout this reads: the magic number, the header, then a metadata document per
/// collection — the first of which names the database everything in the file came from.
#[test]
fn reads_the_database_out_of_an_archive() {
    use mongodb::bson::doc;

    let mut archive = super::ARCHIVE_MAGIC.to_le_bytes().to_vec();
    doc! { "concurrent_collections": 4, "version": "0.1" }
        .to_writer(&mut archive)
        .unwrap();
    doc! { "db": "pnedu_portal", "collection": "users", "size": 10_i64 }
        .to_writer(&mut archive)
        .unwrap();
    let path = std::env::temp_dir().join(format!("mixlab-test-{}.archive", uuid::Uuid::new_v4()));
    std::fs::write(&path, &archive).unwrap();

    let found = super::archive_database(&path.to_string_lossy());
    let not_an_archive = super::archive_database(file!());
    std::fs::remove_file(&path).unwrap();

    assert_eq!(found.unwrap(), "pnedu_portal");
    assert!(not_an_archive.is_err());
}

#[test]
fn refuses_an_srv_uri_over_a_tunnel() {
    assert!(tool_uri(
        "mongodb+srv://user:pw@cluster.example/shop",
        Some(("127.0.0.1", 5001))
    )
    .is_err());
}

/// A mongodump archive is the documents' own BSON with little else in it, so what is on disk
/// against what `$collStats` says they weigh is a fair reading of how far along the dump is.
#[test]
fn measures_an_archive_against_what_its_documents_weigh() {
    const MB: u64 = 1 << 20;
    assert_eq!(super::archive_share(0, 100 * MB), Some(0));
    assert_eq!(super::archive_share(50 * MB, 100 * MB), Some(50));
    // Overrunning what it was expected to come to does not take the bar past the end, and 100
    // is never claimed: the command returning is what says the dump is finished.
    assert_eq!(super::archive_share(500 * MB, 100 * MB), Some(99));
    // A server that would not say what its collections hold: no number to give, but the dump
    // still runs and the bar still moves.
    assert_eq!(super::archive_share(50 * MB, 0), None);
}
