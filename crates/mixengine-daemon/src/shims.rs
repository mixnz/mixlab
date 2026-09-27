//! `<root>/bin` and this user's PATH — the only door into either. Roadmap task **T26**.
//!
//! Two mechanisms with one purpose, which is why they are one type: filling the directory makes
//! `php` a file, putting the directory on the PATH makes `php` a word somebody can type, and either
//! on its own does nothing a person would notice. `path.install` does both and `path.status`
//! reports both.
//!
//! **The two halves have opposite policies about being done without being asked**, and the split is
//! where the change lands.
//!
//! `<root>/bin` is inside the home, so it is refreshed on **every start**, beside the recovery
//! passes that reconcile services and jobs: it is a projection of a table in this binary, exactly as
//! `etc/` is a projection of the database, and a home whose `bin/` was deleted is repaired by
//! starting the daemon. Nothing outside the root is touched by it and there is nothing for a user to
//! consent to.
//!
//! The PATH is **only ever changed when somebody asks**. It is a file in the user's home on Unix and
//! a value in their registry hive on Windows — outside the root, and outside what
//! `docs/architecture/overview.md` lists as MixEngine's to write on its own account. A daemon
//! that edited `~/.zprofile` because it happened to start at login would be a program that changed
//! the shell of somebody who had only installed it.

use std::path::PathBuf;
use std::sync::Arc;

use mixengine_core::{Paths, shims};
use mixengine_platform::{Host, PathState};
use mixengine_proto::{
    CommandConflict, CommandOrigin, CommandSource, Error, PathPlace, PathReport,
};

use crate::error::ToWire as _;

/// What one walk of the installed rows decided `bin/` should hold beyond the compiled table.
///
/// Held as a value so that `path.install` composes one report out of one walk: the copy needs the
/// names, and the report needs the same names in order to say where each of them came from.
#[derive(Debug, Default)]
struct Found {
    /// The commands, with what put each there.
    extra: Vec<shims::Extra>,

    /// Names more than one installed package claimed.
    conflicts: Vec<shims::Conflict>,
}

/// Where each name in `bin/` came from, for the listing a person reads.
///
/// **A directory joined to a decision, and a name in one and not the other is information.** A
/// command with no origin is a copy left behind by something that has since been uninstalled — the
/// next refresh sweeps it, and until then `mix path status` is where it can be seen.
fn origins(commands: &[String], extra: &[shims::Extra]) -> Vec<CommandOrigin> {
    let same = |left: &str, right: &str| match cfg!(windows) {
        true => left.eq_ignore_ascii_case(right),
        false => left == right,
    };

    commands
        .iter()
        .filter_map(|command| {
            // The file name as `bin/` spells it; what was decided is spelled without the suffix.
            let stem = std::path::Path::new(command)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(command);

            let source = if shims::COMMANDS
                .iter()
                .any(|compiled| same(compiled.name, stem))
            {
                CommandSource::BuiltIn {}
            } else {
                match &extra.iter().find(|extra| same(&extra.name, stem))?.origin {
                    shims::Origin::Client { package } => CommandSource::Client {
                        package: package.clone(),
                    },
                    shims::Origin::Global { kind } => CommandSource::Global { kind: *kind },
                }
            };

            Some(CommandOrigin {
                command: command.clone(),
                source,
            })
        })
        .collect()
}

/// The home's `bin/`, the binary that fills it, and the machine whose PATH it goes on.
#[derive(Debug)]
pub(crate) struct Shims {
    /// `<root>/bin`.
    bin: PathBuf,

    /// The program that is running, which is what the shim binary is found beside.
    ///
    /// Held rather than the resolved shim, so that a broken installation is reported by the call
    /// that needs it rather than by refusing to start: a daemon with no `mixengine-shim` next to it
    /// can still supervise every service in this home, and saying so at `path.install` is where a
    /// person can act on it.
    program: PathBuf,

    /// The OS, for the half of this that is not a file inside the home.
    host: Arc<dyn Host>,

    /// The rows `bin/` is a projection of — roadmap tasks **T130** and **T131**.
    ///
    /// The directory stopped being a projection of one compiled constant when it started fronting
    /// the clients of installed packages and the tools somebody put inside a runtime, and this is
    /// what that cost: the thing that fills `bin/` now has to be able to read the database.
    store: mixengine_core::Store,

    /// Which packages this build knows how to run, which is what declares their client commands.
    catalogue: mixengine_core::generate::Catalogue,

    /// Held across every refresh, so two of them cannot sweep against two different expectations.
    ///
    /// **Three callers now instead of one**: the start, `path.install`, and the pass that notices a
    /// global install (T131). A refresh removes what is not expected *before* it writes what is, so
    /// two overlapping passes could each delete the other's files and leave a name on somebody's
    /// PATH with nothing behind it. The window is small and the failure is not, which is the shape
    /// of a lock that is worth taking.
    filling: tokio::sync::Mutex<()>,
}

impl Shims {
    pub(crate) fn new(
        paths: &Paths,
        program: PathBuf,
        host: Arc<dyn Host>,
        store: mixengine_core::Store,
        catalogue: mixengine_core::generate::Catalogue,
    ) -> Self {
        Self {
            bin: paths.bin().to_path_buf(),
            program,
            host,
            store,
            catalogue,
            filling: tokio::sync::Mutex::new(()),
        }
    }

    /// Put one copy of the shim in `bin/` per command, and clear out what is not one.
    ///
    /// Called at every start and by [`install`](Self::install). Touches nothing outside the home.
    ///
    /// Answers the **wire** error rather than the domain one, although its only other caller is the
    /// start-up path that logs it: everything below the API boundary in this binary has already
    /// been through [`ToWire`](crate::error::ToWire), and a second error type flowing up through
    /// one method would be one place for a hint to go missing.
    pub(crate) async fn refresh(&self) -> Result<shims::Refreshed, Error> {
        let found = self.extras().await?;

        self.fill(&found).await
    }

    /// The copy itself, given what a walk of the rows already found.
    ///
    /// Split out so that `path.install` composes one report from one walk rather than asking the
    /// database twice about a directory it has just written.
    async fn fill(&self, found: &Found) -> Result<shims::Refreshed, Error> {
        let shim = shims::source(&self.program).map_err(|error| error.to_wire())?;

        let _filling = self.filling.lock().await;

        let mut refreshed = shims::refresh(&self.bin, &shim, &shims::every_kind(), &found.extra)
            .map_err(|error| error.to_wire())?;
        refreshed.conflicts = found.conflicts.clone();

        Ok(refreshed)
    }

    /// The commands `bin/` fronts on installed packages' behalf — roadmap task **T130**.
    ///
    /// **A recipe declares, the rows decide.** Which names exist at all is
    /// [`Recipe::clients`](mixengine_core::generate::Recipe::clients), compiled in; which of them
    /// this home can actually run is three facts out of the database, asked once per package:
    ///
    /// - is any version of it installed, and which one would a command resolve to
    ///   ([`client::chosen`](mixengine_core::services::client::chosen)),
    /// - does *that* install publish the executable the client names — a Windows MariaDB packs no
    ///   `mariadb-backup` on every branch, and a name in `bin/` resolving to nothing is worse than
    ///   a missing one,
    /// - and is there an instance, on the product's documented port, which is what settles a name
    ///   two packages both want.
    ///
    /// A package that is not installed is skipped rather than reported: `bin/` holding `node` on a
    /// machine with no Node.js is [`shims::COMMANDS`]' deliberate choice and is right for a
    /// *runtime*, whose shim then says which command to type. It is wrong here, because there is no
    /// such sentence to say — `mysqldump` on a machine that has never had a database is a name
    /// nothing would ever make work.
    async fn extras(&self) -> Result<Found, Error> {
        // The whole walk is [`client::claims`], because the shim performs the same one with the
        // name it was invoked by: two implementations would be a `bin/` holding a command the shim
        // then refused, or a command the shim ran that this directory had given to another package.
        //
        // A database that cannot be read fails the whole refresh rather than composing `bin/` out
        // of half a query — the half that failed would be swept away as commands nothing claims.
        let claims = mixengine_core::services::client::claims(&self.store, &self.catalogue, None)
            .await
            .map_err(|error| error.to_wire())?;

        let (mut extra, conflicts) = shims::resolve_claims(&claims);

        // **And the tools somebody installed into a runtime** — roadmap task T131. Read from the
        // table rather than scanned here: a refresh happens on every start and on every
        // `path.install`, where a scan of every bindir is work with nothing to show for it, and
        // [`rescan`](Self::rescan) is what keeps the table current.
        let globals = mixengine_core::bin_commands::all(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        extra.extend(globals.into_iter().map(|(name, kind)| shims::Extra {
            name,
            origin: shims::Origin::Global { kind },
        }));

        Ok(Found { extra, conflicts })
    }

    /// Look for a tool somebody installed into a runtime, and fill `bin/` with what is found.
    ///
    /// **`path.rescan`, and the pass the daemon repeats** — roadmap task **T131**. Two steps, and
    /// the order is the one every projection in this codebase follows: find out what is on disk,
    /// write the table, *then* make the directory match the table. A `bin/` filled before the table
    /// was written would hold names the shim could not dispatch.
    ///
    /// Idempotent, and cheap when nothing changed: the scan is a `read_dir` per installed runtime,
    /// the record is one transaction, and `refresh` compares before it writes.
    pub(crate) async fn rescan(&self) -> Result<shims::Refreshed, Error> {
        let found = mixengine_core::runtimes::globals::everywhere(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        mixengine_core::bin_commands::record(&self.store, &found)
            .await
            .map_err(|error| error.to_wire())?;

        self.refresh().await
    }

    /// `path.status` — what a terminal opened a minute from now would find.
    ///
    /// Reads `bin/` rather than reporting [`COMMANDS`](mixengine_core::shims::COMMANDS): the
    /// question is what is *there*, and a listing composed from the table would answer it out of
    /// this binary's constants on a machine where the directory had been deleted.
    pub(crate) async fn status(&self) -> Result<PathReport, Error> {
        let state = self
            .host
            .path_integration()
            .state(&self.bin)
            .map_err(|error| error.to_wire())?;

        let found = self.extras().await?;

        Ok(self.report(state, self.installed(), Vec::new(), found))
    }

    /// `path.install` — fill `bin/`, then put it on the PATH.
    ///
    /// **That order and not the other**, because the failure that survives has to be the harmless
    /// one: a directory of shims nothing can find is invisible, and a PATH entry naming a directory
    /// that was never filled is a `php` that resolves to nothing.
    pub(crate) async fn install(&self) -> Result<PathReport, Error> {
        let found = self.extras().await?;
        let refreshed = self.fill(&found).await?;

        let state = self
            .host
            .path_integration()
            .add(&self.bin)
            .map_err(|error| error.to_wire())?;

        Ok(self.report(state, refreshed.commands, refreshed.refused, found))
    }

    /// `path.uninstall` — take `bin/` off the PATH, and leave it exactly as it is.
    ///
    /// The shims stay. They are inside the home, they cost a few megabytes there, and removing what
    /// makes the home work in order to undo one line in a profile would be an uninstall wearing a
    /// smaller command's name — `docs/architecture/overview.md` has removing the home remove
    /// them.
    pub(crate) async fn uninstall(&self) -> Result<PathReport, Error> {
        let state = self
            .host
            .path_integration()
            .remove(&self.bin)
            .map_err(|error| error.to_wire())?;

        let found = self.extras().await?;

        Ok(self.report(state, self.installed(), Vec::new(), found))
    }

    /// `path.rescan` — the pass, and then the report a person reads.
    pub(crate) async fn rescanned(&self) -> Result<PathReport, Error> {
        let refreshed = self.rescan().await?;

        let state = self
            .host
            .path_integration()
            .state(&self.bin)
            .map_err(|error| error.to_wire())?;

        let found = self.extras().await?;

        Ok(self.report(state, refreshed.commands, refreshed.refused, found))
    }

    /// The commands `bin/` answers to right now, read off the directory.
    ///
    /// Best effort, and the empty list is the honest answer for a directory that cannot be read: a
    /// status that invented the table's contents would say `php` is there on a machine where it is
    /// not, which is the one thing this method exists to be able to deny.
    fn installed(&self) -> Vec<String> {
        let known: Vec<String> = shims::COMMANDS.iter().map(shims::file_name).collect();

        let Ok(entries) = std::fs::read_dir(&self.bin) else {
            return Vec::new();
        };

        let mut present: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            // A copy a Windows refresh could not overwrite and moved out of the way. It is rubbish
            // the next sweep collects, not a command anybody can type.
            .filter(|name| !name.ends_with(shims::MOVED_ASIDE))
            // Where the Windows trampolines find the resolver (T185): MixEngine's own file, and
            // not a name anybody types.
            .filter(|name| !name.starts_with(mixengine_platform::handover::RESOLVER_POINTER))
            .collect();

        let same = |left: &str, right: &str| match cfg!(windows) {
            true => left.eq_ignore_ascii_case(right),
            false => left == right,
        };

        // In the table's order rather than the directory's, which is arbitrary on every filesystem
        // and stable on none — a listing somebody scans has to be one the eye can predict, which is
        // `runtime.list_installed`'s own reasoning.
        let mut listing: Vec<String> = known
            .into_iter()
            .filter(|name| present.iter().any(|found| same(found, name)))
            .collect();

        // **And then everything else `bin/` holds, alphabetically** — roadmap tasks T130 and T131.
        // A listing composed from [`shims::COMMANDS`] alone used to be the whole answer and is now
        // a claim that `mysqldump` and `yarn` are not there, on a machine where a person can see
        // them and run them.
        present.retain(|found| !listing.iter().any(|name| same(found, name)));
        present.sort();
        listing.extend(present);

        listing
    }

    /// The wire shape of an answer, from the OS's half and the directory's.
    fn report(
        &self,
        state: PathState,
        commands: Vec<String>,
        stale: Vec<String>,
        found: Found,
    ) -> PathReport {
        PathReport {
            directory: self.bin.display().to_string(),
            on_path: state.complete(),
            places: state
                .locations
                .into_iter()
                .map(|location| PathPlace {
                    name: location.name,
                    present: location.present,
                    changed: location.changed,
                })
                .collect(),
            origins: origins(&commands, &found.extra),
            conflicts: found
                .conflicts
                .into_iter()
                .map(|conflict| CommandConflict {
                    command: conflict.name,
                    won: conflict.won,
                    lost: conflict.lost,
                })
                .collect(),
            commands,
            stale,
        }
    }
}
