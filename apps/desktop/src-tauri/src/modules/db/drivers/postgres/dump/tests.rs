use super::{pg_dump_args, pg_reached_table, pg_rewrite_preamble, psql_args, DumpMode, PgPassFile};

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
