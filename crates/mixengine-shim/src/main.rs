//! `php`, `node`, `npm`, `python`, `ruby` — one binary, copied into `<root>/bin` under each of those
//! names, that works out which version this directory means and then gets out of its way.
//!
//! This is the program that makes `cd project-a && php -v` and `cd project-b && php -v` disagree
//! with no shell hook installed, which is Phase 2's milestone. Four steps, and the interesting thing
//! about each of them is what it does *not* do:
//!
//! ```text
//! 1  which command is this?        argv[0], against `core::shims::COMMANDS`
//! 2  which version does it mean?   `core::resolve`, in this process — no daemon, no IPC
//! 3  which file is that?           the `provides` map recorded when it was installed
//! 4  become it                     exec on Unix, a Job Object child on Windows — carrying `PATH`
//!                                  and the generated ini set (T28), and nothing else
//!
//! For `composer`, steps 2 and 3 run twice — once for the file, once for the PHP that runs it —
//! and step 4 starts the PHP with the file first (roadmap task T27c).
//! ```
//!
//! # It has no arguments of its own, and cannot have
//!
//! Every argument belongs to the program being fronted: a `--home` flag here would be one `php`
//! could never be given, and `php --version` has to reach PHP rather than print ours. So the only
//! input beside `argv[0]` is the environment — `MIXENGINE_HOME` for which install this is, and
//! `MIXENGINE_PHP` (per kind, [`RuntimeKind::override_env`]) for a version chosen for one command.
//! **That is also why there is no `--json`, no logging and no `--explain`**: anything this printed
//! on its own account would be a line in the middle of somebody's `php -r` output.
//!
//! # Why it links `mixengine-core` when `mix` deliberately does not
//!
//! `mix` avoids that edge because it can ask a daemon, and a bundled SQLite is a poor trade for
//! finding out where a socket lives. Here there is nothing to ask: the whole promise is that a
//! version resolves **without a daemon** — with one stopped, still starting, or never installed —
//! and in a budget (15 ms, T29) that a connection, a request and a response would spend before the
//! query even started. So the resolution is the same `core::resolve` the daemon and the GUI call,
//! run in this process against the database opened read-only.
//!
//! # What it is not allowed to do to the home
//!
//! Read it. [`Store::open_read_only`] does not create the file and does not migrate it, and SQLite
//! is what enforces that rather than our remembering: a schema upgrade decided by whichever `php -v`
//! ran first, possibly several at once from a build script, is the one thing `mixengine.db` cannot
//! afford. A home that has never had a daemon in it is an error here, not a home this creates.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use mixengine_core::config::PathOverrides;
use mixengine_core::{Paths, Store, paths, resolve, runtimes, shims};
use mixengine_platform::handover::{self, Handover};
use mixengine_platform::process;
use mixengine_proto::{PackageVersion, RuntimeKind, ServiceId, VersionConstraint};

/// What this exits with when it cannot become the program it was asked to be.
///
/// A shell's own word for it: 127 is "command not found", which is what every failure here amounts
/// to from the outside — the version is not installed, the home is not there, the artifact publishes
/// no such executable. Deliberately one code rather than a taxonomy: a script branching on *why* a
/// `php` did not run would be branching on MixEngine's internals, and the sentence on stderr is
/// where the reason belongs.
const NOT_RUNNABLE: i32 = 127;

fn main() {
    // `argv[0]` rather than `current_exe`, and the difference is the whole dispatch: a shim is this
    // binary under another name, so what has to be read is the name it was *invoked* by. On Unix
    // `current_exe` follows a symlink back to `mixengine-shim`, which would make every command in
    // `bin/` the same unknown one.
    //
    // **Unless a trampoline asked (T185).** On Windows `bin/php.exe` is `mixengine-trampoline`,
    // which runs this file under its own name and says in this variable which command it is.
    let invoked = match std::env::var_os(handover::SHIM_AS_ENV) {
        Some(name) => name,
        None => std::env::args_os().next().unwrap_or_default(),
    };
    let invoked = PathBuf::from(invoked);

    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();

    match run(&invoked, &arguments) {
        Ok(code) => std::process::exit(code),

        Err(refusal) => {
            // Named after the command the user typed rather than after MixEngine, the way every
            // other program on their PATH complains. `mix` is named in the hint instead, which is
            // where it is something to type rather than a brand.
            let called = called(&invoked);
            eprintln!("{called}: {}", refusal.said);

            if let Some(hint) = refusal.hint {
                eprintln!("{called}: {hint}");
            }

            std::process::exit(NOT_RUNNABLE);
        }
    }
}

/// Resolve, look up, and hand over.
///
/// Answers a status only on Windows, where the shim outlives the program it started; on Unix the
/// hand-over is an `exec` and the only way back here is the [`Refusal`].
fn run(invoked: &Path, arguments: &[OsString]) -> Result<i32, Refusal> {
    // **The compiled table first, and nothing new happens on this path** — roadmap task T29's
    // budget is fifteen milliseconds for `php -v`, and every arm added below is an arm a runtime
    // command never reaches.
    let Some(command) = shims::dispatch(invoked) else {
        return client(invoked, arguments);
    };

    let own = resolved(command.kind, command.executable)?;

    // **A file for another kind's program** — roadmap task **T27c**, its design's D4. The command's
    // kind named the file; the `via` kind names what runs it, resolved for the same directory and
    // under its own override variable, so `MIXENGINE_PHP=8.1 composer install` means what it says.
    // The environment is the program's: PHP's own directory on the PATH, PHP's generated ini set.
    let (program, root, kind, version, arguments, java) = match command.via {
        None => (
            own.program,
            own.root,
            command.kind,
            own.version,
            arguments.to_vec(),
            own.java,
        ),
        Some(via) => {
            let runner = resolved(via, via.as_str()).map_err(|refusal| Refusal {
                said: format!("no {via} to run {} with: {}", command.name, refusal.said),
                hint: refusal.hint,
            })?;

            let mut handed = Vec::with_capacity(arguments.len() + 1);
            handed.push(own.program.into_os_string());
            handed.extend(arguments.iter().cloned());

            (
                runner.program,
                runner.root,
                via,
                runner.version,
                handed,
                runner.java,
            )
        }
    };

    let environment = surroundings(kind, &program, &root, &version, java.as_deref());

    become_program(&program, &arguments, &environment)
}

/// Hand over, or — asked by a trampoline — say what would have been handed over (T185).
///
/// The trampoline runs this with none of the user's arguments, so `arguments` here is only what
/// this shim puts *before* them (`composer.phar`, T27c). The trampoline appends its own, which is
/// why they never make the round trip through a pipe and back.
fn become_program(
    program: &Path,
    arguments: &[OsString],
    environment: &BTreeMap<String, OsString>,
) -> Result<i32, Refusal> {
    if std::env::var_os(handover::SHIM_AS_ENV).is_none() {
        return process::hand_over(program, arguments, environment).map_err(|error| Refusal {
            said: explain(&error),
            hint: None,
        });
    }

    let record = Handover {
        program: program.to_path_buf(),
        args: arguments.to_vec(),
        env: environment.clone(),
    };

    std::io::Write::write_all(&mut std::io::stdout().lock(), &record.encode()).map_err(
        |error| Refusal {
            said: format!("cannot hand the resolution back to the trampoline: {error}"),
            hint: None,
        },
    )?;

    Ok(0)
}

/// A client of an installed service package — roadmap task **T130**.
///
/// `mysqldump`, `psql`, `redis-cli`: not a runtime, so there is no directory to resolve against and
/// no default version to fall back on. What decides is the **instance** —
/// [`services::client`](mixengine_core::services::client) has the order — and the instance decides
/// two things at once: which install the program comes out of, and where it connects.
///
/// The claim is settled by the same [`shims::resolve_claims`] the daemon filled `bin/` with, over
/// the same rows, which is what stops a terminal and a directory listing from disagreeing about
/// whose `mysql` this is.
fn client(invoked: &Path, arguments: &[OsString]) -> Result<i32, Refusal> {
    let name = called(invoked);

    // The binary run under its own name, before anything copied it into `bin/` — a development
    // tree, or somebody who found it in an install directory. It is the one name that is *never*
    // a command, so it is answered without opening a database.
    if name == shims::BINARY {
        return Err(unknown_command());
    }

    let home = home_override().map(PathBuf::from);
    let root = paths::resolve_root_default(home.as_deref()).map_err(|error| Refusal {
        said: explain(&error),
        hint: None,
    })?;

    let database = Paths::new(root.clone(), &PathOverrides::default())
        .database_file()
        .to_path_buf();

    let catalogue = mixengine_core::generate::Catalogue::builtin();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|source| Refusal {
            said: format!("cannot start: {source}"),
            hint: None,
        })?;

    let (program, environment) = runtime.block_on(async {
        let store = Store::open_read_only(&database)
            .await
            .map_err(|error| Refusal {
                said: explain(&error),
                hint: Some(format!(
                    "{} is where this shim looks — set MIXENGINE_HOME if that is not the install \
                     it belongs to",
                    database.display()
                )),
            })?;

        // Narrowed to the recipes that declare this name, so an ordinary `psql` costs one query
        // rather than one per package in the catalogue.
        let claims = mixengine_core::services::client::claims(&store, &catalogue, Some(&name))
            .await
            .map_err(|error| Refusal {
                said: explain(&error),
                hint: None,
            })?;

        let (extras, _) = shims::resolve_claims(&claims);

        let package = extras.into_iter().find_map(|extra| match extra.origin {
            shims::Origin::Client { package } => Some(package),
            shims::Origin::Global { .. } => None,
        });

        // **No package claims it, so the third arm** — roadmap task T131. A tool somebody installed
        // into a runtime, whose language `bin_commands` recorded when the pass that found it wrote
        // `bin/`. One row, because a shim is handed nothing but the name it was invoked by.
        let Some(package) = package else {
            return global(&store, &name, &root).await;
        };

        let recipe = catalogue.recipe(&package).expect("a package that claimed");

        let executable =
            mixengine_core::services::client::executable_for(&catalogue, &package, &name)
                .expect("the claim named this command");

        // The instance this command was told to use, if somebody said. Read here rather than
        // deeper in for `override_version`'s reason: the process that reads it has to be the one
        // the user invoked.
        let asked = asked_instance(&package)?;

        let chosen = mixengine_core::services::client::chosen(
            &store,
            &package,
            recipe.preferred_port(),
            asked.as_ref(),
        )
        .await
        .map_err(|error| Refusal {
            said: explain(&error),
            hint: None,
        })?;

        let program = chosen
            .program(&package, executable)
            .map_err(|error| Refusal {
                said: explain(&error),
                hint: None,
            })?;

        let mut environment = BTreeMap::new();

        // The install's own directory ahead of the PATH, for the runtime commands' reason: a
        // Windows `mariadb.exe` finds its DLLs beside it, and the Cygwin Redis finds `cygwin1.dll`.
        if let Some(directory) = program.parent() {
            environment.insert("PATH".to_owned(), ahead_of_the_path(directory));
        }

        // **And where its own instance listens** — the design's D4. Only where the person has not
        // said: somebody who exported `MYSQL_TCP_PORT` for a tunnel meant it, and a tool that
        // overrode it would be one that cannot be used against anything but itself.
        if let Some(listen) = &chosen.listen {
            for (variable, value) in recipe.client_env(listen) {
                if std::env::var_os(variable).is_none() {
                    environment.insert(variable.to_owned(), OsString::from(value));
                }
            }
        }

        Ok::<_, Refusal>((program, environment))
    })?;

    become_program(&program, arguments, &environment)
}

/// A tool somebody installed into a runtime — roadmap task **T131**.
///
/// **It follows the version the way `npm` does**, which is the whole of the design: `yarn` is
/// resolved for *this* directory, and the file is looked for inside that version's own bindir. A
/// `bin/yarn` that ran whichever copy it found first would be the silent wrong answer the shim
/// exists to prevent — a project pinned to Node 22 getting Node 24's Yarn.
///
/// A version that does not have the tool is a sentence and not a bare 127: the person is told which
/// version this directory means and what to type to install it there.
async fn global(
    store: &Store,
    name: &str,
    root: &Path,
) -> Result<(PathBuf, BTreeMap<String, OsString>), Refusal> {
    let kind = mixengine_core::bin_commands::kind(store, name)
        .await
        .map_err(|error| Refusal {
            said: explain(&error),
            hint: None,
        })?
        .ok_or_else(|| nothing_answers_to(name))?;

    let asked = override_version(kind)?;
    let cwd = std::env::current_dir().ok();

    let resolved = resolve::runtime(
        store,
        &resolve::Question {
            kind,
            cwd: cwd.as_deref(),
            explicit: asked.as_ref(),
        },
    )
    .await
    .map_err(|error| Refusal {
        hint: hint_for(&error),
        said: explain(&error),
    })?;

    let install = PathBuf::from(&resolved.runtime.path);
    let version = resolved.runtime.version.clone();

    let bindir = runtimes::globals::directory(kind, &install).ok_or_else(|| Refusal {
        said: format!("{kind} installs no tools of its own that a command could front"),
        hint: None,
    })?;

    let program = runnable(&bindir, name).ok_or_else(|| Refusal {
        said: format!(
            "{kind} {version} is what this directory resolves to, and {name} is not installed \
             for it"
        ),
        hint: Some(install_globally(kind, name)),
    })?;

    Ok((
        program,
        surroundings(
            kind,
            &program_for_surroundings(&bindir),
            root,
            &version,
            None,
        ),
    ))
}

/// The file in `bindir` that this system would run for a bare `name`.
///
/// On Windows that is the name plus one of the loader's extensions, in the order the loader tries
/// them — an `.exe` before a `.cmd`, which matters because a package that ships both means the
/// `.exe`, and because a batch file is the one thing whose arguments this cannot always quote.
fn runnable(bindir: &Path, name: &str) -> Option<PathBuf> {
    if !cfg!(windows) {
        let file = bindir.join(name);
        return file.is_file().then_some(file);
    }

    ["exe", "com", "bat", "cmd"]
        .into_iter()
        .map(|extension| bindir.join(format!("{name}.{extension}")))
        .find(|file| file.is_file())
}

/// A path inside `bindir`, for [`surroundings`] to take the parent of.
///
/// `surroundings` describes a program by the directory it lives in, and what has to go on the PATH
/// here is the *bindir* — which on Unix is `<install>/bin`, where `node` itself is, so a `yarn`
/// started from it finds the interpreter that is meant to run it.
fn program_for_surroundings(bindir: &Path) -> PathBuf {
    bindir.join("the-program")
}

/// What to type to put `name` inside the version this directory resolves to.
fn install_globally(kind: RuntimeKind, name: &str) -> String {
    match kind {
        RuntimeKind::Node => format!("npm install -g {name}"),
        RuntimeKind::Python => format!("pip install {name}"),
        RuntimeKind::Ruby => format!("gem install {name}"),
        RuntimeKind::Php | RuntimeKind::Go | RuntimeKind::Java | RuntimeKind::Composer => {
            format!("nothing here installs {name} into a {kind}")
        }
    }
}

/// `MIXENGINE_MARIADB`, if it says anything.
///
/// An empty value is "not set"; anything else that is not a service id is refused rather than
/// skipped past, on [`override_version`]'s own reasoning.
fn asked_instance(package: &str) -> Result<Option<ServiceId>, Refusal> {
    let name = mixengine_core::services::client::override_env(package);

    let Some(value) = std::env::var_os(&name) else {
        return Ok(None);
    };

    let Some(value) = value.to_str().map(str::trim) else {
        return Err(Refusal {
            said: format!("{name} is not text this can read as a service"),
            hint: None,
        });
    };

    if value.is_empty() {
        return Ok(None);
    }

    ServiceId::parse(value).map(Some).map_err(|error| Refusal {
        said: format!("{name} is set to something that is not a service: {error}"),
        hint: Some(format!(
            "an instance of {package}, such as {name}={package}@main"
        )),
    })
}

/// What step three answered: the file to run, and the two things step four needs to describe it.
struct Resolution {
    /// The executable inside the runtime's own directory.
    program: PathBuf,

    /// `MIXENGINE_HOME`, so the generated ini set can be found without resolving the root twice.
    root: PathBuf,

    /// Which version this directory meant, which is what names the ini set.
    version: PackageVersion,

    /// `provides.java` of the same install, for a Java command — what `JAVA_HOME` is derived from
    /// (roadmap task **T27e**, its design's D5). [`None`] for every other kind.
    java: Option<PathBuf>,
}

/// Everything the fronted program is given beside its own arguments.
///
/// `PATH` is what makes a runtime's own tools reach each other, and `PHP_INI_SCAN_DIR` is the
/// generated ini set the pool also reads — the whole point of it being here is that `php -m` in a
/// terminal and `phpinfo()` in a browser answer the same thing. Beside those two, [`trusting`] names
/// the trust bundle a language reads, [`toolchain`] keeps a Go to the release it resolved to, and
/// [`java_home`] names a JDK's own home.
///
/// **Keyed off the directory existing rather than off the command being `php`**:
/// [`runtimes::extensions`] renders nothing for a runtime whose artifact declares no extension
/// directory, and a variable pointing at a directory nothing writes is worse than no variable.
fn surroundings(
    kind: RuntimeKind,
    program: &Path,
    root: &Path,
    version: &PackageVersion,
    java: Option<&Path>,
) -> BTreeMap<String, OsString> {
    let mut environment = BTreeMap::new();

    // The directory the program lives in, ahead of everything already on the path. It is what makes
    // a runtime's own tools reach each other: `php-config` invoked by an extension build, `node`
    // invoked by `npm`, `gem` invoked by `bundle`. Prepended rather than replacing, because the rest
    // of the PATH is the user's session and a shim is standing in the middle of it.
    if let Some(directory) = program.parent() {
        environment.insert("PATH".to_owned(), ahead_of_the_path(directory));
    }

    // `PathOverrides::default()` for `resolved`'s reason: a shim does not read `config.toml`.
    let paths = Paths::new(root.to_path_buf(), &PathOverrides::default());
    let conf_d = runtimes::extensions::conf_d(paths.etc(), kind, version.as_str());

    if conf_d.is_dir() {
        environment.insert(
            runtimes::extensions::SCAN_DIR_ENV.to_owned(),
            conf_d.into_os_string(),
        );
    }

    trusting(kind, &paths, &mut environment);
    toolchain(
        kind,
        std::env::var_os(GOTOOLCHAIN).as_deref(),
        &mut environment,
    );
    java_home(java, &mut environment);

    environment
}

/// The variable Maven, Gradle and a JVM's own children read to find a JDK.
const JAVA_HOME: &str = "JAVA_HOME";

/// Name the JDK a Java command resolved to — roadmap task **T27e**, its design's D5.
///
/// **Always, over a value the session carries.** That is the opposite of [`toolchain`]'s rule, and
/// for T27d's `go env -w` reason: `JAVA_HOME` is usually a machine-wide setting an installer wrote
/// before MixEngine was here. A shim reaches only what it starts, so `mvn` typed in a terminal still
/// reads the session's value — `mix doctor` says so.
fn java_home(java: Option<&Path>, environment: &mut BTreeMap<String, OsString>) {
    let Some(home) = java.and_then(mixengine_core::runtimes::java::home) else {
        return;
    };

    environment.insert(JAVA_HOME.to_owned(), home.into_os_string());
}

/// The variable that decides whether `go` may run a toolchain other than itself.
const GOTOOLCHAIN: &str = "GOTOOLCHAIN";

/// Keep a Go to the release this directory resolved to — roadmap task **T27d**, its design's D5.
///
/// **The archive's `go.env` says `GOTOOLCHAIN=auto`**, kept byte for byte by the packaging
/// repository. With `auto`, a `go` meeting a `go.mod` that asks for a newer release downloads that
/// release into the module cache and runs it instead — so a project pinned to 1.25 would build with
/// whatever its `go.mod` names, silently. `local` makes that a sentence from Go naming the setting.
///
/// Three rules, and the third is the one that differs from [`trusting`]:
///
/// - **A non-empty value in the session is the person's**, left exactly as it arrived.
/// - **An empty one is unset**, because that is how Go reads it: it would fall through to `go.env`.
/// - **A value written with `go env -w` is overridden**, on purpose. Go reads the environment before
///   the user's `go env` file, so this variable wins; and that file is usually a machine-wide
///   setting from before MixEngine was installed, which is exactly the machine where honouring it
///   would make a pin mean nothing.
///
/// `session` is the invoking process's own value, passed in rather than read here so the rules can
/// be asserted without changing the environment of the test process.
fn toolchain(
    kind: RuntimeKind,
    session: Option<&OsStr>,
    environment: &mut BTreeMap<String, OsString>,
) {
    if kind != RuntimeKind::Go || session.is_some_and(|value| !value.is_empty()) {
        return;
    }

    environment.insert(GOTOOLCHAIN.to_owned(), OsString::from("local"));
}

/// Tell this runtime about the authority MixEngine's own sites are signed by — task **T132**.
///
/// **One mechanism per language, and the difference between them is the whole design.**
/// `NODE_EXTRA_CA_CERTS` *adds* to what Node already trusts, so Node is handed the authority itself
/// and keeps its own curated set. Every other variable here **replaces** a trust store, so those
/// runtimes are handed the merged bundle — this machine's roots and then ours — because a file
/// holding one certificate would make `pip install` the first thing to stop working.
///
/// | kind | variable | file |
/// | --- | --- | --- |
/// | Node | `NODE_EXTRA_CA_CERTS` | `certs/ca/root.crt` |
/// | Python | `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE` | `etc/ca/bundle.pem` |
/// | Ruby | `SSL_CERT_FILE` | `etc/ca/bundle.pem` |
/// | PHP, Composer | — | the generated ini set says it instead |
/// | Go | — | it reads the operating system's store already |
/// | Java | — | its own `cacerts`, written by the daemon (T27e, ADR 0039) |
///
/// Go is absent because it needs nothing (roadmap task **T27d**, its design's D7): it verifies
/// through the operating system on Windows and macOS, and on Linux it reads the system bundle that
/// installing this home's authority regenerates.
///
/// PHP is absent on purpose. Its answer is `openssl.cafile` and `curl.cainfo` in the `conf.d` set
/// above, which the **pool** reads too — so `php -r` in a terminal and `curl_exec()` in a browser
/// get the same answer, which is the property T28 exists to hold. Composer runs through a PHP and
/// inherits it.
///
/// Two rules, both of them [`surroundings`]' own:
///
/// - **Only a file that exists is named.** A variable pointing at nothing is worse than no
///   variable, which is why `PHP_INI_SCAN_DIR` is behind an `is_dir` above. A home whose daemon has
///   never run has no bundle, and says nothing.
/// - **A value the person set is theirs.** Somebody who exported `SSL_CERT_FILE` for a corporate
///   authority meant it, and a tool that overrode it would be one that cannot be used inside the
///   company that installed it. `mix doctor` reports the shadowing instead.
fn trusting(kind: RuntimeKind, paths: &Paths, environment: &mut BTreeMap<String, OsString>) {
    let bundle = mixengine_core::generate::ca::path(paths.etc());
    let authority = mixengine_core::certs::ca::certificate_path(paths.certs());

    let named: &[(&str, &Path)] = match kind {
        RuntimeKind::Node => &[("NODE_EXTRA_CA_CERTS", &authority)],
        RuntimeKind::Python => &[("SSL_CERT_FILE", &bundle), ("REQUESTS_CA_BUNDLE", &bundle)],
        RuntimeKind::Ruby => &[("SSL_CERT_FILE", &bundle)],
        RuntimeKind::Php | RuntimeKind::Go | RuntimeKind::Java | RuntimeKind::Composer => &[],
    };

    for (variable, file) in named {
        if std::env::var_os(variable).is_some() || !file.is_file() {
            continue;
        }

        environment.insert((*variable).to_owned(), file.as_os_str().to_owned());
    }
}

/// Steps two and three: which version this directory means, and which file that is.
///
/// A `tokio` runtime of its own rather than `#[tokio::main]`, and dropped before the hand-over: what
/// follows is an `exec` on one system and a wait on the other, and neither wants a reactor thread
/// still standing behind it.
fn resolved(kind: RuntimeKind, executable: &str) -> Result<Resolution, Refusal> {
    let home = home_override().map(PathBuf::from);
    let root = paths::resolve_root_default(home.as_deref()).map_err(|error| Refusal {
        said: explain(&error),
        hint: None,
    })?;

    // `[paths]` cannot move the database — `Paths` passes `None` for it deliberately — so the
    // defaults are enough to name the one file this reads, and `config.toml` is not opened at all.
    // A shim that parsed the user's configuration would be a shim that fails when it has a typo in
    // it, on every command they run.
    let database = Paths::new(root.clone(), &PathOverrides::default())
        .database_file()
        .to_path_buf();

    let asked = override_version(kind)?;

    // A directory that has been deleted out from under this process is `None` rather than a refusal:
    // there is nothing to walk, which is exactly what the default is for. It is also the one shape
    // `resolve` already has a meaning for.
    let cwd = std::env::current_dir().ok();

    let runtime = tokio::runtime::Builder::new_current_thread()
        // Time and not `enable_all`, and the timer is not optional: `sqlx` panics outright without
        // one, because the busy timeout and the pool's own acquire deadline are both timers. What
        // is left out is the I/O driver, which would be an epoll or a completion port registered
        // for a database that is a file.
        .enable_time()
        .build()
        .map_err(|source| Refusal {
            said: format!("cannot start: {source}"),
            hint: None,
        })?;

    runtime.block_on(async move {
        let store = Store::open_read_only(&database)
            .await
            .map_err(|error| Refusal {
                said: explain(&error),
                hint: Some(format!(
                    "{} is where this shim looks — set MIXENGINE_HOME if that is not the install \
                     it belongs to",
                    database.display()
                )),
            })?;

        let resolved = resolve::runtime(
            &store,
            &resolve::Question {
                kind,
                cwd: cwd.as_deref(),
                explicit: asked.as_ref(),
            },
        )
        .await
        .map_err(|error| Refusal {
            hint: hint_for(&error),
            said: explain(&error),
        })?;

        let program = runtimes::program(&store, kind, &resolved.runtime.version, executable)
            .await
            .map_err(|error| Refusal {
                said: explain(&error),
                hint: None,
            })?;

        let java = match kind {
            RuntimeKind::Java => Some(
                runtimes::program(&store, kind, &resolved.runtime.version, "java")
                    .await
                    .map_err(|error| Refusal {
                        said: explain(&error),
                        hint: None,
                    })?,
            ),
            _ => None,
        };

        Ok(Resolution {
            program,
            root,
            version: resolved.runtime.version,
            java,
        })
    })
}

/// `MIXENGINE_HOME`, if it says anything.
///
/// Read here rather than deeper in, per the standards' rule that configuration enters at `main` — and
/// an empty value is passed on rather than treated as absent, because `paths::resolve_root` is what
/// refuses it. A variable somebody meant to point somewhere must not silently become the default.
fn home_override() -> Option<OsString> {
    std::env::var_os("MIXENGINE_HOME")
}

/// The version this one command was told to use, from the kind's own environment variable.
///
/// `MIXENGINE_PHP=8.1 php -v` is step one of the resolution order, and it is read *here* for the
/// reason [`RuntimeKind::override_env`] gives: the process that reads it has to be the one the user
/// invoked. An empty value is "not set"; anything else that is not a constraint is refused rather
/// than skipped past, because a variable that quietly does nothing is the exact confusion this is
/// meant to end.
fn override_version(kind: RuntimeKind) -> Result<Option<VersionConstraint>, Refusal> {
    let name = kind.override_env();

    let Some(value) = std::env::var_os(name) else {
        return Ok(None);
    };

    let Some(value) = value.to_str().map(str::trim) else {
        return Err(Refusal {
            said: format!("{name} is not text this can read as a version"),
            hint: None,
        });
    };

    if value.is_empty() {
        return Ok(None);
    }

    VersionConstraint::parse(value)
        .map(Some)
        .map_err(|error| Refusal {
            said: format!("{name} is set to something that is not a version: {error}"),
            hint: Some(format!(
                "a version ({}=8.3.33), a series ({name}=8.3) or a range ({name}=^8.3)",
                name
            )),
        })
}

/// `directory`, then everything that was already on `PATH`.
///
/// `join_paths` rather than a separator of our own: the character differs by platform, and a
/// `#[cfg]` for it in a client is the one thing `docs/standards/rust.md` will not have. A `PATH`
/// that cannot be rebuilt — an entry containing the separator itself, which Windows allows inside
/// quotes — leaves the directory on its own rather than failing the command: the program still runs
/// and still finds its siblings, which is what the entry was for.
fn ahead_of_the_path(directory: &Path) -> OsString {
    let existing = std::env::var_os("PATH").unwrap_or_default();

    let entries = std::iter::once(directory.as_os_str().to_owned())
        .chain(std::env::split_paths(&existing).map(PathBuf::into_os_string));

    std::env::join_paths(entries).unwrap_or_else(|_| directory.as_os_str().to_owned())
}

/// What to call this program in a message.
///
/// The name it was invoked by, which is the one the user typed — `php`, not `mixengine-shim`, and
/// not the path it was found at.
fn called(invoked: &Path) -> String {
    invoked
        .file_stem()
        .unwrap_or(OsStr::new("mixengine-shim"))
        .to_string_lossy()
        .into_owned()
}

/// A reason this could not become the program it was asked to be, and what to do about it.
///
/// Two strings rather than an error enum: nothing branches on these — the exit code is
/// [`NOT_RUNNABLE`] either way — and what a shim owes its user is one sentence and, where there is
/// one, the command that would fix it.
struct Refusal {
    /// What went wrong, as a sentence.
    said: String,

    /// What to type about it.
    hint: Option<String>,
}

/// Being invoked under a name this build does not front.
///
/// Reached two ways: the binary run directly, before it has been copied into `bin/` under a name
/// that means something, and a leftover copy in a `bin/` from a build that fronted more commands
/// than this one does.
fn unknown_command() -> Refusal {
    let names: Vec<&str> = shims::COMMANDS.iter().map(|command| command.name).collect();

    Refusal {
        said: "this is a MixEngine shim and is not meant to be run under this name".to_owned(),
        hint: Some(format!("it answers to: {}", names.join(", "))),
    }
}

/// A name `bin/` holds and nothing in this home claims any more — roadmap tasks **T130** and
/// **T131**.
///
/// [`unknown_command`]'s sibling and a different sentence, because the situation is different and
/// the old one reads as a bug. `bin/` is a projection of installed state now, so a `mysqldump`
/// whose database was uninstalled a moment ago is an ordinary, momentary state of the world — the
/// next refresh sweeps it — and answering it with "this is a MixEngine shim" plus nineteen runtime
/// commands tells a person nothing about the one they typed.
fn nothing_answers_to(name: &str) -> Refusal {
    Refusal {
        said: "nothing installed here answers to this command any more".to_owned(),
        hint: Some(format!(
            "`mix path status` lists what bin/ holds, and `mix path rescan` brings it up to date \
             (this shim was left behind under the name {name})"
        )),
    }
}

/// One line out of an error and everything that caused it.
///
/// The chain is walked because these types keep the interesting half in the `#[source]` — "cannot
/// open the database at …" is the sentence, and "no such file" is the reason — and a shim has one
/// line to say both in.
fn explain(error: &dyn std::error::Error) -> String {
    let mut said = error.to_string();
    let mut cause = error.source();

    while let Some(next) = cause {
        said.push_str(": ");
        said.push_str(&next.to_string());
        cause = next.source();
    }

    said
}

/// The command that would make a failed resolution succeed, where there is one.
///
/// The same sentence the daemon puts in the `dependency_missing` hint, from the same function, so
/// that a version missing in a terminal and a version missing in the GUI tell the user to type the
/// same thing.
fn hint_for(error: &mixengine_core::Error) -> Option<String> {
    match error {
        mixengine_core::Error::RuntimeUnresolved {
            kind, constraint, ..
        } => Some(resolve::install_command(*kind, constraint)),

        mixengine_core::Error::NoDefaultRuntime { kind } => Some(format!(
            "`mix runtime list --kind {kind}` shows what is installed, and \
             `mix runtime default {kind} <version>` chooses which one is used here"
        )),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T25 left this note: "No `PHPRC`, no `GEM_HOME` — the rest are files T28's `conf.d` model
    /// generates, and a variable pointing at a file nothing writes is worse than no variable."
    /// Something writes them now.
    #[test]
    fn a_php_shim_is_told_where_its_ini_set_is() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let root = home.path();
        let version = PackageVersion::parse("8.3.33").expect("a version");

        let command = shims::COMMANDS
            .iter()
            .find(|command| command.name == "php")
            .expect("this build fronts php");

        let conf_d = mixengine_core::runtimes::extensions::conf_d(
            &root.join("etc"),
            command.kind,
            version.as_str(),
        );
        std::fs::create_dir_all(&conf_d).expect("a generated set");

        let environment = surroundings(
            command.kind,
            &root.join("runtimes/php/8.3.33/bin/php"),
            root,
            &version,
            None,
        );

        assert!(
            environment.contains_key("PATH"),
            "the runtime's own tools still reach each other"
        );
        let scan = environment
            .get(mixengine_core::runtimes::extensions::SCAN_DIR_ENV)
            .expect("a php that is told where its extensions are");
        assert!(
            scan.to_string_lossy().contains("8.3.33"),
            "the shim is pointing at another version's set: {scan:?}"
        );
    }

    /// A runtime with no generated set gets no variable, rather than one pointing at nothing.
    #[test]
    fn a_runtime_with_no_generated_set_is_told_nothing() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let version = PackageVersion::parse("20.11.0").expect("a version");

        let command = shims::COMMANDS
            .iter()
            .find(|command| command.name == "node")
            .expect("this build fronts node");

        let environment = surroundings(
            command.kind,
            &home.path().join("runtimes/node/20.11.0/bin/node"),
            home.path(),
            &version,
            None,
        );

        assert!(!environment.contains_key(JAVA_HOME));

        assert!(!environment.contains_key(mixengine_core::runtimes::extensions::SCAN_DIR_ENV));
    }

    /// **The pin means the release that was resolved** — roadmap task **T27d**, its design's D5.
    /// `go.env` inside the archive says `auto`, which would let a `go.mod` swap the toolchain.
    #[test]
    fn a_go_is_kept_to_the_toolchain_it_resolved_to() {
        let mut environment = BTreeMap::new();

        toolchain(RuntimeKind::Go, None, &mut environment);

        assert_eq!(environment.get(GOTOOLCHAIN), Some(&OsString::from("local")));
    }

    /// Go reads an empty variable as unset and falls through to `go.env`, so this does too.
    #[test]
    fn an_empty_session_value_is_unset_to_go_and_so_to_this() {
        let mut environment = BTreeMap::new();

        toolchain(RuntimeKind::Go, Some(OsStr::new("")), &mut environment);

        assert_eq!(environment.get(GOTOOLCHAIN), Some(&OsString::from("local")));
    }

    /// **A value the person set is theirs** — ADR 0034's rule, for one more variable.
    #[test]
    fn a_session_value_is_left_to_the_session() {
        let mut environment = BTreeMap::new();

        toolchain(
            RuntimeKind::Go,
            Some(OsStr::new("go1.27.1+auto")),
            &mut environment,
        );

        assert_eq!(
            environment.get(GOTOOLCHAIN),
            None,
            "inherited, not overwritten"
        );
    }

    #[test]
    fn no_other_kind_is_told_about_a_go_toolchain() {
        for kind in RuntimeKind::ALL
            .into_iter()
            .filter(|kind| *kind != RuntimeKind::Go)
        {
            let mut environment = BTreeMap::new();

            toolchain(kind, None, &mut environment);

            assert!(environment.is_empty(), "{kind}");
        }
    }

    /// **A Java command names its own JDK**, over whatever the session said — T27e, D5.
    #[test]
    fn a_java_command_is_handed_its_own_java_home() {
        let mut environment = BTreeMap::new();
        let root = Path::new("runtimes").join("java").join("21.0.12.1");

        java_home(Some(&root.join("bin").join("java")), &mut environment);

        assert_eq!(environment.get(JAVA_HOME), Some(&root.into_os_string()));
    }

    #[test]
    fn a_command_with_no_jdk_is_told_nothing_about_one() {
        let mut environment = BTreeMap::new();

        java_home(None, &mut environment);

        assert!(environment.is_empty());
    }
}
