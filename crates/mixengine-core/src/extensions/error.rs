//! The ways an extension, its manifest and its registry can be wrong.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw.

/// The ways an extension, its manifest and its registry can be wrong.
#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    /// An `extension.toml` does not parse.
    ///
    /// **The path is here**, unlike [`BlueprintError::BlueprintManifest`], because there is a file
    /// to point at: an extension is read off disk and never out of a column.
    ///
    /// [`BlueprintError::BlueprintManifest`]: crate::BlueprintError::BlueprintManifest
    #[error("{path} is not an extension manifest this build can read")]
    ExtensionManifest {
        /// The file that was read.
        path: String,
        /// The parse failure, which carries the line and the column.
        #[source]
        source: toml::de::Error,
    },

    /// One entry of the published registry is not a manifest this build can read.
    ///
    /// **No path, where [`Error::ExtensionManifest`] has one**: this arrived inside a signed
    /// document, so there is no file to point somebody at. It is also the one manifest failure that
    /// is usually *not* shown to anybody — the listing skips such an entry and counts it, because
    /// one entry a newer build published should cost that entry and not the whole registry.
    ///
    /// [`Error::ExtensionManifest`]: crate::ExtensionError::ExtensionManifest
    #[error("a registry entry is not an extension manifest this build can read")]
    ExtensionEntry {
        /// The parse failure.
        #[source]
        source: Box<serde_json::Error>,
    },

    /// Something is already installed under that id.
    ///
    /// Named rather than left to the primary key, on [`RuntimeError::PackageAlreadyRecorded`]'s
    /// reasoning: the collision is a real case and the sentence a person needs names the extension
    /// and the way out, which is `mix extension uninstall` and not a constraint.
    ///
    /// [`RuntimeError::PackageAlreadyRecorded`]: crate::RuntimeError::PackageAlreadyRecorded
    #[error("{id} is already installed")]
    ExtensionAlreadyInstalled {
        /// What is already there.
        id: String,
    },

    /// An `extensions` row holds something this build cannot read back.
    ///
    /// [`ServiceError::UnreadableServiceRow`]'s twin one table across, and it exists for the same
    /// reason: a hand-edited database, or one written by a later MixEngine, is answered by naming
    /// the row and the column rather than by a failure somewhere downstream that mentions neither.
    ///
    /// [`ServiceError::UnreadableServiceRow`]: crate::ServiceError::UnreadableServiceRow
    #[error("the {column} of extension {extension} cannot be read: {value}")]
    UnreadableExtensionRow {
        /// Which extension's row.
        extension: String,
        /// Which column.
        column: &'static str,
        /// What it held.
        value: String,
    },

    /// A `services` row points at an extension that runs no process.
    ///
    /// Unreachable through this build's own writes, since only a `service` extension is given a
    /// row. Named rather than defaulted, on [`ServiceError::UnreadableServiceRow`]'s reasoning: a
    /// hand- edited database is answered by saying what is wrong with it.
    ///
    /// [`ServiceError::UnreadableServiceRow`]: crate::ServiceError::UnreadableServiceRow
    #[error("{id} is a {kind} extension and runs no process")]
    ExtensionNotAService {
        /// Which extension.
        id: String,
        /// What it actually is.
        kind: &'static str,
    },

    /// Nothing is published for this machine.
    ///
    /// A state rather than a fault: the extension exists and was simply not built for this OS and
    /// architecture, so the message names what *was* built rather than implying something is
    /// broken.
    #[error("{id} publishes no artifact for this machine; it has: {}", targets.join(", "))]
    ExtensionNoArtifact {
        /// Which extension.
        id: String,
        /// The targets it does publish.
        targets: Vec<String>,
    },

    /// A `web-app` administers a database and this home runs none of the engines it named.
    ///
    /// **Refused before the download**, for [`RuntimeError::RuntimeUnresolved`]'s reason: an
    /// install whose stated effect cannot happen is worse than one that never started. Like that
    /// variant, the message names what would satisfy it rather than implying something is broken —
    /// nothing is.
    ///
    /// [`RuntimeError::RuntimeUnresolved`]: crate::RuntimeError::RuntimeUnresolved
    #[error(
        "{id} administers a database and this machine runs none of: {}",
        engines.join(", ")
    )]
    ExtensionNoDatabase {
        /// Which extension.
        id: String,
        /// The engines it can administer, in the order it declared them.
        engines: Vec<String>,
    },

    /// A site naming the php-fpm pool that belongs to an extension.
    ///
    /// **Raised by [`crate::sites`] rather than by the daemon**, because `blueprint.apply` and
    /// `domain.add` reach `sites::update` without going through a CLI, and a refusal they could
    /// cross is no refusal — the rule an extension site's own refusals follow, arriving at a second
    /// field.
    ///
    /// What it protects is the whole reason a `web-app` has a pool of its own: that process holds a
    /// database superuser's password, read from the keyring at spawn, and a project's PHP inside it
    /// could read the same variable.
    #[error("{pool} belongs to the {extension} extension and serves nothing else")]
    ExtensionPoolNotShared {
        /// The pool that was named.
        pool: String,
        /// The extension it belongs to.
        extension: String,
    },

    /// An extension written by a build whose format this one does not know.
    ///
    /// Refused rather than half-read, for [`BlueprintError::UnknownBlueprintSchema`]'s reason: a
    /// manifest whose unknown sections were skipped would install as something other than what its
    /// author wrote down.
    ///
    /// [`BlueprintError::UnknownBlueprintSchema`]: crate::BlueprintError::UnknownBlueprintSchema
    #[error("the extension {id} is schema {schema}, which this build does not read")]
    UnknownExtensionSchema {
        /// The extension's own id, where the file got that far.
        id: String,
        /// The schema it declares.
        schema: u32,
    },

    /// A table that belongs to a different kind of extension.
    ///
    /// Refused rather than ignored: a `[service]` table under `kind = "web-app"` is somebody
    /// who believes their extension will be supervised, and a key silently dropped is a belief
    /// nothing corrects.
    #[error("a {kind} extension has no [{table}] table, and {id} declares one")]
    ExtensionTableUnexpected {
        /// Whose manifest.
        id: String,
        /// What it said it is.
        kind: &'static str,
        /// The table it should not have.
        table: &'static str,
    },

    /// A kind with none of its own table.
    #[error("a {kind} extension needs a [{table}] table, and {id} has none")]
    ExtensionTableMissing {
        /// Whose manifest.
        id: String,
        /// What it said it is.
        kind: &'static str,
        /// The table it is missing.
        table: &'static str,
    },

    /// A field in an `extension.toml` says something the format does not allow.
    ///
    /// **One variant for every such refusal** — an address written out, a path that does not grow
    /// from a placeholder, an unknown placeholder, a stop command, a `web-app` asking for the LAN.
    /// What whoever wrote the file needs is the field and the sentence; a variant per rule would be
    /// a vocabulary to keep in step with the rules rather than with the file.
    #[error("{id}: {field} {reason}")]
    ExtensionField {
        /// Whose manifest.
        id: String,
        /// Which field, spelled the way the file spells it.
        field: String,
        /// What is wrong with it.
        reason: String,
    },

    /// An extension id a compiled-in recipe already claims.
    ///
    /// A `service` extension's process is named by its id, so this one would be two definitions of
    /// one service. Said here rather than discovered when the install writes the row.
    #[error("{id} is the name of a service MixEngine already defines")]
    ExtensionIdTaken {
        /// The id.
        id: String,
    },

    /// A rendered extension manifest is not a usable service.
    ///
    /// Reported against the extension rather than against a spec nobody wrote: the file is the
    /// thing its author can fix.
    #[error("{id} does not describe a service that could run")]
    ExtensionSpec {
        /// Whose manifest.
        id: String,
        /// What `ServiceSpec::validate` said.
        #[source]
        source: mixengine_proto::SpecError,
    },

    /// A `minisign.pub` handed to the generator is not a public key file.
    ///
    /// Distinct from [`Error::RegistryKeyMismatch`] because nothing was compared: `minisign -G`
    /// writes an untrusted comment and one key line, and a file with any other number of them is
    /// one nothing can be said about.
    ///
    /// [`Error::RegistryKeyMismatch`]: crate::ExtensionError::RegistryKeyMismatch
    #[error("{path}: expected an untrusted comment and one key line, found {lines}")]
    RegistryPublicKeyShape {
        /// The file that was read.
        path: String,
        /// How many non-empty lines it held.
        lines: usize,
    },

    /// The key the packaging repository would sign with is not the key this build checks against.
    ///
    /// The load-bearing refusal of the whole publishing chain. A signature made with a key no
    /// installed MixEngine accepts is worse than no signature, because it looks published — so a
    /// half-finished key rotation is a red run rather than a document at a stable URL that nothing
    /// can read. Rotating the index key is an application release: the MixEngine carrying the new
    /// key goes out first.
    #[error(
        "{path} is not the key this build checks against:
    committed: {committed}
    compiled: {compiled}"
    )]
    RegistryKeyMismatch {
        /// The file that was read.
        path: String,
        /// What that file holds.
        committed: String,
        /// [`index::PUBLIC_KEY`], which is what an installed MixEngine verifies against.
        ///
        /// [`index::PUBLIC_KEY`]: crate::index::PUBLIC_KEY
        compiled: &'static str,
    },

    /// A manifest file is not named after the id it declares.
    ///
    /// The one rule the registry generator adds that [`extensions::manifest::read`] cannot have,
    /// because that reads one file and knows nothing about the directory around it. It is also what
    /// makes a repeated id impossible: a directory holds one `mailpit.toml`, so `<id>.toml` is the
    /// roster's uniqueness and not only its tidiness.
    ///
    /// [`extensions::manifest::read`]: crate::extensions::manifest::read
    #[error("{path} declares the id {id}, so it should be named {id}.toml")]
    ExtensionFileName {
        /// The file as it is named.
        path: String,
        /// The id it declares.
        id: String,
    },

    /// A generated registry holds an entry the build that generated it cannot read back.
    ///
    /// The registry reader makes an unreadable entry survivable on a user's machine on purpose,
    /// because an entry a *newer* build published should cost that entry and nothing else. Here it
    /// can only mean the generator is older than its own inputs, and the honest place to stop is
    /// before the signature rather than on somebody's machine.
    #[error("{count} generated entries cannot be read back by the build that made them")]
    RegistryUnreadable {
        /// How many.
        count: usize,
    },
}
