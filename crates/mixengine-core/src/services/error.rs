//! The ways a service's state, settings, generated configuration and ports can be wrong.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw — roadmap task **T207j**.

use std::path::PathBuf;

/// The ways a service's state, settings, generated configuration and ports can be wrong.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// A database or account name is not one this build will put in a statement.
    ///
    /// **The refusal is the boundary** — roadmap task **T77a**. Every statement quotes its
    /// identifiers and nothing escapes them, which is safe for exactly one reason: this refused
    /// every character that could end a quoted identifier.
    #[error("{name} cannot be a database or account name: {reason}")]
    InvalidDatabaseName {
        /// What was asked for.
        name: String,
        /// Which rule it broke, in the words the user is shown.
        reason: &'static str,
    },

    /// A password a person chose is not one this build will put in a statement.
    ///
    /// **Never the value** — roadmap task **T77b**. Unlike [`Error::InvalidDatabaseName`], this
    /// variant carries no copy of what was refused: a database or account name is meant to be seen
    /// again, a password is not, and an error that echoed it back would put it in a terminal's
    /// scrollback and a CI log for the one call that exists to keep it out of both.
    ///
    /// [`Error::InvalidDatabaseName`]: crate::ServiceError::InvalidDatabaseName
    #[error("that password cannot be used: {reason}")]
    InvalidPassword {
        /// Which rule it broke, in the words the user is shown.
        reason: &'static str,
    },

    /// An account of this name is on the server and MixEngine holds no credential for it.
    ///
    /// **A keyring entry is the deed of ownership** — the T77a design, D3. Without this refusal,
    /// "make sure the account exists" would be an `ALTER USER` that silently resets the password of
    /// an account somebody else made.
    #[error(
        "an account called {user} already exists on {service}, and MixEngine has no credential for \
         it"
    )]
    AccountNotOurs {
        /// Which instance.
        service: String,
        /// Which account.
        user: String,
    },

    /// This package has no databases to make.
    #[error("{package} has no databases: nothing on it can be created that way")]
    NoDatabaseVocabulary {
        /// The `packages.name` that was asked.
        package: String,
    },

    /// A service was asked to make a move its state machine does not have.
    ///
    /// A bug in the caller rather than a condition to recover from — every legal edge is in
    /// [`mixengine_proto::ServiceState::can_become`], and code that asks for one that is not there
    /// has lost track of what it is supervising. Reported instead of silently written, because a
    /// state machine that accepts anything is a status display that means nothing.
    #[error("{service} cannot go from {from} to {to}")]
    IllegalTransition {
        /// The service that was asked.
        service: String,
        /// Where it actually is.
        from: mixengine_proto::ServiceState,
        /// Where the caller wanted it.
        to: mixengine_proto::ServiceState,
    },

    /// The state changed between being read and being written.
    ///
    /// The assertion behind [`services::transition`]'s compare-and-swap. Two supervisors reaching
    /// the same service at once — a health check going `Degraded` while a user's stop arrives — are
    /// serialised by the `BEGIN IMMEDIATE` that transaction opens with, so the second one reads what
    /// the first committed and judges its move against that. This error is what says that stopped
    /// being true: something wrote the row from outside that transaction, and the transition the
    /// caller computed was based on a state that is no longer there.
    ///
    /// [`services::transition`]: crate::services::transition
    #[error("the state of {service} changed while it was being moved from {expected}")]
    StateRaced {
        /// The service that was being moved.
        service: String,
        /// What it had been when the decision was made.
        expected: mixengine_proto::ServiceState,
    },

    /// A `services` row holds a state this build does not recognise.
    ///
    /// Unreachable through our own writes — the column is `CHECK`ed against the same closed list
    /// [`mixengine_proto::ServiceState`] is — so it means a database edited by hand, or one written
    /// by a version that knew a state this one does not.
    #[error("the state of {service} is stored as {value}, which is not a service state")]
    UnknownServiceState {
        /// The service whose row cannot be read.
        service: String,
        /// The word that is in the column.
        value: String,
    },

    /// A `services` row holds a value this build cannot read back.
    ///
    /// [`RuntimeError::UnreadableRuntimeRow`]'s sibling on the other table, and reached through the
    /// same two doors: a database edited by hand, or a row written by a build that knew more than
    /// this one. `id` has no `CHECK` that says it is a [`mixengine_proto::ServiceId`] and `port`
    /// none that says it fits in sixteen bits, so the reader is the only thing that refuses.
    ///
    /// [`RuntimeError::UnreadableRuntimeRow`]: crate::RuntimeError::UnreadableRuntimeRow
    #[error("the {column} of the service {service} is stored as {value}, which cannot be read")]
    UnreadableServiceRow {
        /// The service whose row cannot be read.
        service: String,
        /// Which column.
        column: &'static str,
        /// What is in it, or why it was refused.
        value: String,
    },

    /// A `*_json` column of a `services` row does not hold the document it is supposed to.
    ///
    /// The one thing SQLite cannot constrain, as at [`JobError::UnreadableJobRow`]: the column is
    /// TEXT, and no `CHECK` can say it is an object of overrides or a set of resource limits.
    ///
    /// [`JobError::UnreadableJobRow`]: crate::JobError::UnreadableJobRow
    #[error("the {column} of the service {service} is not a document this build can read")]
    UnreadableServiceDocument {
        /// The service whose row cannot be read.
        service: String,
        /// Which column.
        column: &'static str,
        /// How it failed to parse.
        #[source]
        source: serde_json::Error,
    },

    /// A `services` row belongs to a package this build has no recipe for.
    ///
    /// What a service *is* — the binary, the template, the ready check — is compiled in, so a row
    /// naming something else cannot be started, configured or listed. Reached by a home whose
    /// database was written by a newer MixEngine, and by every home for a service an extension
    /// declared and then went away.
    #[error(
        "nothing in this build knows how to run {package}, which the service {service} belongs to \
         (it knows: {})",
        if known.is_empty() { "nothing yet".to_owned() } else { known.join(", ") }
    )]
    NoRecipe {
        /// The service that cannot be generated.
        service: String,
        /// The `packages.name` there is no recipe for.
        package: String,
        /// What this build does have recipes for, in the order a listing shows them.
        known: Vec<String>,
    },

    /// An override names a setting its service does not have.
    ///
    /// Refused rather than ignored, which is `config.toml`'s rule ([`Error::Config`]) one directory
    /// down: a silently dropped override is a setting the user believes is in effect.
    ///
    /// [`Error::Config`]: crate::Error::Config
    #[error(
        "{service} has no setting called {key} (it has: {})",
        if known.is_empty() { "none".to_owned() } else { known.join(", ") }
    )]
    UnknownSetting {
        /// The service the override was written against.
        service: String,
        /// The key that is not one of its settings.
        key: String,
        /// The keys that are, in the order a listing shows them.
        known: Vec<String>,
    },

    /// An override is the right key and the wrong shape.
    ///
    /// `"port": "3306"` — a number written as a string — is the one that actually happens, and it
    /// is worth its own message because both halves look correct on their own.
    #[error("the {key} of {service} has to be {expected}, and is {found}")]
    SettingType {
        /// The service the override was written against.
        service: String,
        /// Which setting.
        key: String,
        /// What the recipe declared it as.
        expected: &'static str,
        /// What the override offered.
        found: &'static str,
    },

    /// An override is the right key, and the right shape, and still cannot be used.
    ///
    /// `"admin_port": 70000` is a whole number exactly as the recipe declared it, and is not a port.
    /// [`Error::SettingType`] cannot say that — it names two *shapes*, and both of these are the
    /// same one — so the distinction is what each message can carry: that one says what the value is
    /// instead of, and this one says what is wrong with the value itself.
    ///
    /// Raised from a recipe rather than from the merge, because what a value is allowed to be is
    /// knowledge about the service and not about the type: 70000 is not a port, and 3 is not a
    /// number of megabytes for an InnoDB buffer pool.
    ///
    /// [`Error::SettingType`]: crate::ServiceError::SettingType
    #[error("the {key} of {service} cannot be {value}: {reason}")]
    SettingValue {
        /// The service the override was written against.
        service: String,
        /// Which setting, as the recipe declares it.
        key: &'static str,
        /// What the override offered, as it would be written back.
        value: String,
        /// Why it cannot be used, as a clause completing the sentence.
        reason: &'static str,
    },

    /// A template this build ships does not render.
    ///
    /// Ours and not the user's: templates are compiled in, and an override cannot make one
    /// syntactically invalid. What it *can* do is reach a branch nothing had exercised, which is why
    /// the message names the file rather than only the service.
    #[error("the {file} MixEngine generates for {service} could not be rendered")]
    TemplateBroken {
        /// The service being configured.
        service: String,
        /// Which of its files, as the recipe names it.
        file: &'static str,
        /// What the template engine said. Boxed to keep this enum small, as at
        /// [`InstallError::IndexTransport`].
        ///
        /// [`InstallError::IndexTransport`]: crate::InstallError::IndexTransport
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A generated configuration was refused by the program that has to read it.
    ///
    /// **Nothing was installed.** The rendering is judged in a staging directory — `caddy validate`,
    /// `nginx -t` — so the configuration that is live is the last one that worked, and the service
    /// reading it has not been disturbed.
    #[error("{} was refused by {}: {detail}", path.display(), checker.display())]
    ConfigRejected {
        /// The staged file the checker was pointed at.
        path: PathBuf,
        /// The program that refused it.
        checker: PathBuf,
        /// What it said, or that it said nothing.
        detail: String,
    },

    /// A recipe produced a specification that is not runnable.
    ///
    /// A bug in this build rather than anything a user wrote — a program that is not absolute, a
    /// grace period of zero — and reported rather than unwrapped because nothing in this crate
    /// panics. An override *can* reach it, since a recipe builds its spec out of settings, which is
    /// why the service is named.
    #[error("the configuration generated for {service} does not describe a service that can run")]
    Unrunnable {
        /// The service being configured.
        service: String,
        /// Which field, and why.
        #[source]
        source: mixengine_proto::SpecError,
    },

    /// A service with this id, or this instance of this package, already exists.
    ///
    /// Two unique constraints and one variant, because what a person did wrong is the same in both
    /// cases: `services.id` and `UNIQUE (package_id, instance_name)` both mean the service they
    /// asked for is already here.
    #[error("{service} already exists")]
    ServiceAlreadyDeclared {
        /// Which service.
        service: mixengine_proto::ServiceId,
    },

    /// Another service already points at this data directory — roadmap task **T36**.
    ///
    /// Two instances of one server are two data directories, and the layout the generator derives
    /// guarantees that on its own. This is the case it cannot guarantee: a caller that named the
    /// path itself, twice. Refused where the row is written, because two servers over one set of
    /// files is a cost paid in the user's data rather than in a start that fails.
    #[error("{holder} already keeps its data in {path}")]
    DataDirectoryTaken {
        /// The directory both services were pointed at.
        path: String,

        /// The id of the service that got there first, as its row spells it.
        holder: String,
    },

    /// Nothing free was found near the port a recipe asked for — roadmap task **T34c**.
    ///
    /// **A bounded search, and running out of it is an error rather than a longer loop.** A machine
    /// on which sixty-four consecutive ports above 3306 are all held is not a machine one more
    /// probe would help on: something is wrong with it that a person has to look at, and a service
    /// quietly landing on a number three hundred away from the one its product is documented under
    /// would hide that.
    #[error("no free port between {preferred} and {last}")]
    PortsExhausted {
        /// The port the recipe asked for.
        preferred: u16,

        /// The highest port the search reached.
        last: u16,
    },

    /// A recipe asked the install behind its service for an executable it does not publish.
    ///
    /// Distinct from [`RuntimeError::RuntimeProvidesNothing`], which is the shim's question —
    /// *which file is `php`* — asked of a runtime by kind and version. This one is asked by a
    /// **recipe**, of whatever the service's row points at, and it names the service because that
    /// is what the person reading has in their hand. The usual cause is an artifact packed without
    /// the SAPI the recipe needs: a PHP whose `provides` has `php` and no `php-fpm`.
    ///
    /// [`RuntimeError::RuntimeProvidesNothing`]: crate::RuntimeError::RuntimeProvidesNothing
    #[error(
        "{service} runs out of an install that publishes no executable called {executable} (it has: {})",
        if known.is_empty() { "nothing recorded".to_owned() } else { known.join(", ") }
    )]
    ServiceProvidesNothing {
        /// Which service.
        service: String,
        /// The name the recipe looked up.
        executable: String,
        /// What the install does publish, in the order a listing shows them.
        known: Vec<String>,
    },

    /// A client command was typed, and the install it belongs to publishes no such program.
    ///
    /// The third of the family and its own variant for the same reason the second is: what the
    /// person has in their hand is **the command they typed and the version it resolved to**, not a
    /// service id they may never have named. A Windows MariaDB packs no `mariadb-backup` on every
    /// branch, so this is the ordinary message on a machine where that name was never packed —
    /// `bin/` fronts what the *chosen* install publishes, and this is what a build that changed its
    /// mind between the refresh and the run says.
    #[error(
        "{package} {version} publishes no executable called {executable} (it has: {})",
        if known.is_empty() { "nothing recorded".to_owned() } else { known.join(", ") }
    )]
    PackageProvidesNothing {
        /// Which package.
        package: String,
        /// Which version of it the command resolved to.
        version: mixengine_proto::PackageVersion,
        /// The name that was looked up.
        executable: String,
        /// What it does publish, in the order a listing shows them.
        known: Vec<String>,
    },
}
