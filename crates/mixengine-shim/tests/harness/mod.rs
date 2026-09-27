//! A home the way an install leaves one, and the shim binary sitting in its `bin/`.
//!
//! It lives here rather than in `mixengine-testkit` for the reason the CLI's own harness gives:
//! none of it is about MixEngine, it is about *this* suite — a fake runtime unpacked where
//! `provides` says it is, and `bin/` filled through the product's own [`shims::refresh`] so that
//! the `php` these tests run is the file a daemon start would have put there.
//!
//! **No `MIXENGINE_HOME` is ever set in this process.** Every case sets it on the child's own
//! `Command`, which is what `docs/standards/testing.md` requires and what lets these run in
//! parallel: `std::env::set_var` is process-global, and two homes in one binary would overwrite
//! each other.

// Each integration test binary compiles this module separately, so anything `shim.rs` uses and
// `overhead.rs` does not is dead code in one of them. The alternative is two fixtures that build a
// home slightly differently, which is the one thing a benchmark and the suite it is measured
// against must not have.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mixengine_core::runtimes::Installation;
use mixengine_core::{Store, paths, runtimes, shims};
use mixengine_proto::{PackageChannel, PackageVersion, RuntimeKind, ServiceId, Timestamp};

/// A fixed moment: nothing here asserts on time, and a fixture that read the clock would be one
/// more thing that can differ between two runs.
const NOW: Timestamp = Timestamp(1_760_000_000_000);

/// Where inside an install directory the fake runtime's program sits.
///
/// Nested rather than at the root, because that is the shape the Unix artifacts have and it is the
/// one that would break a shim which assumed the executable is the directory itself.
pub(crate) fn published_at() -> String {
    format!("bin/php{}", std::env::consts::EXE_SUFFIX)
}

/// A home with runtimes installed in it, and a `bin/` holding the shim under a real command name.
pub(crate) struct Home {
    root: tempfile::TempDir,
}

impl Home {
    /// A home holding one PHP per version named, the first of them the default — which is what
    /// installing them in this order really does.
    pub(crate) fn with(versions: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("a temporary home");
        let home = Self { root };

        let database = home.path().join(paths::DATABASE_FILE_NAME);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own writes");

        runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");

            for version in versions {
                let directory = home.runtime_directory(version);
                home.unpack_a_fake_runtime(&directory);

                runtimes::remember(
                    &store,
                    &Installation {
                        kind: RuntimeKind::Php,
                        version: PackageVersion::parse(*version).expect("a version"),
                        channel: PackageChannel::Stable,
                        path: directory,
                        bytes: 41_000_000,
                        url: format!("https://example.invalid/php-{version}.tar.zst"),
                        sha256: "00".to_owned(),
                        provides: [("php".to_owned(), published_at())].into_iter().collect(),
                        extension_dir: None,
                        extensions: mixengine_core::index::Extensions::default(),
                    },
                    NOW,
                )
                .await
                .expect("a row");
            }

            // Closed rather than dropped, so the write-ahead log is checkpointed and the `-shm`
            // file goes: a shim opening the database read-only afterwards is the case that has to
            // work on a machine where no daemon has run since the last reboot.
            store.close().await;
        });

        home.fill_bin();
        home
    }

    pub(crate) fn path(&self) -> &Path {
        self.root.path()
    }

    pub(crate) fn runtime_directory(&self, version: &str) -> PathBuf {
        self.path().join("runtimes").join("php").join(version)
    }

    /// What an install would have left on disk: the program, where `provides` says it is.
    fn unpack_a_fake_runtime(&self, directory: &Path) {
        let program = directory.join(published_at());
        std::fs::create_dir_all(program.parent().expect("a bin directory")).expect("a directory");

        // Copied rather than linked, and one copy per version, because the whole question is which
        // of two identical programs ran — the answer is the path it ran from.
        std::fs::copy(mixengine_testkit::package::executable_source(), &program).unwrap_or_else(
            |error| panic!("copy the fake runtime to {}: {error}", program.display()),
        );
    }

    /// Fill `bin/` the way a daemon start does — **through the product's own function**.
    ///
    /// Roadmap task T26. It used to copy one file under one name, which proved the shim reads
    /// `argv[0]` and proved nothing whatever about how a real `bin/` comes to exist. Going through
    /// [`shims::refresh`] is what makes this suite the end-to-end claim of Phase 2's milestone
    /// rather than half of it: the directory a person's PATH points at is filled by the code that
    /// fills it, and the `php` run below is the file that code put there.
    pub(crate) fn fill_bin(&self) -> shims::Refreshed {
        self.fill_bin_also_fronting(&[])
    }

    /// The same, plus names nothing in this home claims.
    ///
    /// For the one case that has to exist: a copy left in `bin/` after whatever put it there was
    /// uninstalled, which is the state a person meets in the moment between removing a database
    /// and the next refresh.
    pub(crate) fn front(&self, name: &str) {
        self.fill_bin_also_fronting(&[name]);
    }

    /// Fill `bin/` from copies of this build's shim and trampoline put in `install` — T185.
    ///
    /// For a test that replaces the resolver while a program is running, which it may not do to the
    /// build directory every other test is reading from.
    pub(crate) fn fill_bin_from(&self, install: &Path) -> shims::Refreshed {
        let built = Path::new(env!("CARGO_BIN_EXE_mixengine-shim"))
            .parent()
            .expect("the build directory");

        for name in [shims::BINARY, shims::TRAMPOLINE] {
            let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);

            // The trampoline is only needed, and only required to be built, on Windows.
            if built.join(&file).is_file() {
                std::fs::copy(built.join(&file), install.join(&file))
                    .unwrap_or_else(|error| panic!("copy {file} into the test's install: {error}"));
            }
        }

        let source = shims::source(&install.join("mixengined")).expect("both copied");
        let bin = self.path().join("bin");

        // Emptied first: `fs::copy` keeps the modification time on Windows, so a `bin/` already
        // filled from the build directory would look current and keep naming *those* files.
        shims::clear(&bin).expect("bin/ can be emptied");

        shims::refresh(&bin, &source, &shims::every_kind(), &self.client_extras())
            .expect("bin/ can be filled in a temporary home")
    }

    fn fill_bin_also_fronting(&self, names: &[&str]) -> shims::Refreshed {
        let mut extra = self.client_extras();

        extra.extend(names.iter().map(|name| shims::Extra {
            name: (*name).to_owned(),
            origin: shims::Origin::Global {
                kind: RuntimeKind::Node,
            },
        }));

        shims::refresh(
            &self.path().join("bin"),
            &built_source(),
            &shims::every_kind(),
            &extra,
        )
        .expect("bin/ can be filled in a temporary home")
    }

    /// The client commands of the installed service packages — roadmap task **T130**.
    ///
    /// **The daemon's own walk**, through the same two functions `crate::shims::Shims::extras` uses,
    /// for [`fill_bin`](Self::fill_bin)'s reason one step further along: what this suite claims is
    /// that a name a person can type is a name the shim then resolves, and a fixture that composed
    /// `bin/` by hand would be asserting only the second half of that.
    fn client_extras(&self) -> Vec<shims::Extra> {
        let database = self.path().join(paths::DATABASE_FILE_NAME);

        if !database.is_file() {
            return Vec::new();
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own reads");

        runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");
            let catalogue = mixengine_core::generate::Catalogue::builtin();

            let claims = mixengine_core::services::client::claims(&store, &catalogue, None)
                .await
                .expect("the installed rows can be read");

            store.close().await;

            shims::resolve_claims(&claims).0
        })
    }

    /// What `npm install -g` leaves behind, and the pass that notices it — roadmap task **T131**.
    ///
    /// The program is written into the runtime's own bindir (on Windows under every spelling npm
    /// really writes, so the case that one command comes out of three files is exercised rather
    /// than assumed), the scan records which language it belongs to, and `bin/` is refilled.
    pub(crate) fn install_globally(&self, kind: RuntimeKind, version: &str, name: &str) {
        let install = self
            .path()
            .join("runtimes")
            .join(kind.as_str())
            .join(version);

        let bindir = mixengine_core::runtimes::globals::directory(kind, &install)
            .expect("a language with a bindir");
        std::fs::create_dir_all(&bindir).expect("a bindir");

        let spellings: Vec<String> = match cfg!(windows) {
            true => vec![
                format!("{name}.cmd"),
                format!("{name}.ps1"),
                name.to_owned(),
            ],
            false => vec![name.to_owned()],
        };

        for spelling in &spellings {
            let file = bindir.join(spelling);

            // Only the one this system would actually run is the recording program; the others are
            // the data npm writes beside it, and a scan that turned them into commands would be the
            // bug the Windows case here is for.
            if cfg!(windows) && !spelling.ends_with(".cmd") {
                std::fs::write(&file, b"not a program").expect("a file");
                continue;
            }

            std::fs::copy(mixengine_testkit::package::executable_source(), &file)
                .unwrap_or_else(|error| panic!("copy to {}: {error}", file.display()));
        }

        // A `.cmd` cannot be handed arguments the way an `.exe` can, and what these cases assert is
        // the resolution rather than Windows' batch quoting — so the recording program is *also*
        // written under the name the loader tries first.
        if cfg!(windows) {
            let exe = bindir.join(format!("{name}.exe"));
            std::fs::copy(mixengine_testkit::package::executable_source(), &exe)
                .unwrap_or_else(|error| panic!("copy to {}: {error}", exe.display()));
        }

        self.rescan();
    }

    /// This home's certificate authority, where `certs::ca` puts one — roadmap task **T133**.
    ///
    /// **Not a real certificate**, because nothing in the shim parses it: what is being asserted is
    /// *which file a runtime is pointed at*, and the only property that matters is that the file
    /// exists. A generated authority would be a key pair per test for no assertion.
    pub(crate) fn write_authority(&self) {
        let certificate = mixengine_core::certs::ca::certificate_path(&self.path().join("certs"));

        std::fs::create_dir_all(certificate.parent().expect("a ca directory"))
            .expect("a directory");
        std::fs::write(&certificate, b"-----BEGIN CERTIFICATE-----\nnot really\n")
            .expect("an authority");
    }

    /// The bundle a daemon start writes, where `generate::ca` puts one.
    pub(crate) fn write_trust_bundle(&self) {
        let bundle = mixengine_core::generate::ca::path(&self.path().join("etc"));

        std::fs::create_dir_all(bundle.parent().expect("a ca directory")).expect("a directory");
        std::fs::write(&bundle, b"# a bundle, as far as a file name is concerned\n")
            .expect("a bundle");
    }

    /// Take a runtime away, the way `runtime.uninstall` does: the tree, then the row, then the pass.
    pub(crate) fn uninstall_runtime(&self, kind: RuntimeKind, version: &str) {
        let directory = self
            .path()
            .join("runtimes")
            .join(kind.as_str())
            .join(version);

        std::fs::remove_dir_all(&directory).expect("a removable directory");

        let database = self.path().join(paths::DATABASE_FILE_NAME);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own writes");

        runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");

            runtimes::forget(
                &store,
                kind,
                &PackageVersion::parse(version).expect("a version"),
            )
            .await
            .expect("the row can be removed");

            store.close().await;
        });

        self.rescan();
    }

    /// The discovery pass, as the daemon runs it: scan every installed runtime, record, refill.
    pub(crate) fn rescan(&self) {
        let database = self.path().join(paths::DATABASE_FILE_NAME);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own writes");

        runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");

            let found = mixengine_core::runtimes::globals::everywhere(&store)
                .await
                .expect("the installed runtimes can be read");

            mixengine_core::bin_commands::record(&store, &found)
                .await
                .expect("the projection can be written");

            store.close().await;
        });

        self.fill_bin_with_globals();
    }

    /// `bin/` as the daemon composes it: the clients, plus every discovered tool.
    fn fill_bin_with_globals(&self) -> shims::Refreshed {
        let database = self.path().join(paths::DATABASE_FILE_NAME);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own reads");

        let globals: Vec<shims::Extra> = runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");
            let recorded = mixengine_core::bin_commands::all(&store)
                .await
                .expect("the projection can be read");
            store.close().await;

            recorded
                .into_iter()
                .map(|(name, kind)| shims::Extra {
                    name,
                    origin: shims::Origin::Global { kind },
                })
                .collect()
        });

        let mut extra = self.client_extras();
        extra.extend(globals);

        shims::refresh(
            &self.path().join("bin"),
            &built_source(),
            &shims::every_kind(),
            &extra,
        )
        .expect("bin/ can be filled in a temporary home")
    }

    /// A service package on disk and in the database, optionally with one instance of it.
    ///
    /// `provides` is the artifact's own map — the keys are MixEngine's names and the values are
    /// wherever the publisher put the file — and every value becomes a copy of the recording
    /// program, so a command that reaches one can say which file it was and what it was told.
    pub(crate) fn install_package(
        &self,
        package: &str,
        version: &str,
        provides: &[(&str, &str)],
        instance: Option<(&str, u16)>,
    ) {
        let directory = self.path().join("packages").join(package).join(version);

        let provides: BTreeMap<String, String> = provides
            .iter()
            .map(|(name, at)| ((*name).to_owned(), (*at).to_owned()))
            .collect();

        for published in provides.values() {
            let program = directory.join(published);
            std::fs::create_dir_all(program.parent().expect("a directory")).expect("a directory");
            std::fs::copy(mixengine_testkit::package::executable_source(), &program)
                .unwrap_or_else(|error| panic!("copy to {}: {error}", program.display()));
        }

        let database = self.path().join(paths::DATABASE_FILE_NAME);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own writes");

        runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");

            mixengine_core::packages::remember(
                &store,
                &mixengine_core::packages::Installation {
                    package: package.to_owned(),
                    version: PackageVersion::parse(version).expect("a version"),
                    path: directory.clone(),
                    bytes: 41_000_000,
                    url: format!("https://example.invalid/{package}-{version}.tar.zst"),
                    sha256: "00".to_owned(),
                    provides,
                },
                NOW,
            )
            .await
            .expect("a packages row");

            store.close().await;
        });

        if let Some((id, port)) = instance {
            self.instantiate(package, version, id, port);
        }

        self.fill_bin();
    }

    /// One more instance of an already-installed package, on a port this test chose.
    ///
    /// **[`Port::Fixed`] and never [`Port::Allocate`]**, which is what keeps this suite honest on a
    /// busy machine: allocation asks the operating system whether a number is free, and whether
    /// 3307 is free is a property of the machine rather than of MixEngine
    /// (`docs/standards/testing.md`). What these cases assert is which *row* a command resolves
    /// to, and a row is a row whether or not anything is listening on it.
    pub(crate) fn instantiate(&self, package: &str, version: &str, id: &str, port: u16) {
        let database = self.path().join(paths::DATABASE_FILE_NAME);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own writes");

        runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");

            mixengine_core::services::create(
                &store,
                mixengine_platform::host().as_ref(),
                &mixengine_core::services::Declaration {
                    service: ServiceId::parse(id).expect("a service id"),
                    origin: mixengine_core::services::Origin::Package {
                        name: package.to_owned(),
                        version: PackageVersion::parse(version).expect("a version"),
                    },
                    instance_name: id.to_owned(),
                    port: mixengine_core::services::Port::Fixed(port),
                    bind_addr: None,
                    data_dir: None,
                    autostart: false,
                    overrides: "{}".to_owned(),
                },
            )
            .await
            .expect("a services row");

            store.close().await;
        });

        self.fill_bin();
    }

    /// Another language installed beside the PHPs, publishing what its real artifact publishes.
    ///
    /// Not a second fixture but a method on this one, because what it is here to exercise is a home
    /// with **more than one** language in it: `bin/` is one directory holding shims for all of them,
    /// and a `node` row must not change what `php` resolves to.
    pub(crate) fn install(
        &self,
        kind: RuntimeKind,
        version: &str,
        provides: BTreeMap<String, String>,
    ) {
        let directory = self
            .path()
            .join("runtimes")
            .join(kind.as_str())
            .join(version);

        for published in provides.values() {
            let program = directory.join(published);
            std::fs::create_dir_all(program.parent().expect("a directory")).expect("a directory");
            if program.extension().is_some_and(|kind| kind == "cmd") {
                continue; // written by the case that wants one, since its contents are the point
            }
            std::fs::copy(mixengine_testkit::package::executable_source(), &program)
                .unwrap_or_else(|error| panic!("copy to {}: {error}", program.display()));
        }

        let database = self.path().join(paths::DATABASE_FILE_NAME);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the fixture's own writes");

        runtime.block_on(async {
            let store = Store::open(&database).await.expect("a database");
            runtimes::remember(
                &store,
                &Installation {
                    kind,
                    version: PackageVersion::parse(version).expect("a version"),
                    channel: PackageChannel::Stable,
                    path: directory,
                    bytes: 37_000_000,
                    url: format!("https://example.invalid/{}-{version}.zip", kind.as_str()),
                    sha256: "00".to_owned(),
                    provides,
                    extension_dir: None,
                    extensions: mixengine_core::index::Extensions::default(),
                },
                NOW,
            )
            .await
            .expect("a row");
            store.close().await;
        });
    }

    /// A project directory under this home's temporary root, with the manifest it pins with.
    pub(crate) fn project(&self, name: &str, manifest: Option<&str>) -> PathBuf {
        let directory = self.path().join("projects").join(name);
        std::fs::create_dir_all(&directory).expect("a project directory");

        if let Some(body) = manifest {
            std::fs::write(directory.join("mixengine.toml"), body).expect("a manifest");
        }

        directory
    }

    /// The shim in `bin/` that answers to `command`, as a path a `Command` can be built from.
    pub(crate) fn shim(&self, command: &str) -> PathBuf {
        self.path()
            .join("bin")
            .join(format!("{command}{}", std::env::consts::EXE_SUFFIX))
    }

    /// Run `bin/php` from `cwd` and have the program it becomes write down what it was handed.
    ///
    /// **`--touch` is load-bearing rather than decoration.** `fakeservice --dump-env` records the
    /// environment and then goes on to *be a service*, which in a test is not a failure but a hang;
    /// the touch file is what makes it a one-shot, and it doubles as the proof that the arguments
    /// reached the program at all. Every case that expects a program to run goes through here so
    /// that none of them can forget it.
    pub(crate) fn record(
        &self,
        cwd: &Path,
        session: &BTreeMap<&str, String>,
        exit_code: i32,
    ) -> Recorded {
        self.record_command("php", cwd, session, exit_code)
    }

    /// The same for a command that is not `php`, which is what a home with a second language in it
    /// needs: the recording is about the shim and not about PHP.
    pub(crate) fn record_command(
        &self,
        command: &str,
        cwd: &Path,
        session: &BTreeMap<&str, String>,
        exit_code: i32,
    ) -> Recorded {
        let dump = cwd.join(format!("environment-{command}.txt"));
        let touched = cwd.join(format!("ran-{command}.txt"));

        let run = self.run_with(
            command,
            cwd,
            &[
                "--dump-env",
                &dump.display().to_string(),
                "--touch",
                &touched.display().to_string(),
                "--exit-code",
                &exit_code.to_string(),
            ],
            session,
        );

        // A run that refused has no dump to read, and reading one would fail on the file rather
        // than on the assertion the case is about.
        let reached = touched.is_file();

        Recorded {
            environment: if reached {
                dumped(&dump)
            } else {
                BTreeMap::new()
            },
            reached,
            run,
        }
    }

    /// The same, with variables the user's session would have exported.
    /// The same as [`record_command`](Self::record_command), with `cleared` taken *out* of the
    /// child's environment first — roadmap task **T133**.
    ///
    /// **Because a runner is a machine somebody set up.** `ubuntu-latest` exports
    /// `SSL_CERT_FILE=/usr/lib/ssl/cert.pem`, so a shim running there correctly leaves it alone —
    /// the design's D11 — and a case asserting that MixEngine's own value arrives was asserting
    /// about this machine rather than about the shim. The case that a person's value *wins* is a
    /// separate one below, and it is the more valuable of the two.
    pub(crate) fn record_without(
        &self,
        command: &str,
        cwd: &Path,
        cleared: &[&str],
        exit_code: i32,
    ) -> Recorded {
        let dump = cwd.join(format!("environment-{command}.txt"));
        let touched = cwd.join(format!("ran-{command}.txt"));

        let run = self.run_clearing(
            command,
            cwd,
            &[
                "--dump-env",
                &dump.display().to_string(),
                "--touch",
                &touched.display().to_string(),
                "--exit-code",
                &exit_code.to_string(),
            ],
            cleared,
        );

        let reached = touched.is_file();

        Recorded {
            environment: if reached {
                dumped(&dump)
            } else {
                BTreeMap::new()
            },
            reached,
            run,
        }
    }

    /// Run with `cleared` removed from what the child inherits.
    fn run_clearing(&self, command: &str, cwd: &Path, arguments: &[&str], cleared: &[&str]) -> Run {
        let shim = self.shim(command);
        let mut child = Command::new(&shim);

        child
            .args(arguments)
            .current_dir(cwd)
            .env("MIXENGINE_HOME", self.path());

        for name in cleared {
            child.env_remove(name);
        }

        let output = child
            .output()
            .unwrap_or_else(|error| panic!("run {}: {error}", shim.display()));

        Run { output }
    }

    pub(crate) fn run_with(
        &self,
        command: &str,
        cwd: &Path,
        arguments: &[&str],
        environment: &BTreeMap<&str, String>,
    ) -> Run {
        let shim = self.shim(command);

        let output = Command::new(&shim)
            .args(arguments)
            .current_dir(cwd)
            // On the child, never on this process: the home is an argument here, exactly as the
            // testing standard requires, and it happens to be spelled as a variable because a shim
            // has nowhere else to be told.
            .env("MIXENGINE_HOME", self.path())
            .envs(environment)
            .output()
            .unwrap_or_else(|error| panic!("run {}: {error}", shim.display()));

        Run { output }
    }
}

/// A run whose program was asked to record what it was handed.
pub(crate) struct Recorded {
    /// The run itself: its status and whatever the shim said on the way out.
    pub(crate) run: Run,

    /// The environment the program was given, or empty if it never ran.
    environment: BTreeMap<String, String>,

    /// Whether the program ran at all, which is what its own arguments arriving proves.
    pub(crate) reached: bool,
}

impl Recorded {
    /// One variable the program recorded, by name.
    ///
    /// **Case-insensitively on Windows**, which is the operating system's own rule and not a
    /// courtesy: an environment block there holds one entry per name whatever case it is written
    /// in, and the *spelling* that survives is whichever the parent process happened to use. A CI
    /// runner exports `Path`; this repository's shell exports `PATH`; the shim writes `PATH` and
    /// the child is handed the parent's spelling carrying the shim's value. A lookup that folded no
    /// case therefore answered `None` on a green machine — found by the Windows runner, on a case
    /// that passes here.
    pub(crate) fn recorded(&self, name: &str) -> Option<&str> {
        if !cfg!(windows) {
            return self.environment.get(name).map(String::as_str);
        }

        self.environment
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Which runtime really ran: the first entry of the `PATH` the program was given.
    pub(crate) fn ran_from(&self) -> PathBuf {
        let path = self
            .environment
            .iter()
            // Windows spells it `Path`, and a child's block keeps whichever spelling it already had.
            .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no PATH was recorded: {}", self.run.stderr()));

        std::env::split_paths(&path)
            .next()
            .expect("the PATH has an entry")
    }
}

/// One run of a shim, and what can be asked of it afterwards.
pub(crate) struct Run {
    output: Output,
}

impl Run {
    /// The status a shell would see.
    pub(crate) fn code(&self) -> i32 {
        self.output.status.code().expect("the child exited")
    }

    pub(crate) fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }
}

/// The environment the program was handed, as `fakeservice --dump-env` recorded it.
fn dumped(path: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("the program did not record its environment: {error}"));

    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// What this build fills `bin/` from: the shim, and on Windows the trampoline beside it (T185).
///
/// `cargo test -p mixengine-shim` builds this package's binary and no other, so on Windows the
/// trampoline is there only after a workspace build or its own — the message says which.
pub(crate) fn built_source() -> shims::Source {
    let source = shims::source(Path::new(env!("CARGO_BIN_EXE_mixengine-shim")))
        .unwrap_or_else(|error| panic!("{error}"));

    // `shims::source` falls back to the shim when there is no trampoline, which is right for an
    // updated install and wrong here: this suite would then test the layout T185 replaced.
    assert!(
        !cfg!(windows) || source.placed != source.resolver,
        "no mixengine-trampoline beside {} — run `cargo build -p mixengine-trampoline` first, or \
         `cargo test --workspace`",
        source.resolver.display()
    );

    source
}
