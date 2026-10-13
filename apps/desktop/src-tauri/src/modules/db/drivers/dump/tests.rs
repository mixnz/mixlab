use super::split_uri;
use super::{pg_reached_table, pg_rewrite_preamble, run, tool_uri, Invocation, Watch};

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

/// Where the database name goes on a MySQL command line, and what stands in front of it.
///
/// `my_getopt` reads any argument beginning with `-` as an option wherever it appears, so a
/// database named `-x` used to be swallowed as one — and mysqldump's short options are mostly
/// flags, which means it is not even rejected: the dump runs, on the wrong terms, over no
/// database at all. A bare `--` ends the options in both MySQL's parser and MariaDB's.
#[test]
fn a_mysql_database_name_is_passed_as_a_name_and_not_as_an_option() {
    for args in [
        mysqldump_args(
            Path::new("opts.cnf"),
            "utf8mb4",
            false,
            true,
            DumpMode::All,
            "-x",
        ),
        mysql_restore_args(Path::new("opts.cnf"), "-x"),
    ] {
        assert_eq!(args.last().map(String::as_str), Some("-x"), "{args:?}");
        assert_eq!(args[args.len() - 2], "--", "{args:?}");
        // And exactly one: a second would be handed to the tool as a table name.
        assert_eq!(
            args.iter().filter(|arg| *arg == "--").count(),
            1,
            "{args:?}"
        );
    }
}

/// The credentials file is the only option file the tools read, first on the line. As an
/// extra file it lost to a `password=` left in the user's `~/.my.cnf`, and the dump connected
/// with no password — see [`OptionFile`].
#[test]
fn the_mysql_tools_read_the_credentials_file_alone() {
    for args in [
        mysqldump_args(
            Path::new("opts.cnf"),
            "utf8mb4",
            false,
            true,
            DumpMode::All,
            "db",
        ),
        mysql_restore_args(Path::new("opts.cnf"), "db"),
    ] {
        assert_eq!(args[0], "--defaults-file=opts.cnf", "{args:?}");
        assert!(
            !args
                .iter()
                .any(|arg| arg.starts_with("--defaults-extra-file")),
            "{args:?}"
        );
    }
}

/// The switches that depend on which tool and which server this is. MariaDB's mysqldump
/// refuses the whole run over either of the two MySQL-only ones — see `is_mariadb_tool`.
#[test]
fn mysqldump_only_asks_mysqls_own_tool_for_mysql_only_options() {
    let mysql = mysqldump_args(Path::new("o"), "utf8mb4", false, false, DumpMode::All, "db");
    assert!(mysql.iter().any(|arg| arg == "--set-gtid-purged=OFF"));
    assert!(mysql.iter().any(|arg| arg == "--column-statistics=0"));

    // A server that does have the histogram table is not told to skip it.
    let has_stats = mysqldump_args(Path::new("o"), "utf8mb4", false, true, DumpMode::All, "db");
    assert!(!has_stats.iter().any(|arg| arg == "--column-statistics=0"));

    let mariadb = mysqldump_args(Path::new("o"), "utf8mb4", true, false, DumpMode::All, "db");
    assert!(!mariadb
        .iter()
        .any(|arg| arg.starts_with("--set-gtid-purged")));
    assert!(!mariadb
        .iter()
        .any(|arg| arg.starts_with("--column-statistics")));
}

/// The database reaches the PostgreSQL tools through the environment, and nowhere else.
///
/// libpq expands a `dbname` that holds an `=` — or that starts `postgresql://` — into a whole
/// connection string, so a name like `dbname=other` on the command line does not open a
/// database of that name, it opens whichever server and database the name spells out. For a
/// restore that is a file written into a database nobody chose. `PGDATABASE` is read as a
/// plain name.
#[test]
fn the_postgres_database_never_reaches_the_command_line() {
    let hostile = "dbname=elsewhere host=attacker.example";
    for args in [
        pg_dump_args("localhost", 5432, "u", DumpMode::All),
        psql_args("localhost", 5432, "u"),
    ] {
        assert!(
            !args.iter().any(|arg| arg.contains("--dbname")),
            "the database is back on the command line: {args:?}",
        );
        assert!(!args.iter().any(|arg| arg.contains(hostile)), "{args:?}");
    }

    let pgpass = PgPassFile::new("localhost", 5432, "u", "p").unwrap();
    let env = pgpass.env(hostile);
    assert_eq!(
        env.iter()
            .find(|(key, _)| *key == "PGDATABASE")
            .map(|(_, value)| value.as_str()),
        Some(hostile),
    );
}
use super::{mysql_restore_args, mysqldump_args, pg_dump_args, psql_args, DumpMode, PgPassFile};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Not a test: the child the two tests below need. Ignored so it never runs on its own, and
/// spawned by name from `current_exe` so those tests depend on no program the machine might
/// not have — `sleep` is not on Windows, and `ping` is not guaranteed in a container.
#[test]
#[ignore]
fn sleeper() {
    std::thread::sleep(std::time::Duration::from_secs(60));
}

/// That child, as a tool `run` can be pointed at. It outlives any wait a test is willing to
/// make, so the only way it finishes is by being killed.
fn slow_tool() -> (PathBuf, Vec<String>) {
    let args = [
        "--exact",
        "modules::db::drivers::dump::tests::sleeper",
        "--ignored",
    ];
    (
        std::env::current_exe().expect("the test binary knows where it is"),
        args.iter().map(|a| a.to_string()).collect(),
    )
}

/// The whole of what R29 was about: a transfer that is called off stops the tool, rather than
/// leaving it to run to completion for a connection that has gone.
#[test]
fn a_cancelled_run_kills_the_tool_instead_of_waiting_for_it() {
    let (tool, args) = slow_tool();
    // Set before the run starts, so the first poll — a quarter of a second in — sees it.
    let cancel: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&cancel);
    let watch = Watch {
        report: &|_| {},
        cancel: &|| flag.load(Ordering::Relaxed),
    };

    let started = Instant::now();
    let result = run(
        Invocation {
            tool: &tool,
            args: &args,
            env: &[],
            stdin: None,
            stdout: None,
            name: "sleeper",
        },
        &watch,
        |_| {},
    );

    assert!(
        result.is_err(),
        "a killed tool is not a transfer that worked"
    );
    assert!(
        started.elapsed().as_secs() < 10,
        "it waited the full minute out instead of killing the tool: {:?}",
        started.elapsed()
    );
}

/// And the other half: a run nobody stops is not cut short by the polling, and the tool's own
/// success is still what the answer is read from.
#[test]
fn a_run_nobody_stops_is_left_alone() {
    // `--list` exits at once and exits cleanly, which is all this needs of it.
    let tool = std::env::current_exe().expect("the test binary knows where it is");
    let watch = Watch {
        report: &|_| {},
        cancel: &|| false,
    };
    let args = vec!["--list".to_string()];
    assert!(run(
        Invocation {
            tool: &tool,
            args: &args,
            env: &[],
            stdin: None,
            stdout: None,
            name: "list"
        },
        &watch,
        |_| {},
    )
    .is_ok());
}

/// The preamble pg_dump 17 and later writes, whatever the version of the server it read.
const PREAMBLE: &str = "--\n-- PostgreSQL database dump\n--\n\n\\restrict S0j3INb3aLmWPSh\n\n\
                            -- Dumped from database version 15.18\n\
                            -- Dumped by pg_dump version 18.6\n\n\
                            SET statement_timeout = 0;\n\
                            SET lock_timeout = 0;\n\
                            SET transaction_timeout = 0;\n\
                            SET client_encoding = 'UTF8';\n";

/// The one line a server older than 17 refuses is swapped for a block it can refuse quietly,
/// and everything above it is handed over untouched.
#[test]
fn guards_the_transaction_timeout_a_dump_sets() {
    let lead = pg_rewrite_preamble(PREAMBLE.as_bytes()).expect("a lead");
    let bytes = String::from_utf8(lead.bytes).unwrap();
    let cut = PREAMBLE.find("SET transaction_timeout").unwrap();
    assert_eq!(&bytes[..cut], &PREAMBLE[..cut]);
    assert!(bytes[cut..].starts_with("DO $$ BEGIN PERFORM"), "{bytes}");
    // What is replaced is the original line, so that the rest of the file follows on from it.
    assert_eq!(
        lead.replaced as usize,
        cut + "SET transaction_timeout = 0;\n".len()
    );
}

/// A dump from pg_dump 16 or older never says it, and there is nothing to rewrite.
#[test]
fn leaves_a_dump_without_the_line_alone() {
    let older = PREAMBLE.replace("SET transaction_timeout = 0;\n", "");
    assert!(pg_rewrite_preamble(older.as_bytes()).is_none());
}

/// The walk stops at the first line that is not part of a preamble, so a row of data reading
/// like the line is left as it is — a restore may not edit what it is restoring.
#[test]
fn stops_before_the_data() {
    let dump = "--\n-- PostgreSQL database dump\n--\n\n\
                    COPY public.notes (body) FROM stdin;\n\
                    SET transaction_timeout = 0;\n\\.\n";
    assert!(pg_rewrite_preamble(dump.as_bytes()).is_none());
}

/// A preamble cut off mid-line is one this side cannot read to the end of, and a half-line is
/// not enough to say what it is.
#[test]
fn ignores_a_line_the_read_cut_in_half() {
    let cut = &PREAMBLE[..PREAMBLE.find("SET transaction_timeout").unwrap() + 12];
    assert!(pg_rewrite_preamble(cut.as_bytes()).is_none());
}

/// Every wording of the line across the pg_dump versions this app can meet, each read back as
/// the name the rest of the app uses — which is what the weights are keyed by.
#[test]
fn reads_the_table_pg_dump_has_reached_however_it_is_worded() {
    // 12 and later, with the program's name in front of it.
    assert_eq!(
        pg_reached_table("pg_dump: dumping contents of table \"public.users\"").as_deref(),
        Some("users")
    );
    // Older: the two halves quoted separately.
    assert_eq!(
        pg_reached_table("pg_dump: dumping contents of table \"sales\".\"orders\"").as_deref(),
        Some("sales.orders")
    );
    // Older still: no quotes and no schema.
    assert_eq!(
        pg_reached_table("dumping contents of table users").as_deref(),
        Some("users")
    );
    // A table whose own name holds a dot: the split is at the first one, and the name comes
    // back quoted exactly as the sidebar writes it.
    assert_eq!(
        pg_reached_table("pg_dump: dumping contents of table \"public.odd.name\"").as_deref(),
        Some("\"odd.name\"")
    );
}

#[test]
fn everything_else_pg_dump_says_is_not_a_table() {
    assert_eq!(
        pg_reached_table("pg_dump: last built-in OID is 16383"),
        None
    );
    assert_eq!(pg_reached_table("pg_dump: reading extensions"), None);
    assert_eq!(pg_reached_table(""), None);
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

/// Real lines from `mysqldump --verbose`, of which exactly one kind says a table has been
/// reached. Everything else it says is chatter the count must not move for.
#[test]
fn reads_the_table_out_of_the_commentary() {
    assert_eq!(
        super::reached_table("-- Retrieving table structure for table orders..."),
        Some("orders")
    );
    assert_eq!(super::reached_table("-- Retrieving rows..."), None);
    assert_eq!(
        super::reached_table("-- Rolling back to savepoint sp..."),
        None
    );
    assert_eq!(
        super::reached_table("-- Connecting to 192.168.1.1..."),
        None
    );
    assert_eq!(
        super::reached_table("mysqldump: Got error: 1049: Unknown database 'shop'"),
        None
    );
}

/// A dump's failure has to be readable through the commentary this module asked for: the lines
/// mysqldump writes about its own progress are held back, and the one that says what went wrong
/// is what the error carries.
#[test]
fn keeps_the_error_out_of_the_commentary() {
    let mut tail = super::Tail::default();
    for line in [
        "-- Connecting to db.example...",
        "-- Retrieving table structure for table orders...",
        "",
        "mysqldump: Got error: 1044: Access denied for user 'app'@'%'",
    ] {
        tail.push(line.to_string());
    }
    assert_eq!(
        tail.message(),
        "mysqldump: Got error: 1044: Access denied for user 'app'@'%'"
    );

    // A failure with nothing but commentary to show still has to say something.
    let mut nothing_else = super::Tail::default();
    nothing_else.push("-- Connecting to db.example...".to_string());
    assert_eq!(nothing_else.message(), "-- Connecting to db.example...");
}

/// A tool's standard error is not promised to be UTF-8: these tools write what the server told
/// them in whatever character set the connection is using, and a dump's is the database's own.
/// The line that says what went wrong comes last, so it only survives if a line that will not
/// decode is stepped over rather than treated as the end of the output.
#[test]
fn reads_past_a_line_that_will_not_decode() {
    let mut stderr = b"-- Connecting to db.example...\n".to_vec();
    // A table name with an accent in it, from a latin1 database.
    stderr.extend_from_slice(b"-- Retrieving table structure for table h\xf3a_don...\n");
    stderr.extend_from_slice(b"mysqldump: Got error: 1044: Access denied\r\n");

    let mut source = &stderr[..];
    let mut buffer = Vec::new();
    let mut lines = Vec::new();
    while let Some(line) = super::next_line(&mut source, &mut buffer) {
        lines.push(line);
    }

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "-- Connecting to db.example...");
    // The byte that would not decode is stood in for, and the rest of its line is still there.
    assert!(lines[1].ends_with("_don..."));
    // The one this is all for: it comes after the line that would not decode.
    assert_eq!(lines[2], "mysqldump: Got error: 1044: Access denied");
}

/// One temporary file, grown to order, standing in for the dump being written.
struct Written(std::path::PathBuf);

impl Written {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("mixlab-test-{}.sql", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"").unwrap();
        Self(path)
    }

    fn grow_to(&self, bytes: usize) {
        std::fs::write(&self.0, vec![b'x'; bytes]).unwrap();
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Written {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The reckoning behind the bar: a table's share is what its rows weigh on the server, and
/// where the dump has got to inside the table it is on is read from the file it is writing.
#[test]
fn weighs_each_table_by_what_its_rows_take() {
    const MB: u64 = 1 << 20;
    let file = Written::new();
    let tables = vec![
        ("small".to_string(), 4 * MB),
        ("large".to_string(), 12 * MB),
    ];
    let mut tracker = super::Tracker::new(&tables, &file.path(), true);

    // Nothing written yet, and the first table only just reached.
    tracker.reached("small");
    let start = tracker.progress();
    assert_eq!(start.percent, Some(0));
    assert_eq!(start.part.as_deref(), Some("small"));
    assert_eq!((start.at_part, start.parts), (1, 2));

    // Halfway through the first table's bytes, at the ratio assumed before any are known: half
    // of a table that is a quarter of the database.
    file.grow_to((2 * MB) as usize * 4 / 5);
    assert_eq!(tracker.progress().percent, Some(12));

    // The whole of it, and the next table begun — the first table's share, and no more, however
    // much of the file it turned out to take.
    file.grow_to((6 * MB) as usize);
    tracker.reached("large");
    let second = tracker.progress();
    assert_eq!(second.percent, Some(25));
    assert_eq!((second.at_part, second.parts), (2, 2));

    // A third of the way through the larger table — measured at the ratio the first one turned
    // out to have, six bytes of file to four of table, rather than at the assumed one.
    file.grow_to((12 * MB) as usize);
    assert_eq!(tracker.progress().percent, Some(50));

    // A table that overruns what it was thought to weigh does not take the bar past its share,
    // and 100 is never claimed: the command returning is what says the dump is finished.
    file.grow_to((400 * MB) as usize);
    assert_eq!(tracker.progress().percent, Some(99));
}

/// A view weighs nothing and is in no list of tables — mysqldump writes one out all the same,
/// and the count has to make room for it rather than report five tables of four.
#[test]
fn makes_room_for_what_it_was_not_told_about() {
    let file = Written::new();
    let tables = vec![("orders".to_string(), 1 << 20)];
    let mut tracker = super::Tracker::new(&tables, &file.path(), true);

    tracker.reached("orders");
    tracker.reached("recent_orders");
    let progress = tracker.progress();
    assert_eq!(progress.part.as_deref(), Some("recent_orders"));
    assert_eq!((progress.at_part, progress.parts), (2, 2));
}

/// A structure-only dump writes much the same for a table whatever it holds, so its bar counts
/// tables — weighing them by rows it is not writing would stall on the big one and then leap.
///
/// The table it is on counts as written: a few hundred bytes go out for one and the next
/// reading is a quarter of a second later, so waiting for the next table to say this one is
/// done would leave a two-table dump ending at half.
#[test]
fn counts_the_tables_when_the_rows_are_not_being_written() {
    let file = Written::new();
    let tables = vec![
        ("small".to_string(), 1 << 10),
        ("large".to_string(), 1 << 30),
    ];
    let mut tracker = super::Tracker::new(&tables, &file.path(), false);

    tracker.reached("small");
    assert_eq!(tracker.progress().percent, Some(50));
    tracker.reached("large");
    assert_eq!(tracker.progress().percent, Some(99));
}

/// A restore's figure is a count rather than an estimate, and the only thing it has to be
/// careful about is the end: the client goes on executing after the last byte has gone in.
#[test]
fn counts_a_restore_out_of_the_file_it_is_fed() {
    assert_eq!(super::share(0, 16), Some(0));
    assert_eq!(super::share(8, 16), Some(50));
    assert_eq!(super::share(16, 16), Some(99));
    // A file whose size could not be read: no number to give, but the restore still runs.
    assert_eq!(super::share(0, 0), None);
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

/// Nothing known about the database's tables — a user `information_schema` shows nothing to —
/// leaves no honest percentage to give, but the dump still runs and still says where it is.
#[test]
fn gives_no_percentage_it_cannot_stand_behind() {
    let file = Written::new();
    let mut tracker = super::Tracker::new(&[], &file.path(), true);

    tracker.reached("orders");
    let progress = tracker.progress();
    assert_eq!(progress.percent, None);
    assert_eq!(progress.part.as_deref(), Some("orders"));
    assert_eq!(progress.parts, 1);
}
