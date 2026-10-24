//! Where a library error becomes the one a client sees.
//!
//! Every crate below keeps its own `thiserror` enum, shaped for the code that raises it and knowing
//! nothing about codes, hints or the wire. The translation happens here, once, at the boundary —
//! `docs/standards/rust.md`. Three things happen in it, and none of them belong anywhere else:
//!
//! - **The chain is flattened.** A client is handed one string and has no `source()` to walk, so
//!   every cause is folded into the message before it leaves. That part is
//!   [`mixengine_proto::flatten`], because `mix` maps the handful of failures it can meet without a
//!   daemon and has to produce the same shape of message.
//! - **A code is chosen.** That is the part a program branches on, so the choice is made by
//!   somebody who can see both sides — the library variant and the published vocabulary.
//! - **A hint is written**, where the daemon knows something the library did not. The library knows
//!   that `create` returned `EACCES`; only this layer knows that MixEngine never runs as an
//!   administrator and that `[paths]` exists. Where the library message already names the way out,
//!   the hint stays `None` rather than saying it a second time in the GUI.

use std::io;
use std::path::Path;

use mixengine_core::{
    BlueprintError, ElevationError, ExtensionError, InstallError, JobError, ProjectError,
    RuntimeError, ServiceError, UpdateError,
};
use mixengine_proto::{Error, ErrorCode};

/// What a row this build cannot decode is told — a runtime's, a package's, a project's or a site's.
const UNREADABLE_ROW: &str =
    "the row was written by a different version of MixEngine, or edited by hand";

/// A library error, rendered for the wire.
///
/// A trait rather than `From`, because both types are foreign here and the orphan rule says so —
/// which is no loss: `to_wire` at a call site names what is happening better than `.into()` would.
pub(crate) trait ToWire {
    /// Translate into the error a client sees.
    fn to_wire(&self) -> Error;
}

impl ToWire for mixengine_core::Error {
    fn to_wire(&self) -> Error {
        use mixengine_core::{Error as Core, services::GraphError};

        match self {
            // `kind` is the RPC namespace the entity belongs to (`site`, `runtime`, …), which is
            // also the noun `mix` uses, so the hint can name the command that would have listed it.
            Core::NotFound { kind, .. } => Error::new(ErrorCode::NotFound, chain(self))
                .with_hint(format!("`mix {kind} list` shows what does exist")),

            Core::Io { path, source, .. } => io_failure(chain(self), path, source),

            Core::Config { path, .. } => Error::new(ErrorCode::InvalidArgument, chain(self))
                .with_hint(format!(
                    "fix {}, or delete it and MixEngine will write a fresh one with the defaults",
                    path.display()
                )),

            // Every way this fails is a file that could not be used as a database, so `io` is what
            // a client branches on — and the reason is nearly always the directory rather than the
            // file, which is what the hint says.
            Core::Database { path, .. } => {
                Error::new(ErrorCode::Io, chain(self)).with_hint(format!(
                    "{} is opened by `mixengined` and by nothing else; a home directory that has \
                     been moved, emptied or copied from another account is the usual reason it \
                     cannot be",
                    path.display()
                ))
            }

            // The hint has to say what did *not* happen, because the database being untouched is
            // the whole point of stopping here.
            Core::Backup { path, .. } => Error::new(ErrorCode::Io, chain(self)).with_hint(format!(
                "the upgrade stopped rather than migrate a database it could not copy first; \
                 make room for {} and start MixEngine again",
                path.display()
            )),

            // Our SQL, not the user's file: `internal` is the honest name for it. The hint is
            // worth writing anyway, and it is true because each migration runs in a transaction —
            // the one that failed rolled back, so the previous release still opens this database.
            Core::Migration { .. } => Error::new(ErrorCode::Internal, chain(self)).with_hint(
                "this is a bug in this release of MixEngine; the database was left as it was, \
                 and the version you upgraded from still runs against it",
            ),

            // Not a bug, which is why it is not `internal`: the file was written by a build whose
            // migrations are not this one's, and it has been left exactly as it was found.
            //
            // **This hint used to name the `.bak-…` copy as *the* way back, and that is sometimes
            // false.** A backup is taken only when the schema is behind, so which file sits there
            // depends on how the database got ahead of this build: a home written by an older
            // MixEngine gets a copy taken by this same run, moments earlier, of this same
            // unreadable file — measured on one from before v0.0.1-beta.1, where copy and original
            // agreed on every checksum — while a home a *newer* build migrated has a copy from
            // before that migration, which does open.
            //
            // So the three are listed as actions in the order that keeps the most, and none of
            // them is promised: whether the copy helps depends on which of those two happened, and
            // that is not something this arm can tell. What it must not do is what it did before —
            // name one of the three as *the* way back.
            //
            // The hint has to carry the way out itself, because nothing else can: the daemon is
            // not listening, and every `mix` command goes through the daemon.
            Core::IncompatibleDatabase { path, .. } => {
                Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(format!(
                    // The path once and never three times: it is absolute, and a home under a
                    // temporary directory or a relocated profile runs to a hundred characters —
                    // repeated for each of the three actions it buries them.
                    "nothing was changed; run the MixEngine that wrote it, or replace {} with the \
                     `.bak-…` copy beside it, or move it aside and MixEngine will start fresh",
                    path.display()
                ))
            }

            // Split rather than given one code, because two different things arrive here: a
            // question about a service that is not there, and a set of specs that does not describe
            // a runnable system. Neither is `internal` — the second is a fault in whatever
            // *declared* those services and not in this machine, and calling it a bug of ours would
            // send the user looking anywhere except the one file they can fix.
            Core::Graph(error) => match error {
                GraphError::NoSuchService { .. } => Error::new(ErrorCode::NotFound, chain(self))
                    .with_hint("`mix service list` shows what does exist"),

                // The message names the services involved, and for a loop it writes the loop out;
                // what it cannot know is where they were written down, which is the whole of the
                // advice available.
                _ => Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                    "services are declared by MixEngine's own packages and by any `extension.toml` \
                     you have added; the edge to change is in whichever of them declares the \
                     services named above",
                ),
            },

            Core::Install(error) => match error {
                // **Reachable by a client once `runtime.*` existed, and unclassified before it.**
                // The download pipeline landed every variant below with no method in front of them,
                // so each fell into the `internal` arm at the bottom of this match — which was true
                // of nothing except that nobody could reach them. `runtime.*` is what made them a
                // user's problem, so this is where they get a code and a way out.
                //
                // The split that matters is *whose fault it is*, because that is what decides where
                // a person is sent: a document or an archive that verified and is then unusable is
                // one we published, and a signature or a checksum that does not match is somebody
                // between us and them. Only the first is `internal`.
                InstallError::IndexTransport { .. }
                | InstallError::ArtifactTransport { .. } => {
                    Error::new(ErrorCode::Io, chain(self)).with_hint(
                        "MixEngine fetches the version list and every runtime over the internet; a \
                         machine that is offline, or behind a proxy that needs a certificate this one \
                         does not trust, is the usual reason it cannot",
                    )
                }

                // The download stopped part way after every resume attempt, and what is on disk is
                // *kept*: asking again continues from it rather than starting over, which is the
                // one thing worth telling somebody who is about to try.
                InstallError::ArtifactIncomplete { .. } => {
                    Error::new(ErrorCode::Io, chain(self))
                        .with_hint("what arrived is kept; asking again resumes from it")
                }

                // The one failure that cannot happen by accident, and the one place this daemon
                // refuses something that verified as well-formed. `precondition_failed` rather than
                // `internal`: nothing here is broken, and what has to change is which server is
                // being asked.
                InstallError::IndexSignature { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "the index is signed by MixEngine and checked before it is read; a mirror \
                         serving somebody else's document, or an index published by a team using its \
                         own key, needs that key given to `mixengined --index-key`",
                    )
                }

                // The root is ours and this file is not the one it names. Nothing here is broken
                // and nothing was kept: the list is read again without it, and the next ask tries
                // again.
                InstallError::IndexKind { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "this file did not match what the signed index says it should be, which \
                         usually means the index was being published at that moment; ask again in a \
                         minute",
                    )
                }

                // Bytes that are ours by signature and then do not match what the index says they
                // hash to, or are longer than it says they are. A corrupted transfer or a mirror
                // serving something else; either way the download has been deleted and the next
                // step is the same one.
                InstallError::ArtifactChecksum { .. }
                | InstallError::ArtifactTooLarge { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "the download did not match what the signed index publishes for it and has \
                         been discarded; asking again fetches it afresh",
                    )
                }

                // An archive shape, or a document version, from a pipeline newer than this build.
                // Not a bug and not a dead end: the update is the fix, and saying so beats the
                // field-by-field confusion a best-effort parse would produce.
                InstallError::IndexSchema { .. }
                | InstallError::ArtifactFormat { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self))
                        .with_hint("this release of MixEngine is older than what it is being offered")
                }

                // A correctly signed index from before a security release, replayed. The cached
                // document is kept, so nothing was lost — what is worth saying is that the *server*
                // is behind.
                InstallError::IndexRolledBack { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "the index already held is newer and is still being used; a mirror that has \
                         stopped syncing is the usual reason a server offers an older one",
                    )
                }

                // The archive names a path outside where it is being unpacked: the oldest attack
                // there is against an installer, and one a correct signature says nothing about.
                // Nothing was written — the staging directory it was going into is gone.
                InstallError::UnsafeArchiveEntry { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "nothing was unpacked; please report this, quoting the archive named above",
                    )
                }

                // Past the signature and past the checksum, so these bytes *are* the ones we
                // published and they are unusable: our packaging, our bug. The same relationship
                // `IndexUnreadable` has to `IndexSignature`, one layer down.
                InstallError::IndexUnreadable { .. }
                | InstallError::ArchiveUnreadable { .. }
                | InstallError::MissingFromArtifact { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(
                        "this is a bug in what MixEngine published rather than on this machine; \
                         `logs/daemon.log` has the detail a report needs",
                    )
                }

                // **What a checksum cannot say.** The bytes are ours and this machine will not run
                // them: a missing VC++ redistributable, a glibc older than the build's floor, an
                // image the loader refuses. `dependency_missing` is the code that tells a GUI to
                // offer the thing that is missing, which is exactly the shape of every one of
                // those.
                InstallError::SmokeTestFailed { .. } => {
                    Error::new(ErrorCode::DependencyMissing, chain(self)).with_hint(
                        "nothing was installed; the runtime was run once from a staging directory and \
                         would not start, so the message above is the operating system's own",
                    )
                }

                // Logged where it happens and reported by `mix doctor`, never answered to a
                // request: nothing a client asks for fails because a JDK's store could not be
                // written.
                InstallError::KeytoolRefused { .. } => {
                    Error::new(ErrorCode::Internal, chain(self))
                }

                // A broken build: the key compiled into this binary is not a key. Nothing a user
                // did and nothing they can do.
                InstallError::IndexKey { .. } => {
                    Error::new(ErrorCode::Internal, chain(self))
                }

                // Never reaches a client as an error: the job registry judges an ending by the
                // token rather than by what the work returned, so work that gave up when asked is
                // recorded as *cancelled*. Classified all the same, because a value that can be
                // constructed can be rendered.
                InstallError::InstallCancelled => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self))
                        .with_hint("what had been downloaded is kept; asking again resumes from it")
                }

                // Never given a code of their own: what the `_` arm below gives every other error.
                InstallError::Timestamp { .. } => Error::new(ErrorCode::Internal, chain(self)),
            },

            Core::Runtime(error) => match error {
                // `UnsupportedPlatform` and not `InvalidArgument`: the request is well formed and
                // is answerable on another machine — the same name is a module on Windows and
                // linked in on the Unix cells — so what refuses it is this build rather than the
                // sentence.
                RuntimeError::ExtensionCompiledIn { .. } => {
                    Error::new(ErrorCode::UnsupportedPlatform, chain(self)).with_hint(
                        "this build has it linked in, so it is always loaded; nothing can unload it \
                         short of a build that ships it as a module",
                    )
                }

                // Two ways of saying "it is already here", and they are deliberately different
                // variants one layer down: `AlreadyInstalled` is a directory, `AlreadyRecorded` is
                // a row. A client cannot act differently on them, so they share a code and a hint.
                RuntimeError::AlreadyInstalled { .. }
                | RuntimeError::AlreadyRecorded { .. }
                | RuntimeError::PackageAlreadyRecorded { .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self)).with_hint(
                        "an installed version is never overwritten; uninstall it first if it is to be \
                         replaced",
                    )
                }

                // A directory an earlier home installed and this one could not check. Recorded by
                // hand once it can be checked, or removed.
                RuntimeError::UnrecordedInstall { path, .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self)).with_hint(format!(
                        "`mix runtime adopt` or `mix package adopt` records it once it can be checked, \
                         or remove {} and install again",
                        path.display()
                    ))
                }

                // A hand-edited database, or a row from a build that knew a channel this one does
                // not. The same reading `UnknownServiceState` gets, and the same code.
                RuntimeError::UnreadableRuntimeRow { .. }
                | RuntimeError::UnreadablePackageRow { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(UNREADABLE_ROW)
                }

                // **The one failure `runtime.resolve` exists to produce well.**
                // `dependency_missing` is the code the feature spec names, and the hint is the
                // whole value of the answer: what to type. A range cannot become an install command
                // — inventing a version would be inventing a release — so it becomes the listing
                // instead.
                RuntimeError::RuntimeUnresolved {
                    kind, constraint, ..
                } => Error::new(ErrorCode::DependencyMissing, chain(self))
                    .with_hint(mixengine_core::resolve::install_command(*kind, constraint)),

                // Nothing was asked for and there is nothing to fall back on. Distinct from the
                // above because the way out is different: there is no constraint to satisfy, only a
                // kind with no default — which is what uninstalling the last version leaves behind.
                // **The row names the folder, and the folder decides.** The version is recorded and
                // its folder is gone: the hint is the install that restores it in place, and the
                // uninstall that forgets it.
                RuntimeError::RuntimeMissing { kind, version, .. } => {
                    Error::new(ErrorCode::DependencyMissing, chain(self))
                        .with_hint(mixengine_core::resolve::restore_command(*kind, version))
                }

                RuntimeError::NoDefaultRuntime { kind } => {
                    Error::new(ErrorCode::DependencyMissing, chain(self)).with_hint(format!(
                        "`mix runtime list --kind {kind}` shows what is installed, and \
                         `mix runtime default {kind} <version>` chooses which one is used here"
                    ))
                }

                // The version is there and the program is not, which no client can produce today —
                // the shim is what looks an executable up, and it resolves in its own process
                // without a daemon. Classified anyway, on `InstallCancelled`'s reasoning: a variant
                // that can be constructed can be rendered, and `internal` would be the wrong word
                // for the case this actually covers, which is a runtime installed before its
                // executables were recorded.
                RuntimeError::RuntimeProvidesNothing { kind, version, .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(format!(
                        "`mix runtime install {kind} {version}` after uninstalling it records what the \
                         build publishes"
                    ))
                }

                // A broken installation rather than anything the caller did, which is what
                // `internal` means here — but the hint is worth having anyway, because the one
                // thing a person can do about it is reinstall, and nothing in the message says so.
                RuntimeError::ShimMissing { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(
                        "a release ships mixengined and mixengine-shim in one directory; reinstall \
                     MixEngine, or build the whole workspace if this is a development tree",
                    )
                }

                // Never given a code of their own: what the `_` arm below gives every other error.
                RuntimeError::RestoreRefused { .. } => Error::new(ErrorCode::Internal, chain(self)),
            },

            Core::Service(error) => match error {
                // The third way of saying "it is already here", and the one whose repair is
                // different: a service is not replaced by installing something, it is replaced by
                // deleting it.
                ServiceError::ServiceAlreadyDeclared { .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self)).with_hint(
                        "`mix service delete` first; deleting a service keeps its data directory",
                    )
                }

                // The fourth, and the one whose repair is a different *argument* rather than a
                // different call: nothing has to be deleted, the create has to name somewhere else.
                // The message already carries the service that got there first, so the hint spends
                // itself on what to do about it.
                ServiceError::DataDirectoryTaken { holder, .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self)).with_hint(format!(
                        "give this one a directory of its own, or `mix service delete {holder}` if it \
                         is the one that should go; two servers over one data directory corrupt it"
                    ))
                }

                ServiceError::InvalidDatabaseName { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "a database and its account are named like a project: lower-case letters, \
                         digits and hyphens, up to thirty-two characters",
                    )
                }

                // The message never carries the value — `ServiceError::InvalidPassword` has none to
                // leak.
                ServiceError::InvalidPassword { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "1 to 128 printable ASCII characters, and none of: a quote, a backslash, or \
                         whitespace",
                    )
                }

                // `conflict` rather than `already_exists`: what is refused is not the name being
                // taken but MixEngine being unable to prove the account is its own to change.
                ServiceError::AccountNotOurs { .. } => Error::new(
                    ErrorCode::Conflict,
                    chain(self),
                )
                .with_hint(
                    "MixEngine only manages an account whose password it holds; `--user` picks another \
                     name, or drop that account on the server first",
                ),

                // **Not `unsupported`**, which means *this operating system cannot* and would be a
                // lie about the machine: every system this ships to runs Redis, and Redis is what
                // has no databases. The same distinction the first planner drew for
                // `blueprint.apply`.
                ServiceError::NoDatabaseVocabulary { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "only the database servers have databases; `mix service list` shows what this \
                         home runs",
                    )
                }

                // **The generated-configuration family.** What separates these is the same thing
                // that separates the index failures above: whose fault it is. An override is the
                // user's, a template and a recipe are ours, and a row that cannot be read back is
                // neither — it is a database somebody edited.
                //
                // All three of these are a person's own overrides, and the message already names
                // the setting — the ones that exist, the shape it had to be, or what is wrong with
                // the value the recipe was handed. The hint says the thing the message cannot: that
                // being refused is the *feature*, because the alternative is a setting silently
                // doing nothing.
                ServiceError::UnknownSetting { .. }
                | ServiceError::SettingType { .. }
                | ServiceError::SettingValue { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "MixEngine refuses a setting it does not know rather than ignoring it, so that \
                         a typo cannot look like a setting that is in effect",
                    )
                }

                // The service's own program said no. Nothing was installed — the rendering is
                // judged in a staging directory — and saying so is the whole point: a user who has
                // just been told their configuration is broken will otherwise assume their site is
                // down.
                ServiceError::ConfigRejected { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "nothing was installed and the configuration that is live is the last one that \
                         worked; the service reading it has not been disturbed",
                    )
                }

                // A `packages.name` this build has no recipe for: a home written by a newer
                // MixEngine, or a service an extension declared and then went away. Not `internal`,
                // because nothing is broken here and the way out is a version rather than a bug
                // report.
                ServiceError::NoRecipe { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "what MixEngine can run is compiled into it; this home describes a service \
                         from a newer release, or from an extension that is no longer installed",
                    )
                }

                // Ours: a template that does not render, or a recipe that produced a spec the
                // supervisor will not take. An override can *reach* both, which is why they are not
                // unreachable, but neither is a mistake a user could have avoided.
                ServiceError::TemplateBroken { .. }
                | ServiceError::Unrunnable { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(
                        "this is a bug in MixEngine's own configuration templates rather than on this \
                         machine; `logs/daemon.log` has the detail a report needs",
                    )
                }

                // Never given a code of their own: what the `_` arm below gives every other error.
                ServiceError::IllegalTransition { .. } | ServiceError::StateRaced { .. } | ServiceError::UnknownServiceState { .. } | ServiceError::UnreadableServiceRow { .. } | ServiceError::UnreadableServiceDocument { .. } | ServiceError::PortsExhausted { .. } | ServiceError::ServiceProvidesNothing { .. } | ServiceError::PackageProvidesNothing { .. } => Error::new(ErrorCode::Internal, chain(self)),
            },

            Core::Project(error) => match error {
                // The fifth way of saying "it is already here", and its repair is a different
                // argument rather than a different call: the message already names the project in
                // the way, so the hint spends itself on what to do about it.
                ProjectError::ProjectRootTaken { holder, .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self)).with_hint(format!(
                        "`mix project show {holder}` is the one that has it; one directory is one \
                         project"
                    ))
                }

                ProjectError::ProjectNameTaken { .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self))
                        .with_hint("`mix project list` shows the names that are taken")
                }

                // The user's own argument, and the message already says which rule it broke.
                ProjectError::InvalidProjectName { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "a project name is a handle: up to sixty-four characters, no path separators \
                         and no control characters",
                    )
                }

                ProjectError::InvalidDomain { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self))
                        .with_hint("a domain is lowercase ASCII labels on .test, .localhost or .local")
                }

                ProjectError::UnmanagedTld { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "use .test; it is reserved for exactly this and resolves nowhere else",
                    )
                }

                ProjectError::RiskyTld { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self))
                        .with_hint("`--i-know` accepts it anyway; .test avoids the question")
                }

                ProjectError::DomainTaken { .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self))
                        .with_hint("`mix site update` can move it, or pick another name")
                }

                ProjectError::LastDomain { .. } => {
                    Error::new(ErrorCode::Conflict, chain(self))
                        .with_hint("`mix site delete` removes the site itself")
                }

                ProjectError::PrimaryDomain { .. } => {
                    Error::new(ErrorCode::Conflict, chain(self)).with_hint(
                        "`mix site update --domain <new-primary> --domain <the-rest>` reorders",
                    )
                }

                ProjectError::DocRootOutsideProject { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self))
                        .with_hint("a doc root is a directory inside the project's own root")
                }

                ProjectError::HttpsRedirectNeedsHttps => {
                    Error::new(ErrorCode::InvalidArgument, chain(self))
                        .with_hint("`--https true` first, or leave `--https-redirect` unset")
                }
                ProjectError::UnreadableProjectRow { .. }
                | ProjectError::UnreadableSiteRow { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(UNREADABLE_ROW)
                }

                // The user's own file, one directory out from `Core::Config` and given the same
                // code — and the same hint would be wrong: `mixengine.toml` is checked into their
                // repository, so "delete it and a fresh one will be written" is advice about
                // somebody else's file.
                ProjectError::Manifest { path, .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(format!(
                        "only the `[runtimes]` table of {} is read while resolving a version; the \
                         languages it may name are php, node, python and ruby",
                        path.display()
                    ))
                }

                // The user's file again, and the same code: what a person does about either is open
                // the file. The hint differs because the repair does — nothing here is about
                // `[runtimes]`.
                ProjectError::ManifestEdit { path, .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(format!(
                        "{} could not be rewritten with the project in it; check that it is a TOML \
                         file this user can write",
                        path.display()
                    ))
                }

                // A client sent a directory that means nothing to a daemon. The message names it,
                // and what to do about it is the caller's own bug rather than the user's.
                ProjectError::NotAbsolute { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "a directory is resolved by walking up from it, so it has to be one this \
                         machine can find on its own",
                    )
                }
            },

            Core::Blueprint(error) => match error {
                // **The first planner left these four in the catch-all below**, so a mistyped
                // blueprint name reached a client as an internal error. Found while adding the
                // three above; fixed here rather than left for whoever meets it next.
                BlueprintError::InvalidBlueprintName { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "a blueprint name is a filename stem: lower-case letters, digits and hyphens",
                    )
                }

                BlueprintError::BlueprintExists { .. } => {
                    Error::new(ErrorCode::AlreadyExists, chain(self))
                }

                // **About the project, never the daemon failing.** The several-sites refusal this
                // replaced had no arm, and reached clients as `internal`.
                BlueprintError::ProjectRunsSeveralPhps { .. } => {
                    Error::new(ErrorCode::Conflict, chain(self)).with_hint(
                        "a blueprint has one [runtimes] php; move the sites onto one PHP first",
                    )
                }

                BlueprintError::BlueprintManifest { .. }
                | BlueprintError::UnknownBlueprintSchema { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self))
                }

                BlueprintError::UnknownBlueprintSource { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(
                        "this home holds a blueprint filed by a newer MixEngine than the one running",
                    )
                }

                // The signature is a property of the file somebody named, so the file is the
                // argument that was wrong. Classified here rather than left to the `_ => internal`
                // arm, which is where the first planner left four variants and the executor had to
                // come back for them.
                BlueprintError::BlueprintSignature { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self))
                        .with_hint("it can still be imported; it will be marked untrusted for good")
                }

                BlueprintError::BlueprintKey { .. } => {
                    Error::new(ErrorCode::Internal, chain(self))
                }

                // Never given a code of their own: what the `_` arm below gives every other error.
                BlueprintError::ArchiveTooLarge { .. } | BlueprintError::ArchiveFormatUnknown { .. } | BlueprintError::ArchiveStripMissing { .. } => Error::new(ErrorCode::Internal, chain(self)),
            },

            Core::Extension(error) => match error {
                // **Every one of them classified, none left to the catch-all.** The first planner
                // left four blueprint variants falling through, and a mistyped name reached a
                // client as an internal error; the whole set is spelled out here rather than
                // repeating that. All are `InvalidArgument` because all of them are about the file
                // the caller handed in, and the one thing they need is where to look.
                ExtensionError::ExtensionManifest { .. }
                | ExtensionError::UnknownExtensionSchema { .. }
                | ExtensionError::ExtensionTableUnexpected { .. }
                | ExtensionError::ExtensionTableMissing { .. }
                | ExtensionError::ExtensionField { .. }
                | ExtensionError::ExtensionIdTaken { .. }
                | ExtensionError::ExtensionSpec { .. } => {
                    Error::new(ErrorCode::InvalidArgument, chain(self)).with_hint(
                        "an extension declares itself in `extension.toml`, in its own directory",
                    )
                }

                // **A precondition rather than an invalid argument.** The id somebody typed is a
                // real service; what is wrong is the state of this home, in which that service is
                // one extension's own process and holds a credential no project's PHP may read.
                ExtensionError::ExtensionPoolNotShared { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "a web-app extension is served on a pool of its own, which carries a database \
                         superuser's password; name the shared `php-fpm@<version>` instead",
                    )
                }

                // **The same shape for a `web-app`'s database.** An administrative interface onto a
                // server this machine does not run is an install whose stated effect cannot happen,
                // and the way out is the same kind of sentence: what to create. `internal` was what
                // it reached a client as before this arm, which is the trap the four blueprint
                // variants above fell into.
                ExtensionError::ExtensionNoDatabase { engines, .. } => {
                    Error::new(ErrorCode::DependencyMissing, chain(self)).with_hint(format!(
                        "`mix service create {}@main <version>` gives this machine one",
                        engines.first().map_or("mariadb", String::as_str)
                    ))
                }

                // Never given a code of their own: what the `_` arm below gives every other error.
                ExtensionError::ExtensionEntry { .. } | ExtensionError::ExtensionAlreadyInstalled { .. } | ExtensionError::UnreadableExtensionRow { .. } | ExtensionError::ExtensionNotAService { .. } | ExtensionError::ExtensionNoArtifact { .. } | ExtensionError::RegistryPublicKeyShape { .. } | ExtensionError::RegistryKeyMismatch { .. } | ExtensionError::ExtensionFileName { .. } | ExtensionError::RegistryUnreadable { .. } => Error::new(ErrorCode::Internal, chain(self)),
            },

            Core::Elevation(error) => match error {
                // The one a person can act on, and the reason it is not `internal` like its
                // shim-shaped sibling: nothing can be granted until this file is there, and saying
                // so at the method that needs it is what makes the sentence useful.
                //
                // **Per OS and not one sentence copied from `ShimMissing`'s.** The shim really does
                // always ship beside `mixengined`; the elevate helper does not — a `.pkg`, a `.deb`
                // or an `.rpm` writes it straight to this system's protected directory and
                // "reinstall" is the only fix, while the NSIS installer and the portable archives
                // keep a copy beside the program that survives an uninstall on their own.
                // `missing_helper_advice` is the one place that distinction is written down, so
                // this arm and its sibling cannot drift apart again.
                ElevationError::ElevateMissing { .. } => {
                    Error::new(ErrorCode::DependencyMissing, chain(self)).with_hint(format!(
                        "{}, or build the whole workspace if this is a development tree",
                        mixengine_platform::install::missing_helper_advice()
                    ))
                }

                // **Not `dependency_missing`, because nothing is missing.** The file is there and
                // this daemon will not run it as an administrator, which is a state of the machine
                // rather than an incomplete install, and the hint therefore says what to look at
                // instead of saying "reinstall".
                ElevationError::ElevateUntrusted { path, .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(format!(
                        "MixEngine will not run {} as an administrator until that file and the \
                         directory holding it belong to one; check who owns them, and reinstall if \
                         you cannot account for how they came to belong to anybody else",
                        path.display()
                    ))
                }

                // A caller bug: `elevation.grant` refuses an empty queue before it composes a
                // request.
                ElevationError::ElevateRequestEmpty => {
                    Error::new(ErrorCode::InvalidArgument, chain(self))
                }

                // `Completed` means the helper ran, not that it left a report, so this is a state
                // rather than an impossibility — and one nothing a user typed caused.
                ElevationError::ElevateReportMissing { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(
                        "the elevated helper ended without writing its answer; `logs/daemon.log` has \
                         the detail, and nothing was applied that it did not report",
                    )
                }

                ElevationError::ElevateReportUnreadable { .. }
                | ElevationError::ElevateReportMismatched { .. } => {
                    Error::new(ErrorCode::Internal, chain(self)).with_hint(
                        "the helper beside this daemon is not the one it expects; reinstall MixEngine",
                    )
                }

                // Unreachable: a `PrivilegedOp` is one of ours and holds nothing serde can refuse.
                ElevationError::OpUnwritable { .. } => {
                    Error::new(ErrorCode::Internal, chain(self))
                }
            },

            // The message already ends in "unset it to use this platform's default location",
            // which is the entire advice available; a hint here would be the same sentence twice.
            Core::EmptyHome => Error::new(ErrorCode::InvalidArgument, chain(self)),

            // Delegated rather than flattened: the OS failure keeps its own code, and going
            // through `chain` here would print a `#[error(transparent)]` message twice.
            Core::Platform(error) => error.to_wire(),

            Core::Update(error) => match error {
                // A copy the `.pkg` installed, asked to swap in place: the way that works is named.
                UpdateError::UpdateUsesInstaller { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self))
                        .with_hint("`mix self-update` opens the next .pkg in Installer.app")
                }

                UpdateError::InstallerUnavailable { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self))
                }

                UpdateError::UpdateNotInstalled { .. } => {
                    Error::new(ErrorCode::PreconditionFailed, chain(self)).with_hint(
                        "finish the installation in Installer.app, then run `mix self-update --finish`",
                    )
                }

                // Never given a code of their own: what the `_` arm below gives every other error.
                UpdateError::SettingUnwritable { .. } | UpdateError::UpdateNotWritable { .. } | UpdateError::UpdateNotOffered { .. } | UpdateError::UpdateUnavailable { .. } | UpdateError::HelperSignature { .. } | UpdateError::HelperStampUnreadable { .. } | UpdateError::HelperNotForThisMachine { .. } | UpdateError::HelperUnavailable { .. } => Error::new(ErrorCode::Internal, chain(self)),
            },

            Core::Job(error) => match error {
                // Never given a code of their own: what the `_` arm below gives every other error.
                JobError::JobEnded { .. }
                | JobError::IllegalJobTransition { .. }
                | JobError::UnknownJobState { .. }
                | JobError::UnknownJobKind { .. }
                | JobError::UnreadableJobRow { .. }
                | JobError::JobOutcomeUnwritable { .. } => Error::new(ErrorCode::Internal, chain(self)),
            },

            // `mixengine_core::Error` is `#[non_exhaustive]`, so this arm is mandatory rather than
            // chosen. A variant that lands in it is one nobody has classified yet, and `internal`
            // is the honest name for that.
            _ => Error::new(ErrorCode::Internal, chain(self)),
        }
    }
}

/// What a Linux release with no credential store is told to do.
///
/// A development build keeps a file already, and Windows and macOS always have a store in the
/// session a daemon runs in, so only a Linux release is offered the file.
pub(crate) fn keyring_hint(release: bool, os: &str) -> Option<&'static str> {
    (release && os == "linux").then_some(
        "this machine has no credential store; to keep this home's passwords in a file only your \
         account can read, run `mix daemon credential-store home`",
    )
}

impl ToWire for mixengine_platform::Error {
    fn to_wire(&self) -> Error {
        use mixengine_platform::Error as Platform;

        match self {
            // `reason` is required to describe the manual workaround where there is one
            // (`docs/architecture/platform-abstraction.md`, rule 4), and it is already in the
            // message. A Linux release with no keyring is also told MixEngine's own answer: a file
            // store in the home.
            Platform::UnsupportedPlatform { capability, .. } => {
                let error = Error::new(ErrorCode::UnsupportedPlatform, chain(self));
                match keyring_hint(mixengine_platform::RELEASE, std::env::consts::OS) {
                    Some(hint) if *capability == "Keyring" => error.with_hint(hint),
                    _ => error,
                }
            }

            // Not `io`: nothing was touched. The environment is missing something the OS considers
            // mandatory, and the user can fix that before anything else will work.
            Platform::NoHomeDirectory { .. } => {
                Error::new(ErrorCode::PreconditionFailed, chain(self))
            }

            Platform::Io { path, source, .. } => io_failure(chain(self), path, source),

            // `io` rather than `internal`, even though most ways this can happen are a bug: the
            // rest are a machine locked down past the point where a token can be read, and greeting
            // that with "report a bug" would send somebody to the wrong place. The code says the OS
            // refused something, which is true either way.
            Platform::Os { .. } => Error::new(ErrorCode::Io, chain(self)),

            // The address is computed from `MIXENGINE_HOME`, so this is the home being wrong rather
            // than anything having failed — and `reason` already ends in what to do about it.
            Platform::Address { .. } => Error::new(ErrorCode::InvalidArgument, chain(self)),

            // Not a failure of this daemon so much as a fact about the machine, which is why the
            // hint points at the daemon that *is* running instead of at something to repair.
            Platform::EndpointInUse { .. } => Error::new(ErrorCode::Conflict, chain(self))
                .with_hint(
                    "a MixEngine daemon is already running for this home; `mix status` talks to \
                     it, and `mix daemon stop` ends it",
                ),

            // A fact about the machine too, and a different one: the endpoint name carries this
            // account's own SID, so somebody else answering on it is somebody else's doing rather
            // than a second daemon of the user's. Deliberately no `mix daemon stop` in the hint —
            // there is nothing of theirs on the name to stop.
            Platform::EndpointNotOurs { .. } => Error::new(ErrorCode::Conflict, chain(self))
                .with_hint(
                    "another account created this endpoint before the daemon could; nothing has \
                     been served through it; end that process, or sign that account out, before \
                     starting MixEngine here",
                ),

            // The tool's own complaint is in the message, and it is a better hint than anything
            // that could be written here.
            Platform::Command { .. } => Error::new(ErrorCode::ProcessFailed, chain(self)),

            _ => Error::new(ErrorCode::Internal, chain(self)),
        }
    }
}

impl ToWire for mixengine_supervisor::Error {
    fn to_wire(&self) -> Error {
        use mixengine_supervisor::Error as Supervisor;

        match self {
            // **Not an `ErrorKind`, because it does not have one.** A Code Integrity refusal
            // arrives as an uncategorised OS error, and what is worth saying about it is not "the
            // process failed" but why this machine will never start this program: nothing MixEngine
            // ships or installs is code-signed, and the policy has no per-file override to offer.
            // Above the `kind()` match rather than inside it, because the kind carries none of
            // that.
            Supervisor::Spawn { program, source }
                if mixengine_platform::refused_by_app_control(source) =>
            {
                Error::new(ErrorCode::ProcessFailed, chain(self)).with_hint(format!(
                    "`{program}` is on this machine and {}",
                    mixengine_platform::APP_CONTROL_REFUSAL
                ))
            }

            Supervisor::Spawn { program, source } => match source.kind() {
                // A program that is not there is not a failed process — it is a missing
                // dependency, which is the code that tells a client to offer an install.
                io::ErrorKind::NotFound => Error::new(ErrorCode::DependencyMissing, chain(self))
                    .with_hint(format!(
                        "`{program}` is neither on PATH nor where the service expects it; install \
                         the runtime that provides it, or correct its path"
                    )),

                // The file is there and the OS refused to run it, which on Unix is nearly always
                // the executable bit and never something a restart fixes.
                io::ErrorKind::PermissionDenied => {
                    Error::new(ErrorCode::ProcessFailed, chain(self))
                        .with_hint(format!("`{program}` exists but is not executable"))
                }

                _ => Error::new(ErrorCode::ProcessFailed, chain(self)),
            },

            _ => Error::new(ErrorCode::Internal, chain(self)),
        }
    }
}

impl ToWire for crate::services::Undeclarable {
    fn to_wire(&self) -> Error {
        use crate::services::Undeclarable;

        match self {
            // The user's own declaration — a cycle, a dependency naming nothing, an id used twice —
            // which `mixengine_core::Error::Graph` already maps to `invalid_argument` with the hint
            // that says where such a thing is written. Delegated rather than re-classified here, so
            // there is one answer to "a set of specs that is not a graph" and not two.
            Undeclarable::Invalid(error) => error.to_wire(),

            // **The generator gave those failures a vocabulary, and the type now carries it.** The
            // source is the daemon's own trait and could in principle be anything, but everything
            // that implements it renders through `mixengine-core` — which already knows whether a
            // failure is a typo in somebody's overrides or a bug in ours. This arm used to hold an
            // `anyhow::Error` and *downcast* to ask: it worked, and it would have stopped working
            // the first time somebody added a `.context(…)` on the way out, silently, by
            // classifying a misspelled setting as `internal` and sending its author to file a bug
            // report.
            Undeclarable::Unavailable(error) => error.to_wire(),
        }
    }
}

/// `io`, plus whatever the OS error kind implies about the way out.
///
/// Shared by `core` and `platform`, whose `Io` variants are deliberately the same shape: the path
/// belongs in the message, the OS error stays the cause, and the advice depends on which of the two
/// or three things that actually go wrong here went wrong.
fn io_failure(message: String, path: &Path, source: &io::Error) -> Error {
    let error = Error::new(ErrorCode::Io, message);

    match source.kind() {
        // The one thing a user cannot guess: MixEngine is not going to elevate its way out of this
        // one. Everything privileged is a one-shot `mixengine-elevate` call for a listed operation
        // (ADR 0005), and touching an arbitrary directory is not on the list.
        io::ErrorKind::PermissionDenied => error.with_hint(format!(
            "MixEngine runs as your own user account and never as an administrator; give \
             yourself access to {}, or move it with [paths] in config.toml",
            path.display()
        )),

        // Carefully worded: which component is missing depends on the action. `create` goes
        // through `create_dir_all`, so a failure there is the drive or the mount point rather than
        // the parent directory, while a `read` is usually the file itself. The one thing worth
        // saying is the one only this layer knows, and it holds either way.
        io::ErrorKind::NotFound => error.with_hint(
            "check the [paths] section of config.toml; a relocation onto a disk that is not \
             mounted is the usual reason a path is simply not there",
        ),

        _ => error,
    }
}

/// Flatten an error and its causes into the single string a client is given.
///
/// A local name for [`mixengine_proto::flatten`], which is where it lives because `mix` needs the
/// same one — see the note at the top of this module. Kept as a function rather than inlined at
/// every arm so that the mapping above reads the way it did when this was written here.
fn chain(error: &dyn std::error::Error) -> String {
    mixengine_proto::flatten(error)
}

#[cfg(test)]
mod tests;
