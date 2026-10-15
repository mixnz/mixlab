use super::{run, Invocation, Watch};
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
