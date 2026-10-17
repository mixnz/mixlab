//! The arc a front end has to walk, whichever program is being asked to be one — roadmap task
//! **T37**.
//!
//! `docs/roadmap/phase-3-services.md` asks T37 for "a parity test suite running both generators",
//! and this is the parity: one sequence of assertions, driven twice. What each front end supplies is
//! a [`FrontEnd`] — where its archive is, what its overrides are called, which line in the rendering
//! carries its control port — and everything a *user* meets is here, once.
//!
//! **Why it is one sequence and not two files that look alike.** The claim T31 made for Caddy and
//! T37 makes for nginx is the same sentence: a row becomes a configuration the server itself
//! accepts, the server comes up, an edited override is *served* by the same process a moment later,
//! a broken one is refused with the last good configuration still live, and a stop ends it. Two
//! copies of that would drift — and the copy that drifted would be the one nobody was reading on the
//! day it mattered, because both suites would still be green.
//!
//! What is deliberately **not** shared is how each server answers its own readiness: Caddy has an
//! admin endpoint and nginx has a `server` block MixEngine renders for the purpose. Those live in
//! the recipes, and each suite asserts its own in its own file.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mixengine_testkit::{FakePackage, Packed, Packing};
use serde_json::Value;

use super::{Home, json, stdout};

/// How long the server is given to be serving something new after a reload.
///
/// Long for what it covers, because what it is really waiting for is a runner's next turn plus a
/// reload on a machine that may be compiling something else at the same time.
const EVENTUALLY: Duration = Duration::from_secs(30);

/// One of the two programs a site can be reached through, as this suite has to know it.
pub(crate) struct FrontEnd {
    /// The `packages.name`, which is also the service id: a front end exists once and its id carries
    /// no `@`.
    pub package: &'static str,

    /// The environment variable naming the directory an unpacked archive is in, as the CI step and a
    /// developer both set it.
    pub variable: &'static str,

    /// The version the index publishes this as, and the one `mix service create` names.
    ///
    /// Nothing compares it against what the binary reports — a recipe is found by `packages.name` —
    /// but an index entry has to say something.
    pub version: &'static str,

    /// The file the recipe renders, under `etc/<service-id>/`.
    pub config: &'static str,

    /// How much of the unpacked archive the fixture has to carry. See [`Archive`].
    pub archive: Archive,

    /// Data files out of the archive that the generated configuration reaches by absolute path,
    /// as `provides` name and the relative path the index publishes for it.
    ///
    /// Empty for Caddy, which includes nothing; two for nginx — `mime.types`, which every rendering
    /// includes, and `fastcgi_params`, which a php-fpm site's `location ~ \.php$` does.
    pub data_files: &'static [(&'static str, &'static str)],

    /// The whole overrides document for a server whose control port is `control` and which is
    /// serving nothing.
    pub alone: fn(control: u16) -> String,

    /// The same, with a site pasted into the free-form override: `says` on `site`.
    pub serving: fn(control: u16, site: u16, says: &str) -> String,

    /// An overrides document this server's own checker has to refuse.
    pub broken: fn(control: u16) -> String,

    /// A `[[recipe.front_end]]` for this server that serves [`FRAGMENT_SAYS`] on the port the
    /// extension holds under `fragment_port` — roadmap task **T81c**.
    ///
    /// **Written in the manifest's placeholders and not in numbers**, because that is the whole of
    /// what an extension may write: `{listen}` comes from `permissions.network` and the port from
    /// `[ports]`, and an author who tried to spell either one out is refused at parse.
    pub fragment: &'static str,

    /// A `[[recipe.front_end]]` this server's own checker has to refuse, for the other half of D5.
    pub broken_fragment: &'static str,

    /// The line the rendering carries when the control port is `control`, which is how a test says
    /// "the file on disk is the one this row asked for" in each program's own spelling.
    pub control_line: fn(control: u16) -> String,

    /// What this server answers `200` on, and the one thing about a front end that is genuinely not
    /// shared: Caddy has an admin endpoint of its own, and nginx has a `server` block MixEngine
    /// renders precisely because it has none. Both are what the recipe's readiness check asks, and
    /// asking it here is what says the endpoint a *person* was told about is the endpoint that
    /// answers.
    pub control_path: &'static str,
}

/// How much of an unpacked archive a fixture has to carry.
pub(crate) enum Archive {
    /// One executable at the root and nothing else — how `mixengine-packages` publishes Caddy.
    OneProgram,

    /// The whole tree, because the generated configuration reaches a data file inside it — nginx,
    /// whose `conf/mime.types` every `Content-Type` it serves comes out of.
    WholeTree,
}

impl FrontEnd {
    /// Where an unpacked copy of this server is, or the reason there is none.
    ///
    /// **A panic and not a skip.** A test that quietly returns when it cannot find a server is a
    /// green suite that proved nothing on the day the download broke; the suites that call this are
    /// `#[ignore]`d, which is *visibly* not run, and CI fetches a real archive on all three systems.
    pub(crate) fn package_directory(&self) -> PathBuf {
        let directory = std::env::var_os(self.variable).unwrap_or_else(|| {
            panic!(
                "{} is not set, so there is no {} to judge this recipe against. The `{}` step in \
                 .github/workflows/_services.yml fetches one; by hand, unpack any {} and point {} at the \
                 directory holding the binary.",
                self.variable, self.package, self.package, self.package, self.variable
            )
        });

        let directory = PathBuf::from(directory);
        let binary = directory.join(self.binary());

        assert!(
            binary.is_file(),
            "{} is {}, which holds no {} binary",
            self.variable,
            directory.display(),
            self.package
        );

        directory
    }

    /// What the executable inside the archive is called on this system.
    pub(crate) fn binary(&self) -> String {
        format!("{}{}", self.package, std::env::consts::EXE_SUFFIX)
    }

    /// The real server, packed the way the index publishes it.
    pub(crate) fn pack(&self) -> Packed {
        let packing = match cfg!(windows) {
            true => Packing::Zip,
            false => Packing::TarZst,
        };

        let directory = self.package_directory();
        let stem = format!("{}-{}", self.package, self.version);

        match self.archive {
            Archive::OneProgram => FakePackage::new(packing)
                .program(&self.binary(), &directory.join(self.binary()))
                .build(&stem),
            Archive::WholeTree => FakePackage::new(packing).directory(&directory).build(&stem),
        }
    }

    /// What this front end names the per-site file it renders — roadmap task **T124a**.
    ///
    /// Derived from [`config`](Self::config)'s own extension rather than declared beside it: a
    /// front end that renders `nginx.conf` renders `sites/<domain>.conf`, and two fields could
    /// disagree.
    pub(crate) fn site_extension(&self) -> &'static str {
        match self.config.rsplit_once('.') {
            Some((_, extension)) => extension,
            // Caddy's is `Caddyfile`, which has no extension and whose site files are `.caddy`.
            None => "caddy",
        }
    }

    /// What this server's artifact publishes, by the names a recipe asks for them by.
    ///
    /// **Out here rather than inside [`index`](Self::index)** since roadmap task **T124a**: a suite
    /// that publishes a front end *beside* other packages — `php_site`, which needs a PHP in the
    /// same index — builds its own entry and would otherwise carry a second copy of this map. One
    /// forgotten `fastcgi_params` is an nginx that renders and will not start.
    pub(crate) fn provides(&self) -> serde_json::Map<String, Value> {
        let mut provides = serde_json::Map::new();
        provides.insert(self.package.to_owned(), Value::String(self.binary()));

        for (name, relative) in self.data_files {
            provides.insert((*name).to_owned(), Value::String((*relative).to_owned()));
        }

        provides
    }

    /// An index offering exactly this server, for this machine.
    fn index(&self, packed: &Packed, url: &str) -> Value {
        let provides = self.provides();

        serde_json::json!({
            "schema": 1,
            "generated_at": "2026-08-21T06:55:12Z",
            "packages": [{
                "kind": self.package,
                "version": self.version,
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
}

/// How many numbers are tried before this gives up on the machine rather than on the port.
///
/// **Five hundred and twelve, and the size is the finding.** Windows hands out ephemeral ports
/// *sequentially*, and its UDP exclusion ranges are blocks of a hundred that sit next to one
/// another — `49667–49766`, `49767–49866` and `49867–49966` are three in a row, three hundred
/// consecutive numbers. So a retry is not an independent draw: entering such a block means every
/// following candidate is inside it too, and a small budget fails on all of them. The first draft of
/// this loop tried twenty-five and panicked on a machine sitting at 65319, in the middle of
/// `65285–65484`.
///
/// What the budget therefore has to clear is the longest run of adjacent blocks, not the odd bad
/// port. Twice the longest run measured, so the loop walks out of the block and keeps going.
const CANDIDATES: usize = 512;

/// Where every number [`free_port`] hands out comes from.
///
/// **Below every operating system's ephemeral range**: Linux's is 32768–60999, Windows' and
/// macOS's 49152–65535, and those are the numbers a `bind(:0)` anywhere on the machine can be given.
/// Run 36175716790 (`services (ubuntu-latest, all)`) is what handing one out cost: Caddy crash-looped
/// on `listen tcp 127.0.0.1:40789: bind: address already in use`, because the number `free_port` had
/// just let go was given to another listener before Caddy bound it. Nothing binds port 0 into this
/// band, so the only takers left are the ones that ask for a number by name.
///
/// And clear of the ones that do, here: `mixengine_core::services::ports`' tests walk 24000–32000,
/// and the daemon searches 64 numbers up from each recipe's preferred port (3306, 5432, 6379, 9000,
/// 11211, 27017, 41000) and from the extensions' in these suites (18025, 18081).
const BAND: std::ops::Range<u16> = 19_000..24_000;

/// What a service wrote to its own log, or an empty string.
///
/// **`daemon.log` is not where a front end says what it did.** Output travels on its own stream and
/// into `logs/services/<id>/current.log`, per ADR 0009.
pub(crate) fn service_log(home: &Home, service: &str) -> String {
    std::fs::read_to_string(
        home.path()
            .join("logs")
            .join("services")
            .join(service)
            .join("current.log"),
    )
    .unwrap_or_default()
}

/// A port nothing is listening on, from [`BAND`], by listening on it and then not.
///
/// **Not `bind(:0)`, which is what this used to be, and the reason is run 36175716790.** Port 0 is
/// answered from the ephemeral range, and so is every other `bind(:0)` on the machine: a registry,
/// another suite's daemon. A number let go of there is one the next of them can be given, and hold
/// for the rest of the test. Caddy then crash-looped on `listen tcp 127.0.0.1:40789: bind: address
/// already in use`. Walking a band nothing binds port 0 into leaves only the takers that ask for a
/// number by name, and [`BAND`] is placed clear of those.
///
/// A window stays between the drop and the server's bind, as with any number handed over: a process
/// that names the same number in it takes it. The alternative is a fixed port, which two runs of
/// this suite on one machine would fight over.
///
/// **A TCP bind alone is not enough to hand the number to a front end**, which is what this used to
/// do. From T51 a site with a certificate makes Caddy open an **HTTP/3 listener**, which is UDP on
/// the TLS port — and Windows keeps port exclusion ranges *per protocol*, so a number TCP is welcome
/// on can refuse UDP outright with `An attempt was made to access a socket in a way forbidden by its
/// access permissions`. A server that cannot bind one of its listeners refuses its whole
/// configuration, so what that looked like from out here was Caddy in a crash loop and
/// `system (windows-latest)` red in `caddy.rs` — twice, hours apart, on branches that had not
/// touched a front end.
///
/// So the number is proved for **both** protocols before it is handed out, and the TCP listener is
/// held while the UDP half is tried: dropping it first would open a window for another process to
/// take the number between the two checks.
///
/// **And never the same number twice in one process.** The suites run their tests on threads of one
/// binary, and the operating system is free to hand a port that one test has let go of to the next
/// test that asks — while the first test's server has not bound it yet. Run 35668162118
/// (`services (windows-latest, web)`) is what that looks like: a site request answered `403` with a
/// JSON body, which is Caddy's admin endpoint, because another test's Caddy had been given the same
/// number for its control port. Remembering every number handed out closes that window between
/// tests; the one left is with other processes, which is the race described above.
pub(crate) fn free_port() -> u16 {
    static HANDED_OUT: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());

    // A different starting point in each process, so two suites running at once do not walk the band
    // in step and meet on every number.
    let width = usize::from(BAND.end - BAND.start);
    let start = {
        use std::hash::{BuildHasher as _, Hasher as _};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        usize::try_from(hasher.finish() % width as u64).expect("smaller than the band")
    };

    for step in 0..CANDIDATES {
        let offset = u16::try_from((start + step) % width).expect("smaller than the band");
        let port = BAND.start + offset;

        let mut handed_out = HANDED_OUT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if handed_out.contains(&port) {
            continue;
        }

        // Held while the udp half is tried: letting go first would open a window between the two
        // checks.
        let Ok(_held) = TcpListener::bind(("127.0.0.1", port)) else {
            continue;
        };
        if takes_udp(port) {
            handed_out.push(port);
            return port;
        }
    }

    panic!(
        "this machine refused the tcp or udp half of, or had already handed out, {CANDIDATES} ports \
         in a row in {}..{}. On Windows, `netsh interface ipv4 show excludedportrange udp` and \
         `… tcp` list the blocks the system has reserved there, and a reboot releases the dynamic \
         ones.",
        BAND.start, BAND.end
    );
}

/// Whether this port can be had for UDP at all.
///
/// **Loopback and not every interface, deliberately, and it costs nothing.** An exclusion range
/// refuses `127.0.0.1:49424` exactly as it refuses `0.0.0.0:49424` — measured, both with the same
/// message — so binding the wildcard buys no accuracy. What it does buy is a **Windows Firewall
/// prompt per test binary**, since this harness is compiled into every suite in the crate and each
/// one is a different executable listening on the network. The first draft did that and produced a
/// row of dialogs on a developer's screen.
fn takes_udp(port: u16) -> bool {
    std::net::UdpSocket::bind(("127.0.0.1", port)).is_ok()
}

/// **A port a front end could not take is not one to hand it.**
///
/// The deterministic half of this fix: hold the UDP side of a port, and the check must refuse it
/// even though the TCP side is completely free. That is exactly the shape the CI failure had — TCP
/// welcome, UDP not — arrived at by holding the port rather than by hoping the machine has one.
#[test]
fn a_port_whose_udp_half_is_taken_is_refused() {
    // **The port comes from the socket that holds it, and never from `free_port`.** An earlier
    // version asked `free_port` for a number and then bound its udp half, on the reasoning that a
    // number just proved free on both protocols is the cleanest starting point. It is not a
    // starting point at all: `free_port` answers by binding and then *letting go*, so between the
    // number coming back and the bind below, any of the dozen other test binaries in this crate can
    // take it — and this test failed on `bench`-free ubuntu with `AddrInUse` for exactly that
    // reason. The tcp half that reasoning was protecting is not looked at here anyway: what is
    // asserted is `takes_udp`, one protocol, on a port this test never stops holding.
    let _held = std::net::UdpSocket::bind(("127.0.0.1", 0)).expect("a loopback udp port");
    let port = _held.local_addr().expect("the port it was given").port();

    assert!(
        !takes_udp(port),
        "port {port} has its udp half held and was still offered to a front end"
    );
}

/// **A port handed out is one no `bind(:0)` on this machine can be given.**
///
/// Run 36175716790 is why: a number in Linux's ephemeral range, let go by `free_port`, was taken by
/// another listener before Caddy bound it. Only the band is asserted, never a number, because which
/// numbers are free is a fact about the machine.
#[test]
fn a_handed_out_port_is_below_every_ephemeral_range() {
    for _ in 0..8 {
        let port = free_port();

        assert!(
            BAND.contains(&port),
            "free_port handed out {port}, outside {}..{}: a number there can be given to any \
             listener that binds port 0 between this handing it out and the server binding it",
            BAND.start,
            BAND.end
        );
    }
}

// **There was a second test here, and removing it is the point.** It took twenty-five ports from
// `free_port` and asserted each still took a udp bind — and since it ran in the same binary as the
// test above, which deliberately holds the udp half of a port it was just handed, the two raced:
// whichever ran second could be handed the number the first was holding, in the window between
// `free_port` proving it free and the assertion asking again. Green on Windows, red on Linux and
// macOS, and flaky by construction — the exact thing this whole change exists to stop the harness
// producing. What it was meant to catch, `free_port` losing its check, is six lines above and reads
// plainly enough.

/// `GET /` on a loopback port, as raw as it can be, and whatever came back.
///
/// A hand-written request rather than an HTTP client, because what is being asked is small enough
/// that a client would be the bigger thing to get wrong: one connection, one request, read until the
/// server closes. `Connection: close` is what makes the read terminate.
///
/// **The `Host` header has to be the site's own address.** A Caddyfile block written as
/// `http://127.0.0.1:8080` matches on that host, so a request carrying any other one reaches a
/// listening Caddy and is answered `404` — which reads as a reload that did not happen and is a
/// header that did not match.
pub(crate) fn get(port: u16) -> Option<String> {
    request(port, "/")
}

/// The same, for a path that is not the root — the control endpoint each front end answers on.
pub(crate) fn request(port: u16, path: &str) -> Option<String> {
    request_as(port, path, &format!("127.0.0.1:{port}"))
}

/// The same again, addressed to a name rather than to the loopback address.
///
/// **The `Host` header is what a site is matched on**, and it is what T43's own steps need: a site
/// declared as `blog.test` is a Caddyfile block written `http://blog.test` and an nginx
/// `server_name blog.test`, and a request carrying any other host reaches a listening server and is
/// answered `404` — which reads as a reload that did not happen and is a header that did not match.
///
/// **The header rather than the name.** CI has no elevation, so no hosts entry exists, and what this
/// suite is for is proving that the rendering is right and the server is reading it — not that a
/// name resolves. Resolution is T44 and T45's, and has its own suites.
pub(crate) fn request_as(port: u16, path: &str, host: &str) -> Option<String> {
    request_at(std::net::Ipv4Addr::LOCALHOST, port, path, host)
}

/// The same again, at an address that is not loopback — roadmap task **T76**.
///
/// What a shared site is for: the request arrives at the machine's LAN address, carrying that
/// address as its `Host`, which is what a phone handed a URL actually sends.
pub(crate) fn request_at(
    address: std::net::Ipv4Addr,
    port: u16,
    path: &str,
    host: &str,
) -> Option<String> {
    let mut stream = TcpStream::connect((address, port)).ok()?;
    // **A deadline that cannot be set is an answer that did not come, not a bug.** Callers poll
    // this while a front end is reloading, and a connect the kernel accepted into the backlog can
    // be reset by the listener going away before the next line runs. macOS answers `setsockopt`
    // on a socket that has already taken that reset with `EINVAL`, where Linux and Windows let it
    // through and fail the read instead — so an `expect` here turned one lost connection during an
    // nginx reload into a red `test (macos-latest)`. Treating it like the connect and the read
    // beside it lets the poll come round again.
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .ok()?;

    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;

    Some(answer)
}

mod scenarios;
#[allow(unused_imports)]
pub(crate) use scenarios::*;
