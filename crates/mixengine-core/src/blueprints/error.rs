//! The ways reading, naming and fetching a blueprint can fail — what `blueprints` reports.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw — roadmap task **T207j**.

/// The ways reading, naming and fetching a blueprint can fail
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BlueprintError {
    /// A blueprint manifest does not parse.
    ///
    /// No path: what failed to parse is the `manifest_toml` column, or a string on its way into it,
    /// and naming the rendered file beside it would point at something that is a copy rather than
    /// the thing that is wrong (D7).
    #[error("that blueprint is not a manifest this build can read")]
    BlueprintManifest {
        /// The parse failure, which carries the line and the column.
        #[source]
        source: toml::de::Error,
    },

    /// A blueprint was written by a build whose format this one does not know.
    ///
    /// Refused rather than half-read: a manifest whose unknown sections were skipped would apply as
    /// something other than what its author wrote down.
    #[error("the blueprint {name} is schema {schema}, which this build does not read")]
    UnknownBlueprintSchema {
        /// The blueprint's own name, where the file got that far.
        name: String,
        /// The schema it declares.
        schema: u32,
    },

    /// A project's php-fpm sites run more than one PHP, and a blueprint has one — roadmap task
    /// **T204a**, D6.
    ///
    /// Refused rather than reduced: writing one version down would change what the other sites run
    /// on the machine that applies it. The sites are named because "two PHPs" sends somebody
    /// hunting.
    #[error(
        "{project} serves its sites with more than one PHP ({}), and a blueprint has one",
        sites
            .iter()
            .map(|(domain, version)| format!("{domain} on {version}"))
            .collect::<Vec<_>>()
            .join(", ")
    )]
    ProjectRunsSeveralPhps {
        /// The project's name.
        project: String,
        /// Each php-fpm site's primary domain, and the PHP its pool runs.
        sites: Vec<(String, String)>,
    },

    /// A blueprint name is not a slug, and a slug is what a filename stem can be made of.
    ///
    /// The refusal is the security boundary: the name is joined onto `blueprints/`, so `../../x`
    /// would write outside the home. Nothing about the join makes it safe — this does.
    #[error("{name} cannot be a blueprint name: {reason}")]
    InvalidBlueprintName {
        /// What was asked for.
        name: String,
        /// Which rule it broke, in the words the user is shown.
        reason: &'static str,
    },

    /// Something is already filed under that name.
    #[error("a blueprint called {name} is already here")]
    BlueprintExists {
        /// The slug.
        name: String,
    },

    /// A `blueprints` row holds a source word this build does not know.
    ///
    /// Unreachable through our own writes, so it means a hand-edited database or a row written by a
    /// build that knew a fourth source — and answering a listing with a guess about where a
    /// blueprint came from is the wrong direction to be wrong in, since T78a's trust marking is
    /// what reads it.
    #[error("the blueprint {name} is stored as {value}, which is not a source")]
    UnknownBlueprintSource {
        /// The slug.
        name: String,
        /// What is in the column.
        value: String,
    },

    /// A blueprint's signature does not verify against the gallery key — roadmap task **T78a**.
    ///
    /// Not a refusal on its own: an import whose signature does not check out lands *untrusted*
    /// rather than being thrown away, because a stale signature is a fact about the file and not a
    /// reason to lose it. The variant exists so that what happened can be said, in the log and in a
    /// client's message, rather than folded into a silent `false`.
    #[error("this blueprint is not signed by the gallery key")]
    BlueprintSignature {
        /// What the verifier said.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The gallery key compiled into this binary is not a public key.
    ///
    /// [`InstallError::IndexKey`]'s twin: a broken build and nothing a user can act on, reported
    /// rather than unwrapped because nothing in this crate panics.
    ///
    /// [`InstallError::IndexKey`]: crate::InstallError::IndexKey
    #[error("this build's blueprint gallery key is not a valid minisign key")]
    BlueprintKey {
        /// What the parser said.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A blueprint's archive is past the ceiling an apply downloads — roadmap task **T205**, D2.
    ///
    /// Its own variant rather than [`InstallError::ArtifactTooLarge`], whose sentence is about a
    /// signed index: an archive has no index, and the number here is this build's, not a
    /// publisher's.
    ///
    /// [`InstallError::ArtifactTooLarge`]: crate::InstallError::ArtifactTooLarge
    #[error("{url} is larger than the {limit} bytes a blueprint's archive may be")]
    ArchiveTooLarge {
        /// The address.
        url: String,
        /// The ceiling.
        limit: u64,
    },

    /// An archive's suffix names no format this build unpacks — roadmap task **T205**, D2.
    #[error("{url} is not a .zip, .tar.gz or .tar.zst, so this build cannot unpack it")]
    ArchiveFormatUnknown {
        /// The address.
        url: String,
    },

    /// `strip` names a folder the archive does not have at its top level — roadmap task **T205**.
    #[error("{url} has no top-level folder {strip}; it has: {}", found.join(", "))]
    ArchiveStripMissing {
        /// The address.
        url: String,
        /// What the blueprint said.
        strip: String,
        /// What the archive's top level holds.
        found: Vec<String>,
    },
}
