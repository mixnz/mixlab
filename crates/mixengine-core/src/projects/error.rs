//! The ways a project, its manifest, its sites and their domains can be wrong.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw — roadmap task **T207j**.

use std::path::PathBuf;

/// The ways a project, its manifest, its sites and their domains can be wrong.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProjectError {
    /// A `mixengine.toml` in the user's repository does not parse.
    ///
    /// [`Error::Config`]'s sibling one directory out, and refused for the same reason: an unknown
    /// key inside `[runtimes]` is a pin naming a language MixEngine does not manage, which would do
    /// nothing at all while looking exactly like a pin that does not work. Unknown *sections* are
    /// allowed through — the file also declares a site and its services, which are Phase 4's.
    ///
    /// [`Error::Config`]: crate::Error::Config
    #[error("{} is not a valid project manifest", path.display())]
    Manifest {
        /// The manifest that failed to parse.
        path: PathBuf,
        /// The parse failure, which carries the line, the column and the accepted keys.
        #[source]
        source: toml::de::Error,
    },

    /// A `mixengine.toml` that parses but could not be edited in place.
    ///
    /// [`Error::Manifest`]'s sibling on the write path, and separate from it because the two are
    /// different accusations: the first says the user's file is wrong, and this says this build
    /// could not put something into a file that is right. The reason is carried as text rather than
    /// as the editor's own error type, so the shape of a dependency does not become part of this
    /// enum.
    ///
    /// [`Error::Manifest`]: crate::ProjectError::Manifest
    #[error("{} could not be edited: {reason}", path.display())]
    ManifestEdit {
        /// The manifest that could not be edited.
        path: PathBuf,
        /// What the editor said about it.
        reason: String,
    },

    /// A directory that has to be absolute was not.
    ///
    /// Version resolution walks upwards from it, so a relative path would be walked from wherever
    /// the *daemon* was started — a directory belonging to nobody's project, silently producing a
    /// plausible answer about the wrong tree.
    #[error("{} is not an absolute directory", path.display())]
    NotAbsolute {
        /// What was offered.
        path: PathBuf,
    },

    /// A `projects` row holds a value this build cannot read back.
    ///
    /// [`RuntimeError::UnreadableRuntimeRow`]'s sibling, and reached through the same two doors: a database
    /// edited by hand, or a row written by a build that knew more than this one. `runtime_pins_json`
    /// is TEXT with no `CHECK` available to it, so the reader is the only thing that refuses — but
    /// only for the language being asked about, since a pin naming a fifth one must not stop this
    /// build resolving PHP.
    ///
    /// [`RuntimeError::UnreadableRuntimeRow`]: crate::RuntimeError::UnreadableRuntimeRow
    #[error("the project at {root} has a {column} this build cannot read: {value}")]
    UnreadableProjectRow {
        /// The project's root directory, which is how a person finds the row.
        root: String,
        /// Which column.
        column: &'static str,
        /// What is in it.
        value: String,
    },

    /// A project name that cannot be one.
    ///
    /// Refused rather than corrected, because a name is a handle: it is typed on a command line,
    /// shown in a listing, and T39a takes a site's default domain from it — so a name silently
    /// changed on the way in is a name that does not work where the user next types it.
    #[error("{name} cannot be a project name: {because}")]
    InvalidProjectName {
        /// What was offered.
        name: String,
        /// Which rule it broke, as a phrase finishing "cannot be a project name: …".
        because: &'static str,
    },

    /// A domain that cannot be one.
    ///
    /// Refused rather than corrected, on [`Error::InvalidProjectName`]'s reasoning: a domain is
    /// typed into a browser, and one silently changed on the way in is one that does not work
    /// where the user next types it. Case is the single exception — DNS has never been
    /// case-sensitive, so lowercasing is normalisation rather than correction.
    ///
    /// [`Error::InvalidProjectName`]: crate::ProjectError::InvalidProjectName
    #[error("{domain} cannot be a domain: {because}")]
    InvalidDomain {
        /// What was offered.
        domain: String,
        /// Which rule it broke, as a phrase finishing "cannot be a domain: …".
        because: &'static str,
    },

    /// A TLD this home does not manage.
    ///
    /// Public suffixes are refused rather than served: `.dev` and `.app` are HSTS-preloaded, so a
    /// site on one would be a browser refusing plain HTTP before any of this was consulted.
    #[error("{domain} is on .{tld}, which is a public TLD this home will not answer for")]
    UnmanagedTld {
        /// What was offered.
        domain: String,
        /// The last label, without its dot.
        tld: String,
    },

    /// `.local`, without the acknowledgement it needs.
    ///
    /// mDNS territory (RFC 6762): it works until somebody plugs in a printer. Allowed, because a
    /// person who knows that is entitled to it — and never by default.
    #[error("{domain} is on .local, which belongs to mDNS")]
    RiskyTld {
        /// What was offered.
        domain: String,
    },

    /// A domain another site already answers to.
    ///
    /// `site_domains_domain` is `UNIQUE` — one domain is one site, primary or alias — and this
    /// names the site holding it, which the index cannot. Without the name the answer would be
    /// "taken" with nowhere to go and look.
    #[error("{domain} already belongs to {holder}")]
    DomainTaken {
        /// The domain that is claimed.
        domain: String,
        /// The primary domain of the site holding it.
        holder: String,
    },

    /// The only domain a site has.
    ///
    /// `0001_initial.sql` records "at least one" as an invariant this layer upholds, because SQLite
    /// has no deferred constraint to express it with. This is that invariant being upheld: a site
    /// with no name is one nothing can reach and nothing can render.
    #[error("{domain} is the only domain its site has")]
    LastDomain {
        /// The domain that was to go.
        domain: String,
    },

    /// A site's primary domain, which `domain.remove` does not get to change.
    ///
    /// The primary decides the site's canonical URL and, from phase 5, the name on its certificate.
    /// Promoting another domain in its place would change what the site *is* under a verb that says
    /// "remove a domain" — a larger act than the one asked for (T46 design, D3).
    #[error("{domain} is its site's primary domain")]
    PrimaryDomain {
        /// The domain that was to go.
        domain: String,
    },

    /// A doc root that is not inside the project it belongs to.
    ///
    /// Refused rather than stored: a site whose files are outside its project's root is one no
    /// renderer can express, and `project.update { root }` would move the project out from under
    /// it.
    #[error("{doc_root} is not inside {root}")]
    DocRootOutsideProject {
        /// What was offered.
        doc_root: String,
        /// The project's root, as the filesystem spells it.
        root: String,
    },

    /// A site's `https_redirect` cannot be `true` on a site whose `https_enabled` is, or is
    /// becoming, `false` — roadmap task **T98**.
    ///
    /// `sites.https_redirect`'s own `CHECK` already refuses the row; this is the message a
    /// caller sees before that, in words about what was asked for rather than about the column that
    /// would have refused it.
    #[error("https_redirect needs https enabled first")]
    HttpsRedirectNeedsHttps,

    /// A `sites` row this build cannot read.
    #[error("the {column} of site {site} is not something this build can read: {value}")]
    UnreadableSiteRow {
        /// The rowid, which is the only handle a broken row has.
        site: i64,
        /// Which column.
        column: &'static str,
        /// What was in it.
        value: String,
    },

    /// A directory that is already a project.
    ///
    /// `projects.root_path` is `UNIQUE` — one directory is one project — and this names the project
    /// holding it, which the unique index cannot. A root *inside* another project's root is not
    /// this: the walk takes the nearest, so nesting has a defined answer.
    #[error("{root} is already the project {holder}")]
    ProjectRootTaken {
        /// The directory, spelled the way the filesystem spells it.
        root: String,
        /// The project that got there first.
        holder: String,
    },

    /// A project name that is already registered.
    ///
    /// The other unique column, and the one whose repair is different: a name is not freed by
    /// moving a directory, only by renaming or deleting the project that holds it.
    #[error("a project called {name} is already registered")]
    ProjectNameTaken {
        /// The name that is taken.
        name: String,
    },
}
