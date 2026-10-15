use super::{mysql_restore_args, mysqldump_args, DumpMode};
use std::path::Path;

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
