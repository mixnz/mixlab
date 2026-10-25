//! An old `mixengine.db`, migrated by this build.
//!
//! Every other migration test in this workspace builds "the old database" out of today's migration
//! files: the unit tests in `store.rs` write two migrations at run time. What none of them can be
//! is a database **this build did not write**, which is the only kind that can say whether a
//! migration that has already shipped was edited afterwards — the first rule in data-model.md's
//! compatibility list, and until this file the one nothing checked.
//!
//! The fixtures are committed, frozen, and copied before they are opened; see
//! [`mixengine_testkit::upgrade`]. **The first is `schema-0001`, the schema v0.0.7 shipped** — the
//! first release of MixLab, which folded every development migration into one. Until a release adds
//! a migration it is `Current` and every assertion here is trivially true; the first upgrade this
//! suite checks for real is the one from v0.0.7 to its successor.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mixengine_core::Store;
use mixengine_testkit::upgrade::Fixture;
use sqlx::Row as _;
use sqlx::migrate::Migrator;
use sqlx::{ConnectOptions as _, Connection as _};
use tempfile::TempDir;

/// The migrations this build carries, as this suite's own handle on them.
///
/// Read from the same directory `Store`'s embedded set is read from, but declared here rather than
/// borrowed: what several tests below need is the *list*, and `Store` deliberately exposes none.
static MIGRATIONS: Migrator = sqlx::migrate!("./migrations");

/// A fixture copied into a directory this test owns.
///
/// Never the committed file itself: [`Store::open`] migrates what it is given, so a suite handed
/// the source would rewrite the repository's fixture on its first run.
fn laid_out(fixture: &Fixture) -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("a temporary directory");
    let file = fixture.copy_into(&temp.path().join("mixengine.db"));
    (temp, file)
}

/// Where `Store::back_up` puts the copy.
///
/// Restated here because it is private there, and because what this suite asserts is the *name* a
/// person has to be able to find afterwards.
fn backup_of(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(format!(".bak-{}", env!("CARGO_PKG_VERSION")));
    PathBuf::from(name)
}

/// The versions applied to a database, read without migrating it.
async fn applied(file: &Path) -> Vec<i64> {
    let mut connection = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(file)
        .create_if_missing(false)
        .connect()
        .await
        .expect("the database");

    let versions = sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
        .fetch_all(&mut connection)
        .await
        .expect("the bookkeeping");

    connection.close().await.expect("the reader closes");
    versions
}

/// Every migration version this build carries.
fn shipped() -> Vec<i64> {
    MIGRATIONS
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .map(|migration| migration.version)
        .collect()
}

/// Every row of every table, rendered by SQLite itself.
type Census = BTreeMap<String, Vec<BTreeMap<String, String>>>;

/// Read `file` without migrating it.
///
/// `quote()` and not a typed read: it is SQLite's own faithful rendering of any value — `NULL` for
/// a null, `'x'` for text, `X'00ff'` for a blob, the numeral for a number — so one comparison
/// covers every column type without this suite knowing any of them.
///
/// `_sqlx_migrations` is excluded because it is supposed to grow.
async fn census(file: &Path) -> Census {
    let mut connection = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(file)
        .create_if_missing(false)
        .connect()
        .await
        .expect("the database");

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations'
         ORDER BY name",
    )
    .fetch_all(&mut connection)
    .await
    .expect("the tables");

    let mut census = Census::new();

    for table in tables {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY name")
                .bind(&table)
                .fetch_all(&mut connection)
                .await
                .expect("the columns");

        let projection = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(", ");

        // `AssertSqlSafe`, audited: every name here came out of this database's own `sqlite_master`
        // and `pragma_table_info`, which is our schema and not anything a user typed.
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {projection} FROM \"{table}\""
        )))
        .fetch_all(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("reading {table}: {error}"));

        let mut counted: Vec<BTreeMap<String, String>> = rows
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .enumerate()
                    .map(|(index, column)| (column.clone(), row.get::<String, _>(index)))
                    .collect()
            })
            .collect();

        // A table rebuild is free to reorder; what must not change is the set of rows.
        counted.sort();
        census.insert(table, counted);
    }

    connection.close().await.expect("the reader closes");
    census
}

/// One table's rows, restricted to `columns`.
///
/// The caller passes the columns both censuses have: a migration that *adds* one is not a loss, and
/// the intersection is also what keeps a backfill of a new column out of the comparison — it writes
/// a column that did not exist on the other side.
fn shared(
    rows: &[BTreeMap<String, String>],
    columns: &BTreeSet<String>,
) -> Vec<BTreeMap<String, String>> {
    let mut projected: Vec<BTreeMap<String, String>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .filter(|(column, _)| columns.contains(*column))
                .map(|(column, value)| (column.clone(), value.clone()))
                .collect()
        })
        .collect();
    projected.sort();
    projected
}

/// The rows of `before` that `after` does not have, counting duplicates.
///
/// **A multiset difference and not a set one.** Two identical rows that became one *is* a loss, and
/// a `contains` check would call it a match — so each row of `after` is spent at most once.
///
/// Returning the missing rows rather than a boolean is what lets the assertion name them. A table
/// with eleven rows that lost one prints the one, instead of two lists for a reader to diff by eye.
fn lost(
    before: &[BTreeMap<String, String>],
    after: &[BTreeMap<String, String>],
) -> Vec<BTreeMap<String, String>> {
    let mut unspent: BTreeMap<&BTreeMap<String, String>, usize> = BTreeMap::new();
    for row in after {
        *unspent.entry(row).or_default() += 1;
    }

    before
        .iter()
        .filter(|row| match unspent.get_mut(*row) {
            Some(remaining) if *remaining > 0 => {
                *remaining -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect()
}

/// The column names a table's census rows carry, which is empty for a table with no rows.
fn columns_of(rows: &[BTreeMap<String, String>]) -> BTreeSet<String> {
    rows.first()
        .map(|row| row.keys().cloned().collect())
        .unwrap_or_default()
}

/// What is in a directory, by file name, sorted.
fn contents(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("the home")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn an_old_database_opens_and_ends_up_at_this_builds_schema() {
    for fixture in Fixture::all() {
        let (_temp, file) = laid_out(&fixture);

        let store = Store::open(&file).await.unwrap_or_else(|error| {
            panic!(
                "{} did not migrate: {error:?}\n\
                 IncompatibleDatabase means a shipped migration was edited; Migration means our \
                 SQL is wrong; Database means the file cannot be used",
                fixture.name()
            )
        });
        store.close().await;

        assert_eq!(
            applied(&file).await,
            shipped(),
            "{} did not end up at this build's schema",
            fixture.name()
        );
    }
}

#[tokio::test]
async fn the_copy_is_taken_when_there_is_something_to_lose_and_not_otherwise() {
    let head = *shipped().last().expect("this build carries a migration");

    for fixture in Fixture::all() {
        let (_temp, file) = laid_out(&fixture);
        let behind = fixture.schema() < head;

        Store::open(&file)
            .await
            .expect("the database migrates")
            .close()
            .await;

        assert_eq!(
            backup_of(&file).exists(),
            behind,
            "{} is at schema {} and this build is at {head}",
            fixture.name(),
            fixture.schema()
        );
    }
}

#[tokio::test]
async fn opening_it_a_second_time_changes_nothing() {
    for fixture in Fixture::all() {
        let (temp, file) = laid_out(&fixture);

        Store::open(&file).await.expect("the upgrade").close().await;
        let after_the_upgrade = contents(temp.path());

        Store::open(&file)
            .await
            .expect("the second open")
            .close()
            .await;

        // The daemon starts many times a day. A backup per start would fill a person's home with
        // copies of a database nothing migrated.
        assert_eq!(
            after_the_upgrade,
            contents(temp.path()),
            "{} gained a file on a no-op open",
            fixture.name()
        );
    }
}

/// **Every row an upgrade found is still there afterwards, unchanged.**
///
/// A row that disappeared fails this, and so does one whose value moved: the comparison is over the
/// row's own rendering, so a changed cell leaves the old row with nothing to match.
///
/// **A row a migration *adds* is not a failure**, which is the same rule [`shared`] already applies
/// one axis over: *a migration that adds a column is not a loss*. It was an equality until the
/// credential repair found out why that is wrong — a migration that seeded `settings` with the id a
/// home is known by, deliberately and for its own reasons, and an equality reported that correct
/// migration as data loss on every fixture older than it. Seeding is a normal thing for a migration
/// to do; losing a row is not, and only the second is what this file is for.
///
/// **So nothing here polices additions**, and that gap is deliberate rather than overlooked. What
/// would notice a migration writing a row it should not is a reader of the migration; what this
/// covers is the direction that cannot be read back — a loss, which leaves nothing behind to
/// inspect.
#[tokio::test]
async fn an_upgrade_keeps_every_row_it_found() {
    for fixture in Fixture::all() {
        let (_temp, file) = laid_out(&fixture);
        let before = census(&file).await;

        Store::open(&file).await.expect("the upgrade").close().await;

        let after = census(&file).await;
        let mut compared = 0;

        for (table, rows) in &before {
            if rows.is_empty() {
                continue;
            }
            compared += 1;

            let migrated = after
                .get(table)
                .unwrap_or_else(|| panic!("{}: the migration dropped {table}", fixture.name()));

            let columns: BTreeSet<String> = columns_of(rows)
                .intersection(&columns_of(migrated))
                .cloned()
                .collect();

            let missing = lost(&shared(rows, &columns), &shared(migrated, &columns));

            assert!(
                missing.is_empty(),
                "{}: the migration lost {} row(s) from {table}: {missing:?}",
                fixture.name(),
                missing.len()
            );
        }

        // Ten of the nineteen tables `0001_initial.sql` creates, at the least. A census over a
        // fixture that seeded nothing would compare nothing and pass, which is the shape of failure
        // this whole file exists to stop.
        assert!(
            compared >= 10,
            "{} carried rows in only {compared} tables, which is not a fixture worth having",
            fixture.name()
        );
    }
}

#[tokio::test]
async fn the_copy_taken_first_is_the_database_as_it_was() {
    for fixture in Fixture::all() {
        let (_temp, file) = laid_out(&fixture);
        let before = census(&file).await;

        Store::open(&file).await.expect("the upgrade").close().await;

        let backup = backup_of(&file);
        if !backup.exists() {
            // Nothing to migrate, so nothing to copy — asserted by
            // `the_copy_is_taken_when_there_is_something_to_lose_and_not_otherwise`.
            continue;
        }

        assert_eq!(
            census(&backup).await,
            before,
            "{}: the copy is of the state *after* the upgrade, which is not a backup",
            fixture.name()
        );
    }
}

/// The instrument, checked against itself.
///
/// Every assertion above compares two censuses, and a census that rendered nothing — or rendered
/// every row identically — would make all of them pass while reading nothing. So this takes two
/// censuses of one unchanged file and requires them equal, then changes exactly one value and
/// deletes exactly one row and requires each to be noticed.
///
/// It is the instrument that is checked here. What is done *with* the readings is
/// [`lost`], and [`a_lost_row_is_found_and_an_added_one_is_not`] is that half.
///
/// It is `fakeservice`'s rule applied to a measurement rather than to a program: a fixture that
/// quietly stopped honouring what it was told turns the test using it into one that passes for the
/// wrong reason.
#[tokio::test]
async fn the_census_notices_a_changed_value_and_a_missing_row() {
    let fixture = Fixture::all()
        .into_iter()
        .next()
        .expect("a fixture — see the testkit's own suite");
    let (_temp, file) = laid_out(&fixture);

    let before = census(&file).await;
    assert_eq!(
        before,
        census(&file).await,
        "two readings of one unchanged file disagree, so nothing above compares anything"
    );

    let mut connection = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&file)
        .create_if_missing(false)
        .connect()
        .await
        .expect("the fixture");

    sqlx::query("UPDATE settings SET value_json = 'true' WHERE key = 'telemetry'")
        .execute(&mut connection)
        .await
        .expect("one value, changed");

    let changed = census(&file).await;
    assert_ne!(
        before.get("settings"),
        changed.get("settings"),
        "a changed value read the same, so `an_upgrade_keeps_every_row_it_found` cannot fail"
    );

    sqlx::query("DELETE FROM events WHERE id = 1")
        .execute(&mut connection)
        .await
        .expect("one row, gone");
    connection.close().await.expect("the writer closes");

    let deleted = census(&file).await;
    assert_ne!(
        before.get("events"),
        deleted.get("events"),
        "a deleted row read the same, so a migration that lost one would pass"
    );
}

#[tokio::test]
async fn no_shipped_migration_has_been_edited_since_a_fixture_recorded_it() {
    for fixture in Fixture::all() {
        let (_temp, file) = laid_out(&fixture);

        let mut connection = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&file)
            .create_if_missing(false)
            .connect()
            .await
            .expect("the fixture");

        let recorded: Vec<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&mut connection)
                .await
                .expect("the bookkeeping");

        connection.close().await.expect("the reader closes");

        assert!(
            !recorded.is_empty(),
            "{} records no migration at all",
            fixture.name()
        );

        for (version, checksum) in recorded {
            let shipped = MIGRATIONS
                .iter()
                .find(|migration| migration.version == version)
                .unwrap_or_else(|| {
                    panic!(
                        "{} was captured at a schema including migration {version}, which this \
                         build no longer carries",
                        fixture.name()
                    )
                });

            // `Store::open` would catch this as `IncompatibleDatabase` — a sentence about a
            // database from another build, which is the wrong paragraph to send the reader to.
            assert_eq!(
                checksum,
                shipped.checksum.as_ref(),
                "migration {version} has been edited since {} recorded it. data-model.md: never \
                 rewrite an existing migration file.",
                fixture.name()
            );
        }
    }
}

#[tokio::test]
async fn an_upgraded_database_takes_the_writes_a_current_build_makes() {
    for fixture in Fixture::all() {
        let (_temp, file) = laid_out(&fixture);
        let store = Store::open(&file).await.expect("the upgrade");

        // A file that opens and then refuses every write is not a migrated database. The three
        // statements a site is, against the schema this build ships.
        let project: i64 = sqlx::query_scalar(
            "INSERT INTO projects (name, root_path, created_at)
             VALUES ('after-the-upgrade', '/home/dev/after-the-upgrade', '2026-09-05T09:00:00Z')
             RETURNING id",
        )
        .fetch_one(store.pool())
        .await
        .unwrap_or_else(|error| panic!("{}: a project: {error}", fixture.name()));

        let site: i64 = sqlx::query_scalar(
            "INSERT INTO sites (project_id, doc_root, kind) VALUES (?, 'public', 'static')
             RETURNING id",
        )
        .bind(project)
        .fetch_one(store.pool())
        .await
        .unwrap_or_else(|error| panic!("{}: a site: {error}", fixture.name()));

        sqlx::query("INSERT INTO site_domains (site_id, domain, is_primary) VALUES (?, ?, 1)")
            .bind(site)
            .bind("after-the-upgrade.test")
            .execute(store.pool())
            .await
            .unwrap_or_else(|error| panic!("{}: a domain: {error}", fixture.name()));

        store.close().await;
    }
}

/// **[`lost`] finds a loss and permits an addition**, which is the whole of what the assertion it
/// serves claims.
///
/// The third case is the one a set difference gets wrong and a multiset one does not: two identical
/// rows that became one is a row lost, and `contains` would call it a match.
#[test]
fn a_lost_row_is_found_and_an_added_one_is_not() {
    fn row(key: &str, value: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("key".to_owned(), key.to_owned()),
            ("value_json".to_owned(), value.to_owned()),
        ])
    }

    let before = vec![row("telemetry", "false"), row("update.channel", "stable")];

    assert!(
        lost(&before, &before).is_empty(),
        "an unchanged table lost something"
    );

    // The failure this file exists for.
    assert_eq!(
        lost(&before, &before[1..]),
        vec![row("telemetry", "false")],
        "a deleted row was not reported"
    );

    // A changed value is a loss too: the row as it was has nothing to match.
    assert_eq!(
        lost(
            &before,
            &[row("telemetry", "true"), row("update.channel", "stable")]
        ),
        vec![row("telemetry", "false")],
        "a changed value was not reported"
    );

    // Duplicates are spent one at a time.
    let twice = vec![row("telemetry", "false"), row("telemetry", "false")];
    assert_eq!(
        lost(&twice, &twice[..1]),
        vec![row("telemetry", "false")],
        "two rows that became one read as a match"
    );

    // **And the case the credential repair changed.** A migration seeded this row; an equality
    // called that data loss, and this is the assertion that says it is not.
    let after = vec![
        row("home.id", "b743b62cbba8"),
        row("telemetry", "false"),
        row("update.channel", "stable"),
    ];
    assert!(
        lost(&before, &after).is_empty(),
        "a migration that seeded a row was reported as one that lost one"
    );
}
