use super::verify_host;
use super::{is_transient_accept, known_hosts_file, load_known_hosts, next_backoff, remember_host};
use super::{ACCEPT_ALARM, ACCEPT_RETRY, WATCH_IDLE, WATCH_MAX, WATCH_MIN};
use std::io::{Error, ErrorKind};
use std::time::Duration;

/// What the handler reads and writes between connections. The handshake around it needs a real
/// SSH server to exercise; this is the part that decides whether a key is the one seen before.
#[test]
fn a_remembered_host_is_read_back_and_can_be_replaced() {
    let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    let file = known_hosts_file(&dir);

    // Nothing remembered yet — a first connection has nothing to check against.
    assert!(load_known_hosts(&file).is_empty());

    remember_host(&file, "db.example:22", "SHA256:aaa").unwrap();
    remember_host(&file, "other.example:2222", "SHA256:bbb").unwrap();
    let known = load_known_hosts(&file);
    assert_eq!(
        known.get("db.example:22").map(String::as_str),
        Some("SHA256:aaa")
    );
    assert_eq!(known.len(), 2);

    // Each host stands on its own: accepting a rebuilt server's new key leaves the others be.
    remember_host(&file, "db.example:22", "SHA256:ccc").unwrap();
    let known = load_known_hosts(&file);
    assert_eq!(
        known.get("db.example:22").map(String::as_str),
        Some("SHA256:ccc")
    );
    assert_eq!(
        known.get("other.example:2222").map(String::as_str),
        Some("SHA256:bbb")
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Two first connections at once. Without a lock around the read-modify-write both threads
/// read the same file, both add their own host, and whichever writes last is the only one
/// remembered — so the other server is a stranger again next time, and TOFU accepts whatever
/// answers for it.
#[test]
fn hosts_remembered_at_the_same_time_all_survive() {
    let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    let file = known_hosts_file(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    std::thread::scope(|scope| {
        for n in 0..8 {
            let file = &file;
            scope.spawn(move || {
                for round in 0..8 {
                    let endpoint = format!("host{n}.example:22");
                    remember_host(file, &endpoint, &format!("SHA256:{n}-{round}")).unwrap();
                }
            });
        }
    });

    let known = load_known_hosts(&file);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(known.len(), 8, "an entry was lost to a concurrent write");
}

/// The three answers TOFU has, from the one place that decides them.
#[test]
fn a_changed_key_is_refused_and_the_known_one_is_not() {
    let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    let file = known_hosts_file(&dir);

    // Never seen: accepted, and written down.
    verify_host(&file, "db.example:22", "SHA256:aaa").unwrap();
    assert_eq!(
        load_known_hosts(&file)
            .get("db.example:22")
            .map(String::as_str),
        Some("SHA256:aaa"),
    );

    // Seen, same key: accepted again.
    verify_host(&file, "db.example:22", "SHA256:aaa").unwrap();

    // Seen, different key: refused, and the fingerprint on file is left alone — accepting the
    // new one is the user's decision, taken by deleting the entry, not ours.
    assert!(verify_host(&file, "db.example:22", "SHA256:zzz").is_err());
    assert_eq!(
        load_known_hosts(&file)
            .get("db.example:22")
            .map(String::as_str),
        Some("SHA256:aaa"),
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file that has been corrupted (or written by a future version) reads as "nothing known"
/// rather than failing every SSH connection the app makes.
#[test]
fn an_unreadable_store_is_treated_as_empty() {
    let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = known_hosts_file(&dir);
    std::fs::write(&file, "not json").unwrap();

    let known = load_known_hosts(&file);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(known.is_empty());
}

/// The watcher's interval. Success goes back to the idle interval; the first failure drops to
/// the fastest interval, then doubles gradually up to the cap — and stays at the cap instead of
/// returning to the fast interval.
#[test]
fn the_watcher_backs_off_while_the_tunnel_stays_down() {
    // While things are fine every interval is WATCH_IDLE, whatever interval it just failed at.
    assert_eq!(next_backoff(WATCH_IDLE, true), WATCH_IDLE);
    assert_eq!(next_backoff(WATCH_MIN, true), WATCH_IDLE);
    assert_eq!(next_backoff(WATCH_MAX, true), WATCH_IDLE);

    // The first failure — the current interval is the idle one — retries quickly.
    assert_eq!(next_backoff(WATCH_IDLE, false), WATCH_MIN);

    // Then doubles.
    assert_eq!(next_backoff(WATCH_MIN, false), Duration::from_secs(10));
    assert_eq!(
        next_backoff(Duration::from_secs(10), false),
        Duration::from_secs(20)
    );

    // Hitting the cap stops at the cap, neither exceeding it nor going back to WATCH_MIN.
    assert_eq!(next_backoff(Duration::from_secs(40), false), WATCH_MAX);
    assert_eq!(next_backoff(WATCH_MAX, false), WATCH_MAX);
}

/// What the accept loop above decides: which errors belong to a single connection, and which to
/// the port.
///
/// Checked with the operating system's own error codes rather than hand-written `ErrorKind`s,
/// because what is being asserted is that *Windows' and Unix's codes land in exactly the
/// `ErrorKind`s the loop catches* — the part easiest to get wrong and the part that cannot be
/// read off the code.
#[test]
fn an_aborted_connection_is_not_a_broken_listener() {
    // Raw codes are translated by the *running* operating system, so each half only runs at
    // home — CI runs both. A pool dropping half-open sockets produces exactly these codes.
    #[cfg(windows)]
    {
        assert!(is_transient_accept(&Error::from_raw_os_error(10054))); // WSAECONNRESET
        assert!(is_transient_accept(&Error::from_raw_os_error(10053))); // WSAECONNABORTED
                                                                        // Running out of
                                                                        // handles is not about
                                                                        // one connection: wait,
                                                                        // then retry.
        assert!(!is_transient_accept(&Error::from_raw_os_error(10024))); // WSAEMFILE
    }
    // Unix has no shared number table: Linux numbers `ECONNRESET` 104, macOS 54 — so take them
    // from the `libc` of the operating system being built for, not hand-written numbers.
    #[cfg(unix)]
    {
        assert!(is_transient_accept(&Error::from_raw_os_error(
            libc::ECONNRESET
        )));
        assert!(is_transient_accept(&Error::from_raw_os_error(
            libc::ECONNABORTED
        )));
        assert!(!is_transient_accept(&Error::from_raw_os_error(
            libc::EMFILE
        )));
    }

    // And a signal interrupting the call, everywhere.
    assert!(is_transient_accept(&Error::from(ErrorKind::Interrupted)));

    // Two seconds of a silent port is the threshold for bothering the user: long enough for a
    // passing blip to clear up silently.
    assert_eq!(ACCEPT_RETRY * ACCEPT_ALARM, Duration::from_secs(2));
}
