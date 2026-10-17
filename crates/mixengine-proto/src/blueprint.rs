//! Blueprints on the wire: what one is, and what applying one would do.
//!
//! Roadmap task **T77**. **The plan is a value, not a paragraph** — the T77 design, D8. `mix`
//! renders these steps and a graphical client renders the same ones differently; neither decides
//! what is in the list. And the plan is what T78 executes, which is the only way `--dry-run` can
//! promise to match the real run: one place decides the set and the order, and the executor may
//! fail but may not add a step, drop one or reorder them.

use crate::{PackageVersion, RuntimeKind, SiteKind, VersionConstraint};

/// What the signature check found when a blueprint arrived — roadmap task **T79b**.
///
/// **Evidence beside an answer, never the answer.** [`BlueprintSummary::trusted`] is what decides
/// whether a `[scaffold]` command may be offered; this says which of three things happened. A file
/// that came with nothing and a file whose signature did not verify are both untrusted and are not
/// the same event: the second is the one the gallery key exists to catch, and today both reach a
/// person as one sentence.
///
/// Three words rather than four: "signed by another key" is told from "signed by the gallery and
/// then edited" only by the key id inside the `.minisig`, and **a key id is not authenticated** —
/// whoever edits the file edits the key id with it. A message that branched on it would be a
/// message its subject chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SignatureCheck {
    /// One came with it, and it verified against the gallery key.
    Verified,

    /// Nothing came with it — the ordinary case for a blueprint a colleague sent, which is why it
    /// is an answer rather than an error.
    Missing,

    /// One came with it and did not verify: a manifest edited after it was signed, a signature from
    /// another key, or a file that is not a signature at all. The verifier folds the three
    /// together, so this word does too — and a sentence rendered from it has to be true of all
    /// three.
    Rejected,
}

impl SignatureCheck {
    /// Every answer there is.
    pub const ALL: [Self; 3] = [Self::Verified, Self::Missing, Self::Rejected];

    /// The word the `blueprints.signature` column holds.
    ///
    /// One spelling for the column and the wire, on [`BlueprintSource::as_str`]'s rule: a second
    /// one would be a second vocabulary to keep in step.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Missing => "missing",
            Self::Rejected => "rejected",
        }
    }

    /// Read one back, or [`None`] for a word this build does not know.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|check| check.as_str() == value)
    }
}

/// A reason this build cannot read is no reason, rather than a response nobody can decode.
///
/// Roadmap task **T79b**, its design's D3, applied to the wire. This field decorates an answer that
/// travels beside it, so a variant a later phase adds must cost an older client its explanation and
/// nothing else — where a word it cannot read in [`BlueprintSource`], which drives what a plan
/// does, is refused rather than guessed at.
fn signature_check<'de, D>(deserializer: D) -> Result<Option<SignatureCheck>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;

    let word = Option::<String>::deserialize(deserializer)?;

    Ok(word.as_deref().and_then(SignatureCheck::parse))
}

/// One blueprint, as a listing shows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlueprintSummary {
    /// The handle: what a person types, what the row's primary key is, and the file's stem.
    pub slug: String,

    /// The display name.
    ///
    /// Equal to [`Self::slug`] for a captured blueprint. The two are separate because the gallery
    /// (T79) ships blueprints whose title is not a handle, and one column serving both would have
    /// to pick which of the two it lies about.
    pub name: String,

    /// What it is for, or empty.
    pub description: String,

    /// When it was captured, ISO-8601 UTC.
    pub created_at: String,

    /// Where it came from.
    pub source: BlueprintSource,

    /// Whether this build will offer to run the blueprint's own `[scaffold]` command — roadmap task
    /// **T78a**, its design's D1.
    ///
    /// Decided once, by whatever wrote the row: this build's own gallery and this machine's own
    /// captures are trusted, and a blueprint somebody handed over earns it only from a signature
    /// that verified against the gallery key. **Nothing raises it afterwards**, which is what
    /// "untrusted for good" means when it is a column rather than a promise.
    ///
    /// Defaulted, so a record written before this task reads as untrusted — the direction a missing
    /// answer should fall in.
    #[serde(default)]
    pub trusted: bool,

    /// Why [`Self::trusted`] says what it says — roadmap task **T79b**.
    ///
    /// [`None`] where no signature was ever looked for — this build's own gallery and this
    /// machine's own captures — and for a row written before this task, whose reason the schema
    /// never recorded. Absent rather than guessed: an untrusted import from before this column is
    /// *either* of the two, and the answer to which is not on disk anywhere.
    ///
    /// **A record of what arrived, and never a claim about the file on disk now** (the design's
    /// D10). The rendering beside the row is not the artifact that was signed, and nothing
    /// re-checks it — T78a's D16, which is also why trust is a column and not a promise.
    #[serde(
        default,
        deserialize_with = "signature_check",
        skip_serializing_if = "Option::is_none"
    )]
    pub signature: Option<SignatureCheck>,

    /// The rendered file.
    ///
    /// Carried in the listing so that reading the TOML needs no `blueprint.get`: the row is the
    /// truth and this file is its rendering (D7), so pointing at it is the whole of what a
    /// "show me" method would have done.
    pub file: String,
}

/// Where a blueprint came from.
///
/// Three words rather than a boolean, because T78a's trust marking is what reads this: a
/// hand-imported blueprint carries arbitrary scaffold code and stays untrusted for good, and
/// "not built in" would not distinguish it from one captured on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum BlueprintSource {
    /// Shipped with MixEngine and signed.
    Builtin,

    /// Written by `blueprint.capture` on this machine.
    Captured,

    /// Brought in from a file somebody else wrote.
    Imported,
}

impl BlueprintSource {
    /// Every source, in the order a listing would group them.
    pub const ALL: [Self; 3] = [Self::Builtin, Self::Captured, Self::Imported];

    /// The word the `blueprints.source` column holds.
    ///
    /// One spelling for the column and the wire, on [`RuntimeKind::as_str`]'s rule: a second one
    /// would be a second vocabulary to keep in step.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Captured => "captured",
            Self::Imported => "imported",
        }
    }

    /// Read one back, or [`None`] for a word this build does not know.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == value)
    }
}

/// What `blueprint.list` answers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlueprintList {
    /// Every blueprint this home holds, in slug order.
    pub blueprints: Vec<BlueprintSummary>,
}

/// What applying a blueprint would do, decided before anything happens.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlueprintPlan {
    /// Which blueprint, by slug.
    pub blueprint: String,

    /// The name the new project would have, which is also what `{project}` was expanded to.
    pub project: String,

    /// Where it would live.
    pub root: String,

    /// Every action, in the order it would be carried out.
    pub steps: Vec<PlanStep>,

    /// Where the blueprint being applied came from — roadmap task **T78a**, its design's D5.
    ///
    /// Defaulted, so a plan encoded before this task still decodes as what it was: something this
    /// machine captured.
    #[serde(default = "captured")]
    pub source: BlueprintSource,

    /// Whether this build will offer to run the blueprint's `[scaffold]` command.
    ///
    /// **Two facts rather than one**, because they are independent: a gallery file imported by hand
    /// is [`BlueprintSource::Imported`] *and* trusted, and deriving either from the other would be a
    /// build that lies about that case.
    ///
    /// Carried on the plan so that a client — this repository's `mix`, and the graphical one that is
    /// not in it — shows the right words from the answer it already has, rather than making a second
    /// call to find out who wrote the command it is about to offer.
    #[serde(default)]
    pub trusted: bool,

    /// Why [`Self::trusted`] says what it says — roadmap task **T79b**.
    ///
    /// Carried here as well as on [`BlueprintSummary`] because the `[scaffold]` question is where
    /// it matters most: the moment somebody is asked to run a stranger's command is the moment
    /// worth knowing that a signature came with this file and was not the gallery's.
    ///
    /// It changes what is *said* and nothing about what is allowed: one flag still answers for both
    /// kinds of untrusted, and a signature that did not verify is still not a refusal (T78a's D3).
    #[serde(
        default,
        deserialize_with = "signature_check",
        skip_serializing_if = "Option::is_none"
    )]
    pub signature: Option<SignatureCheck>,
}

/// What a plan with no source in it was, which is the only thing it could have been.
fn captured() -> BlueprintSource {
    BlueprintSource::Captured
}

/// What `blueprint.apply` answers.
///
/// **One method, two answers** — T77 argued the single method, because the plan a person reads and
/// the plan the daemon carries out have to be the same list. A tagged union is what lets that
/// survive execution arriving: a client reads which answer it got from the object rather than
/// inferring it from its own request.
// No `Eq`: a [`JobSummary`](crate::JobSummary) carries the job's result, which is a
// `serde_json::Value` and has no total equality. Following it here rather than working around it —
// a type that claimed an equality its field cannot supply would be a type that lies.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum BlueprintApplyResponse {
    /// `dry_run: true` — what applying would do.
    Planned {
        /// The plan.
        plan: BlueprintPlan,

        /// What the releases this plan would install lack on this machine — roadmap task **T152**.
        ///
        /// Composed by the daemon around the plan, never inside it (T78's D9): the plan holds
        /// constraints, and the daemon resolves them against the index to judge. One entry per
        /// distinct requirement, however many steps share it. [`None`] from a daemon before T152, or
        /// when the index could not be read — a dry run is never refused for want of a judgement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        needs: Option<Vec<crate::Requirement>>,
    },

    /// `dry_run: false` — the job carrying it out.
    Started {
        /// The job, as [`JOB_STATUS`](crate::rpc::method::JOB_STATUS) would answer it.
        job: crate::JobSummary,
    },
}

/// What an apply did, as the job's result — roadmap task **T78**.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlueprintApplied {
    /// Which blueprint, by slug.
    pub blueprint: String,

    /// The project it made.
    pub project: String,

    /// Where it lives.
    pub root: String,

    /// One outcome per plan step, in the plan's own order.
    ///
    /// **Every step is here, including the ones that needed nothing**, because that is what makes a
    /// resumed apply legible: a second run whose every line says *already true* is the proof that
    /// the first one finished.
    pub steps: Vec<StepOutcome>,

    /// What is left to do, from the blueprint's `[[next_steps]]` — roadmap task **T205**, D6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_steps: Option<NextSteps>,

    /// The account the database step really used, when there was one — T205, D6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<AppliedDatabase>,
}

/// What kind of thing a person still has to do after an apply — roadmap task **T205**, D4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum NextStepKind {
    /// A command to run one time.
    Once,
    /// A command that keeps running; the site answers while it does.
    Serve,
    /// Something to finish in the browser.
    Open,
}

/// One thing a blueprint says is left to do — roadmap task **T205**, D4.
///
/// **The same type in a manifest and on the wire.** A manifest holds it with `{project}` in it; an
/// answer holds it expanded. Nothing else differs, so one type carries both.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NextStep {
    /// What it is.
    pub kind: NextStepKind,

    /// The command, for `once` and `serve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,

    /// Where in the site, for `open`; starts with `/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// One sentence shown under the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,

    /// The site answers without this step.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,

    /// This step needs the database account the apply created.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub credentials: bool,

    /// Which site it belongs to. Absent in schema 1 and 2; T204a defines it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

/// A project's steps, and whether the blueprint they came from is trusted — T205, D12.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NextSteps {
    /// The blueprint row's own `trusted`.
    pub trusted: bool,

    /// In order; `{project}` already expanded.
    pub steps: Vec<NextStep>,
}

/// The account an apply's `CreateDatabase` step really used, after T202's free-name walk. Never a
/// password.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AppliedDatabase {
    /// The server it lives on.
    pub service: crate::ServiceId,
    /// The database.
    pub database: String,
    /// The account.
    pub user: String,
}

/// One step, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StepOutcome {
    /// What it was.
    pub action: PlanAction,

    /// What happened.
    pub result: StepResult,
}

/// What happened to one step.
///
/// **A step is reported by what became true, not by how many calls it took** (the T78 design, D3):
/// one daemon call can make several steps true at once — `site.create` writes the row, queues the
/// hosts entry and issues the certificate — and the steps that follow it report
/// [`AlreadyTrue`](Self::AlreadyTrue).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum StepResult {
    /// This apply did it.
    Done {
        /// What differs from the plan, in one sentence a client prints under the step — roadmap
        /// task **T202**, D2: the account used when the plan's was somebody else's, a database
        /// that was already there. `None` when nothing does, which is the common case.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },

    /// It was already so.
    AlreadyTrue,

    /// It was not done, and this is why.
    NotRun {
        /// The reason, in the words a client prints.
        why: String,
    },

    /// It ran, and it did not succeed — roadmap task **T78a**, its design's D7.
    ///
    /// **Distinct from [`NotRun`](Self::NotRun)**, which is a step nothing attempted. Only a
    /// `[scaffold]` produces this: every other action in a plan is a daemon call whose failure
    /// fails the apply, while somebody else's command failing leaves a project that works — the
    /// site serves, the database is there — and destroying that to tidy up would be the more
    /// expensive direction to be wrong in.
    ///
    /// The job still succeeds and still carries every step, because a failed job carries an error
    /// and no result: making this the job's failure would throw away the record of the nine steps
    /// that worked. What reads it is the client, which exits non-zero.
    Failed {
        /// What became of it, in the words a client prints: the exit code, and the last thing the
        /// command had to say.
        why: String,
    },
}

/// One action and what this home makes of it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PlanStep {
    /// What would be done.
    pub action: PlanAction,

    /// Whether it needs doing, and what stands in the way.
    pub disposition: Disposition,

    /// Whether carrying it out puts something in the elevation queue — the T77 design, D11.
    ///
    /// **It queues, and a client is what spends the prompt.** This daemon never raises one on its
    /// own initiative (T40b), so an apply enqueues exactly as `site.create` does and the single
    /// prompt comes afterwards, from whoever asked for the apply — corrected here by T78, which is
    /// the task that found the earlier wording promising a prompt this step does not raise.
    ///
    /// A dry run that did not say a password is coming would still have failed to answer the
    /// question it was run to answer; what it says now is *once, at the end, if you say so*.
    pub elevates: bool,
}

/// One thing an apply would do.
///
/// **The values a machine assigns at execution time are not here** (D8): the port a new instance
/// lands on, a generated password, a rowid. A plan that named them would be a plan the executor has
/// to contradict.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum PlanAction {
    /// Register the directory as a project.
    RegisterProject {
        /// Its name.
        name: String,

        /// Its root.
        root: String,

        /// The versions it will ask for — roadmap task **T78**, its design's D7.
        ///
        /// **Without this the blueprint's `[runtimes]` never reaches the machine**: the new site
        /// resolves to whatever this machine defaults to, and a capture of the applied project
        /// comes back with no `[runtimes]` at all, because `blueprints::capture` keeps only what
        /// `resolve` reports as *not* the default.
        ///
        /// It is also what makes a version question mean something: an answer of
        /// [`MismatchAnswer::UseInstalled`](crate::MismatchAnswer) is a different pin, not just a
        /// skipped download.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        pins: std::collections::BTreeMap<RuntimeKind, VersionConstraint>,
    },

    /// Install a language runtime.
    InstallRuntime {
        /// Which language.
        kind: RuntimeKind,

        /// What the blueprint asks for.
        ///
        /// A constraint rather than a version, because the plan reads this home's tables and never
        /// the index (D9): a captured blueprint pins one exact version and a hand-written one may
        /// pin a range, and which release satisfies a range is a question only the index can answer
        /// — at execution time, on the machine doing the installing.
        wanted: VersionConstraint,
    },

    /// Install a service package — roadmap task **T78**, its design's D8.
    ///
    /// **T77 planned an install for languages and none for services**, and `service.create` refuses
    /// with `precondition_failed` when the version it is asked for is not on disk. That is a plan
    /// discovering the impossible five actions into a project directory, which is what a plan exists
    /// to prevent — and it is the ordinary case for the feature's headline scenario, a blueprint
    /// from a teammate's machine.
    ///
    /// **It never carries a question.** A version mismatch is a question about an *instance* that
    /// already exists, asked by [`PlanAction::EnsureService`]; where there is no instance yet there
    /// is nothing to reuse.
    InstallPackage {
        /// The package: `mariadb`, `redis`.
        package: String,

        /// What the blueprint asks for, where it asks for anything.
        ///
        /// [`None`] takes whatever the index offers newest, decided on the machine doing the
        /// installing — [`PlanAction::InstallRuntime`]'s reasoning, and the shape
        /// [`PlanAction::EnsureService::version`] already has.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wanted: Option<VersionConstraint>,
    },

    /// Have a service instance, whether by reusing a shared one or by creating a dedicated one.
    ///
    /// **The package and the instance travel apart rather than as a [`ServiceId`](crate::ServiceId)**, because the id
    /// is formed on the machine that creates the service and a project name is allowed to hold
    /// things an id is not — a project called `My Blog` cannot give its dedicated database the
    /// instance name `My Blog`. The plan still *checks* that the pair can be spelled and blocks the
    /// step when it cannot (D10); what it does not do is pretend to have built an id that would
    /// have been refused.
    EnsureService {
        /// The package: `mariadb`, `redis`.
        package: String,

        /// The instance name it would be created or found under: `main`, or the project's own.
        instance: String,

        /// What the blueprint asks for, where it asks for anything.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<VersionConstraint>,

        /// Whether this instance would belong to the new project alone.
        dedicated: bool,
    },

    /// Create a database and the account that reaches it.
    CreateDatabase {
        /// The package whose instance would hold it.
        package: String,

        /// Which instance, by the name [`PlanAction::EnsureService`] would have used.
        instance: String,

        /// The database name, `{project}` already expanded.
        database: String,

        /// The account name, `{project}` already expanded.
        ///
        /// **Never a password.** The manifest has no key for one, and what an apply generates is
        /// not something a plan can name in advance.
        user: String,
    },

    /// Create the site.
    CreateSite {
        /// What it serves.
        ///
        /// A php-fpm pool is not named here even though [`SiteKind::PhpFpm`] can carry one: which
        /// pool a new site uses is decided when the site is made, on the machine that makes it.
        kind: SiteKind,

        /// Relative to the project root, spelled the way this system spells a path; `""` is the
        /// root itself.
        doc_root: String,

        /// Whether HTTPS is declared.
        https: bool,

        /// The path routes it declares — roadmap task **T135**.
        ///
        /// A php-fpm route's pool is not named here, for the reason `kind`'s is: which pool answers
        /// is decided on the machine that makes the site. Empty for every blueprint the gallery
        /// ships, which is what makes this an addition rather than a change.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        routes: Vec<crate::SiteRoute>,

        /// The services this site links, as their `EnsureService` steps name them — roadmap task
        /// **T204a**, D3. [`None`] is every service the apply made sure of, which is what a
        /// one-site blueprint links.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        services: Option<Vec<crate::ServiceId>>,
    },

    /// Give the site a name.
    AddDomain {
        /// The name, `{project}` already expanded.
        domain: String,

        /// Whether it is the primary.
        primary: bool,
    },

    /// Issue the site's certificate.
    IssueCertificate {
        /// Every name it would cover.
        domains: Vec<String>,
    },

    /// Turn a PHP extension on.
    ///
    /// **This reaches past the project.** Extension choices belong to an installed runtime, so
    /// enabling one changes the PHP that every project on the receiving machine runs — which is why
    /// the renderer says so on the line rather than in documentation nobody reads at that moment.
    ///
    /// There is no "off" direction (D2): a blueprint says what a project needs *loaded*, and
    /// disabling something for everybody else on the machine is harm it was never asked to do.
    SetPhpExtension {
        /// Which installed PHP the project would run when this plan was made: its pin, or the
        /// default where it pins none.
        ///
        /// [`None`] when no PHP here answers that yet — the plan's own runtime step installs it, and
        /// the apply resolves which one it is after that step has run. The apply resolves it in
        /// every case rather than trusting this field, so a plan made before an install is never
        /// carried out against a PHP that is not there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime: Option<PackageVersion>,

        /// The extension's name, as the index spells it.
        name: String,
    },

    /// Run the blueprint's `[scaffold]` command in the new project's directory.
    ///
    /// Capture never writes one. A hand-written or gallery blueprint may, and since roadmap task
    /// **T78a** an apply runs it — but only carrying a
    /// [`ScaffoldConsent`](crate::ScaffoldConsent) that names this exact command. Without one the
    /// step comes back `NotRun` and everything else is applied.
    ///
    /// The command has `{project}` already expanded, because what is shown has to be what runs.
    RunScaffold {
        /// The exact command, shown before anything runs it.
        command: String,
    },

    /// Download a release archive and unpack it into the project directory — roadmap task
    /// **T205**, D2.
    FetchArchive {
        /// `https://` only; the format comes from its suffix.
        url: String,

        /// The archive's single top-level folder whose contents become the project root.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strip: Option<String>,
    },

    /// Append the project database's URL to `.env` under `key` — roadmap task **T205a**. Asked
    /// for (`Confirm`) and answered by [`BlueprintApply::dotenv`](crate::BlueprintApply), apart
    /// from the scaffold's consent. **Never the value**: it is read when the step runs.
    WriteDotenv {
        /// The variable name.
        key: String,

        /// Relative to the project root: `.env`.
        path: String,
    },
}

/// What this home makes of one action.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Disposition {
    /// Already true. The apply does nothing here.
    Satisfied,

    /// It would be done.
    Create,

    /// A version mismatch, which is a question rather than a decision.
    ///
    /// **A client asks it and the request carries the answer** — a daemon has no keyboard, and a job
    /// that stopped halfway to ask one would be a job holding a project directory hostage. See
    /// [`BlueprintApply::answers`](crate::BlueprintApply).
    Choice {
        /// What this machine has, and would use if the answer were "use the installed one".
        installed: PackageVersion,

        /// What the blueprint asks for.
        wanted: VersionConstraint,
    },

    /// Something a person has to agree to before it happens.
    Confirm {
        /// What they would be agreeing to.
        what: String,
    },

    /// It cannot be done.
    ///
    /// Decided **here** rather than five actions into an apply, which is the whole point of a plan
    /// (D10).
    Blocked {
        /// Why, in the words the CLI prints.
        reason: String,
    },

    /// This operating system cannot do it.
    Unsupported {
        /// Why.
        reason: String,
    },
}

#[cfg(test)]
mod tests;
