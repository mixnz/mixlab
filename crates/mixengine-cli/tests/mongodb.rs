//! The MongoDB recipe against a **real** MongoDB.
//!
//! Everything else about the recipe is provable in one process and is proved there: the template
//! renders, the arguments differ per system, a bind address off loopback is refused. None of that
//! says what the task is about, which is that *`mongod` accepts what MixEngine generates, keeps what
//! it is given, and stops when it is asked to*. That can only be claimed against the program.
//!
//! **It is `#[ignore]`d rather than skipped**, for `redis.rs`' reason: a test that quietly returns
//! when it cannot find a MongoDB is a green suite that proved nothing on the day the download broke.
//! The `mongodb` step in `.github/workflows/_services.yml` fetches a real archive; without one, everything
//! here panics saying so.
//!
//! # Spoken to over the wire, not through a shell
//!
//! The artifact ships no client, and a server test that needs `mongosh` — a separately released
//! package — goes red for reasons that are not about the server. So the questions below are
//! `OP_MSG`s written by hand: a length-prefixed header, one section, one BSON document. That is the
//! whole of what three commands need, and it is the same trade `mixengine-packages` made in
//! `mongodb_smoke.py`.

mod harness;

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harness::{Home, json};
// Every port this suite asks for comes from `harness::frontend::free_port`, which hands out
// numbers no `bind(:0)` on the machine can be given (run 36175716790).
//
// Not 27017: a developer running this suite may well have a MongoDB of their own.
use harness::frontend::free_port;
use mixengine_testkit::{FakePackage, MockRegistry, Packed, Packing};
use serde_json::Value;

/// Where an unpacked MongoDB is, as the CI step and a developer both set it.
const PACKAGE: &str = "MIXENGINE_MONGODB_PACKAGE";

/// The version the index publishes this as, and the one `mix service create` names.
const VERSION: &str = "8.x";

/// The service this suite drives.
const SERVICE: &str = "mongodb@main";

/// How long the server is given to answer, or to stop answering, after it has been asked to.
const EVENTUALLY: Duration = Duration::from_secs(60);

/// `OP_MSG`'s opcode.
const OP_MSG: i32 = 2013;

/// The MongoDB this suite is about, or the reason there is none.
fn package() -> PathBuf {
    let directory = std::env::var_os(PACKAGE).unwrap_or_else(|| {
        panic!(
            "{PACKAGE} is not set, so there is no MongoDB to judge this recipe against. The \
             `mongodb` step in .github/workflows/_services.yml fetches one; by hand, unpack any MongoDB \
             from mixengine-packages' releases and point {PACKAGE} at the directory it unpacked to."
        )
    });

    PathBuf::from(directory)
}

/// What the artifact publishes, as an index entry says it.
fn provides(root: &Path) -> serde_json::Map<String, Value> {
    let mut found = serde_json::Map::new();

    for name in ["mongod", "mongos"] {
        let relative = Path::new("bin").join(format!("{name}{}", std::env::consts::EXE_SUFFIX));

        assert!(
            root.join(&relative).is_file(),
            "{PACKAGE} is {}, which holds no {}",
            root.display(),
            relative.display()
        );

        found.insert(
            name.to_owned(),
            Value::String(relative.to_string_lossy().replace('\\', "/")),
        );
    }

    found
}

/// The few BSON values three commands need.
enum Bson {
    /// A UTF-8 string.
    Text(&'static str),
    /// A 32-bit integer.
    Int(i32),
    /// A boolean, which BSON writes as one byte.
    Bool(bool),
    /// An embedded document, in order.
    Document(Vec<(&'static str, Bson)>),
    /// An array, which BSON writes as a document keyed `"0"`, `"1"`, ….
    Array(Vec<Bson>),
}

/// One BSON document: its length, its elements, a terminating zero.
fn document(elements: &[(&str, Bson)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (key, value) in elements {
        element(&mut body, key, value);
    }

    let length = i32::try_from(body.len() + 5).expect("a small document");
    let mut out = length.to_le_bytes().to_vec();
    out.extend(body);
    out.push(0);
    out
}

/// One element: its type byte, its key, its value.
fn element(out: &mut Vec<u8>, key: &str, value: &Bson) {
    out.push(match value {
        Bson::Text(_) => 0x02,
        Bson::Document(_) => 0x03,
        Bson::Array(_) => 0x04,
        Bson::Bool(_) => 0x08,
        Bson::Int(_) => 0x10,
    });
    out.extend(key.as_bytes());
    out.push(0);

    match value {
        Bson::Text(text) => {
            let length = i32::try_from(text.len() + 1).expect("a short string");
            out.extend(length.to_le_bytes());
            out.extend(text.as_bytes());
            out.push(0);
        }
        Bson::Int(number) => out.extend(number.to_le_bytes()),
        Bson::Bool(value) => out.push(u8::from(*value)),
        Bson::Document(fields) => {
            let mut body = Vec::new();
            for (key, value) in fields {
                element(&mut body, key, value);
            }
            wrap(out, body);
        }
        Bson::Array(items) => {
            let mut body = Vec::new();
            for (at, item) in items.iter().enumerate() {
                element(&mut body, &at.to_string(), item);
            }
            wrap(out, body);
        }
    }
}

/// The elements of an embedded document, framed as one.
fn wrap(out: &mut Vec<u8>, body: Vec<u8>) {
    let length = i32::try_from(body.len() + 5).expect("a small document");
    out.extend(length.to_le_bytes());
    out.extend(body);
    out.push(0);
}

/// Send one command as an `OP_MSG` and return the reply after its length, or [`None`] when nothing
/// answers.
fn command(port: u16, elements: &[(&str, Bson)]) -> Option<Vec<u8>> {
    let mut message = 0u32.to_le_bytes().to_vec(); // flagBits
    message.push(0); // one section of kind 0: the body
    message.extend(document(elements));

    let length = i32::try_from(16 + message.len()).expect("a small message");
    let mut frame = length.to_le_bytes().to_vec();
    frame.extend(1i32.to_le_bytes()); // requestID
    frame.extend(0i32.to_le_bytes()); // responseTo
    frame.extend(OP_MSG.to_le_bytes());
    frame.extend(message);

    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    stream.write_all(&frame).ok()?;

    let mut length = [0u8; 4];
    stream.read_exact(&mut length).ok()?;
    let length = usize::try_from(i32::from_le_bytes(length)).ok()?;
    let mut rest = vec![0u8; length.checked_sub(4)?];
    stream.read_exact(&mut rest).ok()?;

    Some(rest)
}

/// Whether `haystack` holds `needle` anywhere.
fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// `{hello: 1}` — the first thing every driver sends, answered before anything is authenticated.
fn hello(port: u16) -> Option<Vec<u8>> {
    command(
        port,
        &[("hello", Bson::Int(1)), ("$db", Bson::Text("admin"))],
    )
}

/// Whether a server on `port` answers `hello` as a primary would.
fn answers(port: u16) -> bool {
    hello(port).is_some_and(|reply| holds(&reply, b"isWritablePrimary"))
}

/// Wait for `wanted` to hold, or say what it was still doing when the deadline passed.
fn eventually(what: &str, wanted: impl Fn() -> bool) {
    let deadline = Instant::now() + EVENTUALLY;

    while !wanted() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// An index offering exactly this MongoDB, for this machine.
fn index(packed: &Packed, url: &str, provides: serde_json::Map<String, Value>) -> Value {
    serde_json::json!({
        "schema": 1,
        "generated_at": "2026-09-17T06:55:12Z",
        "packages": [{
            "kind": "mongodb",
            "version": VERSION,
            "channel": "stable",
            "artifacts": [{
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "url": url,
                "sha256": packed.sha256,
                "size": packed.size(),
                "provides": provides,
            }],
        }],
    })
}

/// A home with a real MongoDB installed in it, a service created against it, and a daemon over both.
async fn created() -> (Home, harness::Daemon, MockRegistry, u16) {
    let root = package();
    let port = free_port();

    let packing = if cfg!(windows) {
        Packing::Zip
    } else {
        Packing::TarZst
    };
    let packed = FakePackage::new(packing)
        .directory(&root)
        .build(&format!("mongodb-{VERSION}"));

    let registry = MockRegistry::start(&serde_json::json!({
        "schema": 1, "generated_at": "2026-09-17T06:55:12Z", "packages": []
    }))
    .await;
    let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
    registry.publish(&index(&packed, &url, provides(&root)));

    let home = Home::new();
    let daemon = home.start_daemon_reading_index(&registry.url(), registry.public_key());

    let installed = json(&home.mix(&["package", "install", "mongodb", VERSION, "--json"]));
    assert_eq!(
        installed["state"],
        "succeeded",
        "{installed}\n{}",
        home.daemon_log()
    );

    let created = json(&home.mix(&[
        "service",
        "create",
        SERVICE,
        VERSION,
        "--port",
        &port.to_string(),
        "--json",
    ]));
    assert_eq!(
        created["service"]["id"],
        SERVICE,
        "{created}\n{}",
        home.daemon_log()
    );

    (home, daemon, registry, port)
}

/// **The whole of the recipe, in the order a user meets it.**
///
/// One test rather than five, for `redis.rs`' reason: each step is the previous one's precondition.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real MongoDB — see the module note, and the `mongodb` step in _services.yml"]
async fn a_database_is_generated_started_written_to_restarted_whole_and_stopped() {
    let (home, _daemon, _registry, port) = created().await;

    // --- generated and started ------------------------------------------------------------------
    let started = json(&home.mix(&["service", "start", SERVICE, "--json"]));
    assert_eq!(
        started["complete"],
        true,
        "{started}\n{}",
        home.daemon_log()
    );

    let config = home.path().join("etc").join(SERVICE).join("mongod.conf");
    let rendered = std::fs::read_to_string(&config).expect("the generated mongod.conf");
    assert!(rendered.contains(&format!("port: {port}")), "{rendered}");

    // --- it is *this* server on *this* port -------------------------------------------------------
    //
    // The port was free a moment ago, and a `mongod` that had not read the file would be on 27017.
    assert!(answers(port), "nothing answered hello on {port}");

    // --- it is a database, and it keeps what it is given ------------------------------------------
    //
    // **Journaled before it is acknowledged.** A restart on Windows is a kill, and `mongod` flushes
    // its journal every 100 ms, so an insert acknowledged without `j` is lost whenever the kill lands
    // inside that window — which the Windows CI leg found on 2026-09-17. `j: true` asks for exactly
    // the promise the assertion below checks: what was acknowledged survives the process.
    let inserted = command(
        port,
        &[
            ("insert", Bson::Text("greetings")),
            (
                "documents",
                Bson::Array(vec![Bson::Document(vec![
                    ("_id", Bson::Int(1)),
                    ("text", Bson::Text("hello")),
                ])]),
            ),
            (
                "writeConcern",
                Bson::Document(vec![("j", Bson::Bool(true))]),
            ),
            ("$db", Bson::Text("mixengine")),
        ],
    )
    .expect("an answer to insert");
    assert!(
        holds(&inserted, &[0x10, b'n', 0, 1, 0, 0, 0]),
        "one document should have been inserted: {inserted:?}"
    );

    let restarted = json(&home.mix(&["service", "restart", SERVICE, "--json"]));
    assert_eq!(
        restarted["complete"],
        true,
        "{restarted}\n{}",
        home.daemon_log()
    );

    eventually("the restarted server never answered hello", || {
        answers(port)
    });

    let found = command(
        port,
        &[
            ("find", Bson::Text("greetings")),
            ("filter", Bson::Document(Vec::new())),
            ("$db", Bson::Text("mixengine")),
        ],
    )
    .expect("an answer to find");
    assert!(
        holds(&found, b"hello"),
        "a database that lost a document across a restart is not one: {found:?}"
    );

    // --- stopped ----------------------------------------------------------------------------------
    let stopped = json(&home.mix(&["service", "stop", SERVICE, "--json"]));
    assert_eq!(
        stopped["complete"],
        true,
        "{stopped}\n{}",
        home.daemon_log()
    );

    eventually("the server went on answering after it was stopped", || {
        hello(port).is_none()
    });

    let status = json(&home.mix(&["service", "status", SERVICE, "--json"]));
    assert_eq!(
        status["state"],
        "stopped",
        "{status}\n{}",
        home.daemon_log()
    );
}
