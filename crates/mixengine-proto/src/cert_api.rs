//! What `cert.ca_status` answers: this home's certificate authority, and nothing about the machine.
//!
//! **There is no trust-store field in [`Ca`], and that is a decision rather than an omission.**
//! Whether an operating system trusts this certificate is a question about the operating system,
//! answered beside it in [`CaStatus::trust`]. [`DnsStatus`](crate::DnsStatus) took the same shape
//! for the same reason: report the independent facts, and refuse to collapse them into a verdict.
//!
//! **And there is nowhere here a private key could travel.**
//! `docs/architecture/security-model.md` says the key is never copied, exported by an RPC, or
//! sent to a client, and the way that stays true is that no type below has a field to put one in.

use crate::{SiteRef, Timestamp};

/// `cert.ca_status` takes no options, and says so in a type.
///
/// An empty struct with `deny_unknown_fields` rather than no parameters at all: a caller who
/// misspells something is told, instead of being silently given the default — the reasoning every
/// parameterless method follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CaStatusQuery {}

/// This home's certificate authority, as far as the daemon can see it.
///
/// **A struct around one enum, and the enum is flattened into it.** The wrapper is what lets
/// [`trust`](Self::trust) say whether the machine's stores trust this certificate without turning a
/// tagged enum into something a client has to unwrap; the `flatten` is what stops the tag — which
/// is also called `state` — from arriving nested inside a field of the same name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CaStatus {
    /// What is there.
    #[serde(flatten)]
    pub state: CaState,

    /// Whether this machine trusts it.
    ///
    /// **Beside [`Ca`] rather than inside it**, because whether a machine trusts a certificate is a
    /// fact about the machine and `Ca` describes the certificate. A home whose authority is absent
    /// or damaged still has a machine with or without a store, and this still has something true to
    /// say about it.
    pub trust: Trust,

    /// What Firefox and Chrome say.
    ///
    /// **Beside [`trust`](Self::trust) and not inside it.** They are separate questions with
    /// separate answers: `trust` is one store and one `bool`, and this is N databases that are
    /// orthogonal to it. A machine can hold the authority in `/etc/ssl/certs` and in none of its
    /// browsers, which is an ordinary state rather than a contradiction.
    pub browsers: Browsers,
}

/// What the browsers on this machine say about MixEngine's authority.
///
/// **Four states, mirroring [`Trust`]'s** — and, as there, every branch that could not ask says why
/// rather than reporting `false`. A client renders what the daemon returns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Browsers {
    /// The tool is here, and this is what each database said.
    ///
    /// **An empty list is a machine with no browser profiles**, which is a normal server and not a
    /// failure — and a different answer from [`NoTool`](Self::NoTool), which is a machine that may
    /// well have browsers nobody could ask about.
    Reached {
        /// One per database found.
        databases: Vec<BrowserDatabase>,
    },

    /// `certutil` is not installed, so nothing was asked.
    ///
    /// The reason names `libnss3-tools`, because "certutil not found" sends a person to a search
    /// engine where the package name ends the question.
    NoTool {
        /// In words, naming the package.
        because: String,
    },

    /// Not a system MixEngine searches.
    ///
    /// Windows and macOS. The reason says what MixEngine did rather than what Firefox reads there:
    /// that depends on `security.enterprise_roots` and is not measured.
    NotSearched {
        /// In words.
        because: String,
    },

    /// The search itself failed.
    ///
    /// As [`Trust::Unknown`]: a read that said nothing must not be rendered as "no".
    Unknown {
        /// What went wrong.
        because: String,
    },
}

/// One NSS database, and whether it holds the authority.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BrowserDatabase {
    /// The directory, so a person can go and look at it.
    pub path: String,

    /// What put it there: `Firefox`, `Firefox (snap)`, `Chrome and Chromium`.
    ///
    /// **Which browser and not just which path**, because what a person does about a database that
    /// lacks it is restart the browser that owns it.
    pub owner: String,

    /// Whether it holds exactly this authority.
    pub installed: bool,

    /// Why not, or why this one could not be asked.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub because: Option<String>,
}

/// Whether a site has a usable certificate.
///
/// **[`CaState`]'s vocabulary, reused deliberately.** The ways a key and a certificate on disk can
/// disagree do not depend on which of the two they are, so [`Unusable`] is the same closed enum
/// rather than a second one that would drift from it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CertState {
    /// This site has no certificate.
    Absent {},

    /// There is one, and it parses.
    ///
    /// **Including one that has expired** — see [`SiteCert::days_left`].
    Present {
        /// What it is.
        cert: SiteCert,
    },

    /// There is something, and it cannot be used.
    Unusable {
        /// Which of the ways, as a name rather than a sentence.
        because: Unusable,
    },
}

/// One site's certificate.
///
/// **There is no `certificate_pem` and there is no field a private key could travel in.** [`Ca`]
/// carries its PEM because a client installs it; nothing installs a leaf, so the field would be
/// surface with no caller — and `docs/architecture/security-model.md`'s guarantee is easier to
/// keep on a type with fewer fields.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SiteCert {
    /// The distinguished name, as X.509 spells it.
    pub subject: String,

    /// Every name it covers, in the order it covers them.
    pub sans: Vec<String>,

    /// The authority that signed it, as X.509 spells the name.
    ///
    /// **The fourth question the issuer asks is a comparison of this against the authority's own
    /// subject** (`docs/specs/2026-08-25-t50-leaf-issuance-design.md`, D6), which is free because
    /// the authority carries its key's identity in that name. On the wire rather than derived only
    /// where it is needed, because `mix cert status` has to be able to show a person *which*
    /// authority signed a certificate they cannot get a padlock for.
    pub issuer: String,

    /// SHA-256 of the certificate's DER, lowercase hex, no separators.
    pub fingerprint: String,

    /// When it starts being valid.
    pub not_before: Timestamp,

    /// When it stops.
    pub not_after: Timestamp,

    /// Whole days from now until [`not_after`](Self::not_after).
    ///
    /// **Signed, and allowed to be negative**, for [`Ca::days_left`]'s reason: an expired
    /// certificate is a true statement about one that exists and parses.
    pub days_left: i64,
}

/// `cert.issue` — give a site the certificate its names need, or every site one.
///
/// **It names a site and never a list of domains.** `docs/features/tls.md` specified
/// `{ domains }`; that would put in the client the decision of *what a certificate covers*, which is
/// business logic, and `CLAUDE.md`'s first rule is that a client only renders what the
/// daemon returns. The daemon reads the site's domains from its own rows.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CertIssue {
    /// One site, or **every site with HTTPS declared** when it is absent.
    ///
    /// The absent form is what the daemon's own producer calls and what `mix cert issue` runs with
    /// no argument, so the automatic path and the manual one are one piece of code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<SiteRef>,
}

/// What `cert.issue` did, per site.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CertIssueReport {
    /// One entry per site considered, in primary-domain order.
    pub sites: Vec<SiteCertOutcome>,
}

/// One site, and what happened to its certificate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SiteCertOutcome {
    /// The site, by its primary domain.
    pub domain: String,

    /// What was done.
    pub outcome: IssueOutcome,

    /// What is on disk afterwards.
    pub state: CertState,
}

/// What `cert.status` was asked about.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CertStatusQuery {
    /// One site, by any of its domains. Every site when this is left out.
    pub site: Option<SiteRef>,
}

/// What this home's certificates are doing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CertStatusReport {
    /// One entry per site considered, in primary-domain order.
    pub sites: Vec<SiteCertStatus>,
}

/// One site: what is on disk, what the server presents, and what is wrong if anything is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SiteCertStatus {
    /// The site, by its primary domain.
    pub domain: String,

    /// Every name this site declares, in the order it declares them.
    pub domains: Vec<String>,

    /// The certificate in `certs/sites/`.
    pub disk: CertState,

    /// The certificate the running front end presents for this name.
    pub handshake: Handshake,

    /// The one condition worth acting on, if there is one.
    pub problem: Option<CertProblem>,
}

/// A condition of one site's certificate.
///
/// **A name for a condition rather than advice**, which is [`ProblemId`](crate::ProblemId)'s own
/// words about itself and holds here unchanged: `mix` renders the command to run and a graphical
/// client renders a button. A daemon that answered with the command string would be sending a GUI
/// a sentence telling its user to open a terminal.
///
/// **Not [`ProblemId`](crate::ProblemId) itself.** Those name conditions of the *machine* that `mix
/// doctor` repairs, and merging would mean every one of these needs an entry in the daemon's repair
/// table — while [`Self::ServedCertificateDiffers`] is repaired by reloading a front end rather
/// than by touching a certificate at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CertProblem {
    /// There is no usable certificate on disk for this site.
    NoCertificate,

    /// The certificate on disk does not cover the names this site declares.
    ///
    /// The most common "the padlock broke" report there is: a domain was added and the certificate
    /// was never replaced.
    NamesDiffer,

    /// Nothing is serving this site over TLS.
    NotServed,

    /// The server is presenting a certificate other than the one on disk.
    ///
    /// **The report this task was built for.** A certificate was replaced and the running server
    /// still holds the old one in memory, so everything that reads files calls the machine healthy
    /// while every browser refuses it.
    ServedCertificateDiffers,

    /// The presented chain does not validate against this home's authority.
    NotTrusted,

    /// There is a certificate, it is being served, and it runs out soon.
    Expiring,
}

/// What this home's front end presented when asked for a site over TLS.
///
/// **The first answer in this crate that comes from a socket rather than from a file.** Everything
/// else said about a certificate here is read off disk and believed; this is what the running
/// server actually hands a client, which is the only thing a browser ever sees.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "handshake", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Handshake {
    /// This site declares no HTTPS, so nothing was attempted.
    ///
    /// **Not a failure and not a problem**, which is the distinction [`IssueOutcome::NotWanted`]
    /// draws for the same reason: a site that asked for no certificate is not a site whose
    /// certificate is missing.
    NotAsked {},

    /// Nothing answered, and this is why: no front end in this home, or a port nothing holds.
    NotServed {
        /// In words.
        because: String,
    },

    /// Something answered and TLS did not complete.
    Failed {
        /// In words, from the TLS library.
        because: String,
    },

    /// A certificate was presented.
    Presented {
        /// Its leaf, described exactly as the file on disk is — by the same function, so that the
        /// two are comparable by construction rather than by agreement.
        cert: SiteCert,

        /// Whether it validates against this home's own authority.
        trust: Verdict,
    },
}

/// Whether a presented chain validates.
///
/// **An enum rather than a `bool` with a sentence beside it**, on [`CaState`]'s shape: the pair has
/// two states nobody means — rejected with no reason, and trusted with one — and this makes every
/// branch that can say why say it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "trust", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Verdict {
    /// It chains to this home's authority and covers the name it was asked for.
    Trusted {},

    /// It does not, and this is what the verifier said.
    Rejected {
        /// In words, from the TLS library.
        because: String,
    },
}

/// The three things `cert.issue` can do to one site.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum IssueOutcome {
    /// A key and a certificate were written.
    Issued {},

    /// What was already there answered every question, so nothing was written.
    Reused {},

    /// Nothing was written because nothing was asked for: this site declares no HTTPS.
    ///
    /// **Not a refusal, and the distinction is not cosmetic.** A refusal is MixEngine failing at
    /// something it was asked to do; this is MixEngine correctly doing nothing. The renewal loop
    /// announces every failure it finds, so with these two under one name it would announce one per
    /// plaintext site, once an hour, for as long as the daemon runs — and
    /// `crate::sites::Sites::now_has_a_certificate` was already logging `the site has no
    /// certificate yet` about a site that never wanted one.
    NotWanted {
        /// In words.
        because: String,
    },

    /// Nothing was written, and this is why: no usable authority, or no domains at all.
    ///
    /// **A per-site outcome and not a failed call.** One site that cannot be issued for must not
    /// take the answer for the others with it, which is the same shape `BrowserChange` has.
    ///
    /// A site with no domains stays here rather than moving to [`Self::NotWanted`]: it is a row
    /// that should not exist, and calling it "not wanted" would hide it.
    Refused {
        /// In words.
        because: String,
    },
}

/// Whether this machine holds MixEngine's certificate authority in its own trust store.
///
/// **This says "is it in the store", not "does a browser trust it".** Firefox and Chrome on Linux
/// read NSS databases and not the system store at all — that is [`CaStatus::browsers`] — and a
/// browser already running may not re-read a store it has cached. The honest end-to-end answer is a
/// live TLS handshake, which is `mix cert status`' job.
/// **A nested object rather than a flattened one, unlike [`CaState`] above.** Two reasons, and both
/// are about this type rather than about taste: `Trust::NotInstalled` and `CaState::Unusable` both
/// spell their reason `because`, so flattening would let one silently overwrite the other on the one
/// screen that has to say what is wrong; and the two are about different subjects — the certificate,
/// and the machine — where `CaStatus`' single field was about one. A client reads
/// `status.trust.state`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Trust {
    /// This machine holds it.
    Installed {
        /// Which store, in words a person can go and look in.
        store: String,
    },

    /// This machine has a store MixEngine knows how to write, and it does not hold it.
    NotInstalled {
        /// Which store, and why it is not there — `mix doctor` reports the same sentence.
        because: String,
    },

    /// This machine has no system trust store MixEngine knows how to write.
    ///
    /// **A supported machine, not a failure**, exactly as `ResolverMethod::None` is: Linux without
    /// either anchors directory keeps working over HTTP, and its browsers are reached through NSS
    /// rather than through a system store at all.
    NoStore {
        /// Which of those it is, in words.
        because: String,
    },

    /// The store could not be read.
    ///
    /// A real outcome and the only honest thing to print when it happens — a read that failed has
    /// said nothing, and rendering that as "not installed" would be the client inventing an answer.
    Unknown {
        /// What went wrong reading it.
        because: String,
    },
}

/// Whether this home has a usable certificate authority.
///
/// **Internally tagged**, so a client matches on a word rather than working out which fields
/// arrived.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CaState {
    /// This home has no certificate authority.
    ///
    /// Reachable even though the daemon makes one at start
    /// (`docs/specs/2026-08-24-t48-internal-ca-design.md`, D4): a start whose generation failed
    /// warns and carries on, and this is what the next question gets.
    ///
    /// An empty struct variant rather than a unit one, for [`Outcome`](crate::Outcome)'s reason:
    /// `deny_unknown_fields` never fires on a unit variant of an internally tagged enum, because it
    /// is read through `deserialize_any`.
    Absent {},

    /// There is one, and it parses.
    ///
    /// **Including one that has expired** — see [`Ca::days_left`].
    Present {
        /// What it is.
        ca: Ca,
    },

    /// There is something, and it cannot be used.
    Unusable {
        /// Which of the ways, as a name rather than a sentence.
        because: Unusable,
    },
}

/// The ways a certificate authority on disk can be unusable.
///
/// **Closed rather than a string**, for [`ProblemId`](crate::ProblemId)'s reason: a client that
/// matches on wording is a client that silently stops matching. Nothing here is advice — what to do
/// about each is the client's to say, and for most of them it is `mix cert ca-rotate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Unusable {
    /// The certificate is there and its private key is not.
    KeyMissing,

    /// The private key is there and the certificate is not.
    ///
    /// The state a crash between the two writes leaves, which is why the key is written first
    /// (`docs/specs/2026-08-24-t48-internal-ca-design.md`, D2): this is a shape that can be
    /// recognised, where a certificate whose key never arrived looks exactly like one whose key was
    /// lost.
    CertificateMissing,

    /// The private key is there and is not a private key this build can read.
    KeyUnreadable,

    /// The certificate is there and is not a certificate at all.
    CertificateUnreadable,

    /// Both are there, both parse, and the certificate is not this key's.
    ///
    /// How a home restored from a backup that caught one file and not the other comes back. Left
    /// unchecked the symptom appears much later and somewhere else, as leaf certificates that no
    /// browser trusts.
    KeyAndCertificateDisagree,
}

/// A certificate authority that exists and parses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Ca {
    /// The distinguished name, as X.509 spells it.
    pub subject: String,

    /// SHA-256 of the certificate's DER, lowercase hex, no separators.
    ///
    /// **The number a browser shows and a trust store lists**, and therefore the only one a person
    /// can compare against anything. Not the same value as [`key_id`](Self::key_id), and
    /// `docs/specs/2026-08-24-t48-internal-ca-design.md`, D1, is why there are two.
    pub fingerprint: String,

    /// The first eight hex characters of the SHA-256 of the public key, which is what
    /// [`subject`](Self::subject) ends with.
    ///
    /// A certificate cannot carry a hash of itself — the subject is inside the bytes the hash is
    /// over — so the name is derived from the key instead. That also makes two certificates over
    /// one key recognisable as one authority, which is what a rotation of the certificate alone
    /// would produce.
    pub key_id: String,

    /// When it starts being valid.
    pub not_before: Timestamp,

    /// When it stops.
    pub not_after: Timestamp,

    /// Whole days from now until [`not_after`](Self::not_after).
    ///
    /// **Signed, and allowed to be negative.** An expired authority is a true statement about a
    /// certificate that exists and parses, not an unusable one: what it needs is rotation, which is
    /// `cert.ca_rotate`'s.
    pub days_left: i64,

    /// The certificate itself, PEM. The public half, and there is no field for the other one.
    pub certificate_pem: String,
}

/// `cert.ca_rotate` takes no options, and says so in a type — [`CaStatusQuery`]'s reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CaRotateQuery {}

/// `cert.ca_uninstall` takes no options, and says so in a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CaUninstallQuery {}

/// What `cert.ca_rotate` did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CaRotateReport {
    /// What happened.
    ///
    /// **Flattened**, for [`CaStatus::state`]'s reason: the tag is also called `outcome`, and
    /// without this it would arrive nested inside a field of the same name.
    #[serde(flatten)]
    pub outcome: RotateOutcome,

    /// The authority that was replaced.
    ///
    /// [`None`] for a home whose authority was damaged: [`CaState::Unusable`] carries no [`Ca`],
    /// because there was nothing parseable to describe — and that is a state a rotation repairs
    /// rather than refuses. It is also the one case where the old certificate is left in the trust
    /// store, since nothing may be removed that cannot be named by key-id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Ca>,

    /// Where this home and this machine stand now, **measured after the grant rather than reported
    /// by it**.
    ///
    /// The same [`CaStatus`] `cert.ca_status` answers, produced by the same code, so the two cannot
    /// come to disagree about what reading a trust store means. A rotation that did not commit
    /// reports the *old* authority here, because that is what is there.
    pub status: CaStatus,

    /// One entry per site whose certificate was reissued, exactly `cert.issue`'s.
    ///
    /// Empty when nothing was committed. A rotation reissues through the call `mix cert issue`
    /// already runs, because a leaf signed by a superseded authority is never reused — so there is
    /// no second reissue path to keep in step with the first.
    pub sites: Vec<SiteCertOutcome>,
}

/// What became of a rotation.
///
/// **Internally tagged and `#[non_exhaustive]`**, as [`CertProblem`] is: a client matches on a word,
/// and a later build may find a fourth thing to say.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum RotateOutcome {
    /// The authority was replaced, and every store that could be reached was updated.
    Rotated {},

    /// **Nothing was changed**, and this is why.
    ///
    /// The candidate was generated, permission was asked for, and the reading afterwards said this
    /// machine would trust the old authority and not the new one. Committing there would have left
    /// every site serving a certificate no browser accepts, so the candidate was thrown away.
    NotCommitted {
        /// What the reading said, in words.
        because: String,
    },

    /// This home has no certificate authority to replace.
    NothingToRotate {
        /// Why not, and what to run instead.
        because: String,
    },
}

/// What `cert.ca_uninstall` did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CaUninstallReport {
    /// What happened. Flattened, as [`CaRotateReport::outcome`] is.
    #[serde(flatten)]
    pub outcome: UninstallOutcome,

    /// Where this machine stands now, measured after.
    ///
    /// **What is left is read here and stated nowhere else.** [`CaStatus::trust`] is
    /// [`Trust::Installed`] when the system store still holds it, and [`CaStatus::browsers`] lists
    /// each database and whether it does. A second field restating that would be a second answer
    /// that can disagree with the first; [`UninstallOutcome`] carries the *reason*, which a
    /// measurement genuinely cannot.
    pub status: CaStatus,
}

/// What became of a removal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UninstallOutcome {
    /// Every store that could be reached no longer holds it.
    Removed {},

    /// Some store still does, and this is why.
    ///
    /// **Not a failure.** Each store is independent, so taking the authority out of Firefox is a
    /// complete action on Firefox whatever the system store did — which is why a declined elevation
    /// prompt still leaves the browser databases cleaned. A rotation cannot take that shape, and
    /// [`RotateOutcome`] has no equivalent: a home with a new authority and half the machine
    /// trusting the old one serves leaves nobody accepts.
    PartlyRemoved {
        /// Which store still holds it, and why it was not taken out.
        because: String,
    },

    /// This home has no authority to take out of anything.
    NothingToRemove {
        /// Why not.
        because: String,
    },
}

#[cfg(test)]
mod tests;
