//! `msys2`, a toolchain: listed, installed and removed like a package, never made into a service —
//! roadmap task **T206a**, its design's D5.
//!
//! **Multi-threaded on purpose**, for `tests/package.rs`' reason: `MockRegistry` serves the daemon
//! `mix` talks to, in this same process, while `mix` runs as a blocking child.

mod harness;

use harness::{Home, stdout};
use mixengine_testkit::{FakePackage, MockRegistry, Packed, Packing};
use serde_json::{Value, json as document};

/// The date-shaped version the packaging repository gives a build.
const VERSION: &str = "2026.10.08";

/// The one toolchain this build knows.
const PACKAGE: &str = "msys2";

/// The name the archive publishes its one program under; the program itself is never run here.
fn program_name() -> String {
    format!("fakeservice{}", std::env::consts::EXE_SUFFIX)
}

/// A home, a daemon in it, and a registry offering a `msys2` for this machine.
struct Fixture {
    home: Home,
    _registry: MockRegistry,
    _daemon: harness::Daemon,
}

impl Fixture {
    async fn start() -> Self {
        let packing = match cfg!(windows) {
            true => Packing::Zip,
            false => Packing::TarZst,
        };
        let packed = FakePackage::new(packing)
            .executable(&program_name())
            .build(&format!("{PACKAGE}-{VERSION}"));

        let registry = MockRegistry::start(&document!({
            "schema": 1, "generated_at": "2026-10-08T06:55:12Z", "packages": []
        }))
        .await;

        let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
        registry.publish(&index(&packed, &url));

        let home = Home::new();
        let daemon = home.start_daemon_reading_index(&registry.url(), registry.public_key());

        Self {
            home,
            _registry: registry,
            _daemon: daemon,
        }
    }
}

/// An index offering exactly one `msys2`, for this machine.
fn index(packed: &Packed, url: &str) -> Value {
    document!({
        "schema": 1,
        "generated_at": "2026-10-08T06:55:12Z",
        "packages": [{
            "kind": PACKAGE,
            "version": VERSION,
            "channel": "stable",
            "artifacts": [{
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "url": url,
                "sha256": packed.sha256,
                "size": packed.size(),
                "provides": { "bash": program_name() },
            }],
        }],
    })
}

/// **A toolchain is listed, installed and removed like a package** — T206a, D5.
#[tokio::test(flavor = "multi_thread")]
async fn a_toolchain_is_listed_installed_and_removed() {
    let fixture = Fixture::start().await;
    let home = &fixture.home;

    let listed = stdout(&home.mix(&["package", "available"]));
    assert!(listed.contains(PACKAGE), "{listed}");

    let installed = home.mix(&["package", "install", PACKAGE, VERSION]);
    assert!(
        installed.status.success(),
        "{}{}",
        stdout(&installed),
        String::from_utf8_lossy(&installed.stderr)
    );

    let removed = home.mix(&["package", "uninstall", PACKAGE, VERSION]);
    assert!(
        removed.status.success(),
        "{}{}",
        stdout(&removed),
        String::from_utf8_lossy(&removed.stderr)
    );
}

/// **And it is never a service** — T206a, D5: the sentence says what it is, rather than "cannot
/// run" beside a list of what can that would not name it.
#[tokio::test(flavor = "multi_thread")]
async fn a_toolchain_is_not_a_service() {
    let fixture = Fixture::start().await;
    let home = &fixture.home;
    home.mix(&["package", "install", PACKAGE, VERSION]);

    let refused = home.mix(&["service", "create", "msys2@main", VERSION]);
    assert!(!refused.status.success());

    let said = format!(
        "{}{}",
        stdout(&refused),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(said.contains("toolchain"), "{said}");
}
