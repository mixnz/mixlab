//! Requests in the `blueprint.*` namespace — roadmap task **T77**.

use crate::{ProjectRef, RuntimeKind, ServiceId};

/// `blueprint.capture` — write down what a project is made of.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlueprintCapture {
    /// Which project. The CLI fills this from the current directory when nobody named one.
    pub project: ProjectRef,

    /// The slug to file it under, which becomes the row's key and the rendered file's stem.
    pub name: String,

    /// What it is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether to replace a blueprint already filed under this slug.
    ///
    /// Refusing by default is what stops a second capture quietly overwriting the first. There is
    /// no `blueprint.delete` in this build, so without this flag a mistyped name would be
    /// permanent — which is a worse default than asking.
    #[serde(default)]
    pub overwrite: bool,
}

/// `blueprint.import` — take in a blueprint somebody else wrote.
///
/// **The only thing in this build that can produce [`BlueprintSource::Imported`]**, and therefore
/// the only thing that can produce an untrusted one — roadmap task **T78a**, its design's D3. A
/// capture is this machine's own and the gallery is this build's own; everything else arrives
/// through here, and what it arrives with decides whether its `[scaffold]` will ever be offered.
///
/// [`BlueprintSource::Imported`]: crate::BlueprintSource
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlueprintImport {
    /// The manifest to read. Absolute; the client resolves it against its own current directory,
    /// as every other path in this API is resolved.
    pub path: String,

    /// A detached minisign signature to check it against.
    ///
    /// [`None`] looks for `<path>.minisig` — the name minisign gives it, so somebody handed a
    /// signed pair has it on disk already — and uses that if it is there.
    ///
    /// **A signature that does not verify is not a refusal**: the blueprint lands untrusted. A file
    /// whose signature is stale is still a file its owner may want, and saying so is more use than
    /// throwing it away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    /// The slug to file it under. [`None`] takes the **file's own stem**, which is how every
    /// rendering this product writes carries its name — `[blueprint] name` is display text and may
    /// be spelled the way a person would say it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Whether to replace a blueprint already filed under that slug.
    ///
    /// Refusing by default for [`BlueprintCapture::overwrite`]'s reason, and one more of this
    /// task's own: replacing a signed blueprint with an unsigned one is how a trusted row would
    /// become an untrusted one, and that is a thing to ask about rather than do quietly.
    #[serde(default)]
    pub overwrite: bool,
}

/// `blueprint.apply` — what applying one would do, and doing it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlueprintApply {
    /// Which blueprint, by slug.
    pub blueprint: String,

    /// What the new project is called, and what `{project}` expands to.
    pub project: String,

    /// Where it would live. Absolute; the client resolves it against its own current directory.
    pub root: String,

    /// Whether [`root`](Self::root) is the project's directory or the place to make one in.
    ///
    /// `false` — the default, and what a path somebody typed means — takes `root` exactly as
    /// spelled. A folder a person named is a folder a person named, and re-spelling it quietly
    /// would be worse than the failure this field exists to answer.
    ///
    /// `true` says *make one under here*, and the daemon names it with the project's handle —
    /// `domains::slug`, the rule `{project}` already expands through. **The naming happens at that
    /// end and not in a client**, because a client may not hold that rule: `mix` cannot depend on
    /// `mixengine-core` (`mixengine-proto/tests/workspace_layering.rs`), and the desktop
    /// deliberately holds no naming rule at all. A client knows where it is standing; the daemon
    /// knows what things are called.
    ///
    /// The composed path is not a surprise: it comes back in
    /// [`PlanAction::RegisterProject`](crate::PlanAction), which every client renders before
    /// anything is created.
    ///
    /// Roadmap task **T120a**.
    #[serde(default)]
    pub root_is_parent: bool,

    /// Whether to stop after planning.
    ///
    /// `true` answers [`BlueprintApplyResponse::Planned`](crate::BlueprintApplyResponse) and touches
    /// nothing; `false` answers the job carrying the plan out. One method for both, because the plan
    /// a person reads and the plan the daemon executes have to be the same list.
    #[serde(default)]
    pub dry_run: bool,

    /// Install what the plan's releases need of the machine before anything else — roadmap task
    /// **T152**. What [`BlueprintApplyResponse::Planned`](crate::BlueprintApplyResponse)'s `needs`
    /// asked about.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub install_prerequisites: bool,

    /// Do not judge what the plan's releases need of the machine — T152. The smoke tests still run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_requirements: bool,

    /// The answers to the version questions this plan raises — roadmap task **T78**.
    ///
    /// **A question is asked by a client and answered in the request**, because a daemon has no
    /// keyboard: a job that stopped halfway to ask one would be a job holding a project directory
    /// hostage. An apply meeting a [`Disposition::Choice`](crate::Disposition) with no answer here
    /// is refused before anything happens, naming every question it could not answer — and an
    /// answer to a question this plan does not ask is refused too, because it is an answer composed
    /// against a machine or a moment that has since changed.
    ///
    /// Defaulted, so a request written before this task still decodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<VersionAnswer>,

    /// Agreement to run the blueprint's own `[scaffold]` command — roadmap task **T78a**, its
    /// design's D4.
    ///
    /// **Asked by a client and answered here**, exactly as a version mismatch is, and for the same
    /// reason: a daemon has no keyboard, and a job that stopped halfway to ask would be a job
    /// holding a project directory hostage.
    ///
    /// Absent, and the scaffold step ends `NotRun` while everything else is applied — a blueprint
    /// must not become worthless because nobody answered one question. Present and disagreeing with
    /// the plan, and the apply is refused before anything happens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scaffold: Option<ScaffoldConsent>,

    /// The `.env` keys the person agreed to have written, password included — roadmap task
    /// **T205a**. Apart from [`BlueprintApply::scaffold`], since a plan can carry both and one
    /// consent must not answer the other. A key the plan does not write is refused before
    /// anything is touched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dotenv: Vec<String>,

    /// Whether to plan a front end where this home has none — roadmap task **T115**.
    ///
    /// `core::sites` renders nothing for a home with no front end and succeeds while doing it, so a
    /// manifest with a `[site]` applied to a fresh machine ends with a project, a database, a site
    /// row, a domain, a certificate — and nothing listening.
    ///
    /// **Asked for rather than always.** An apply is about a project; provisioning the machine it
    /// runs on is a wider thing, and doing it unasked would make every apply on a home with no web
    /// server download one — including the ones deliberately about something else. The caller that
    /// sets this is the one whose sentence is *get me a working site*.
    ///
    /// A home that has already chosen a front end — Nginx as much as Caddy — is left alone whatever
    /// this says: the question is whether there is one at all, and a blueprint may not name which,
    /// because exactly one runs per home and the choice is the home's.
    ///
    /// Defaulted, so a request written before this task still decodes and still means what it did.
    #[serde(default)]
    pub front_end: bool,

    /// Whether the services this apply **creates** start with the daemon — roadmap task **T116**.
    ///
    /// **Only what it creates.** A shared MariaDB somebody deliberately leaves stopped is not
    /// something a second project gets to re-decide, so a step that plans
    /// [`Satisfied`](crate::Disposition::Satisfied) is untouched — which also means a second apply
    /// of the same blueprint sets nothing, and the flag is a statement about the machine this apply
    /// is building rather than a repair of one it found.
    ///
    /// **Defaulted to `false`, and the default is a constraint rather than a taste.**
    /// `crates/mixengine-cli/tests/warm_start.rs` is the `bench` job and times a single
    /// `mix service start`; a fixture that quietly turned this on would put a boot walk beside that
    /// measurement and make the number mean something else.
    #[serde(default)]
    pub autostart: bool,
}

/// Agreement to run one command, naming the command.
///
/// **It names what was read rather than saying yes.** A blueprint can be re-imported between the
/// plan a person read and the apply they sent; a consent naming the command they were shown is the
/// only kind that cannot be spent on a different one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ScaffoldConsent {
    /// The command as it was shown, `{project}` already expanded. Empty for an archive.
    #[serde(default)]
    pub command: String,

    /// The archive URL as it was shown — roadmap task **T205**, D2. Exactly one of `command` and
    /// this names what the plan showed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,

    /// Whether the person was told this blueprint is nobody's to vouch for.
    ///
    /// **Checked against the row rather than believed.** A blueprint re-imported without its
    /// signature between the reading and the sending would otherwise have its command run under a
    /// consent given for a signed one; disagreement in either direction refuses the apply. It is
    /// also what keeps the rule in the daemon, where a client cannot decline to hold it.
    #[serde(default)]
    pub untrusted: bool,
}

/// One version question, answered.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct VersionAnswer {
    /// What the question was about.
    pub subject: AnswerSubject,

    /// What to do about it.
    pub answer: MismatchAnswer,
}

/// What a version question is about.
///
/// **Tagged rather than a bare string.** `php` is a language and `mariadb@main` is an instance; one
/// string field holding both is one collision away from applying an answer to the wrong thing.
///
/// A service question only ever arises for an instance that already exists, so its id is always
/// spellable — which is why this may carry a [`ServiceId`] where
/// [`PlanAction::EnsureService`](crate::PlanAction) deliberately carries the package and the
/// instance apart.
///
/// **Closed, where most of this crate is `non_exhaustive`**, on [`ProjectRef`]'s rule: a client
/// sends this, so a variant nothing here handles would be a request the daemon has to guess at.
/// What a *client* must tolerate growing is the plan it is answering, and that is
/// [`PlanAction`](crate::PlanAction), which is open.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "subject", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AnswerSubject {
    /// A language whose installed version is not the one the blueprint asks for.
    Runtime {
        /// Which language.
        kind: RuntimeKind,
    },

    /// A service instance already running a version the blueprint did not ask for.
    Service {
        /// Which instance.
        id: ServiceId,
    },
}

impl std::fmt::Display for AnswerSubject {
    /// The spelling a person typed, which is also the one a refusal names.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime { kind } => f.write_str(kind.as_str()),
            Self::Service { id } => f.write_str(id.as_str()),
        }
    }
}

/// What to do about a version that is not the one asked for.
///
/// **There is no `Cancel`.** The third answer in the feature doc's sentence needs nothing on the
/// wire: it is not sending the apply.
///
/// Closed, for the reason [`AnswerSubject`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum MismatchAnswer {
    /// Install what the blueprint asks for, and pin the project to it.
    ///
    /// Valid for a runtime. A service instance that already exists cannot be repointed at another
    /// version by this build — moving one is a database upgrade under somebody's data directory,
    /// and `service.create` and `service.delete` are the two ends of a row's life with nothing
    /// between them. The plan answers that combination with a `Blocked` step naming the way out.
    Install,

    /// Use what this machine has, and pin the project to *that* version.
    ///
    /// The pin is the whole point (the T78 design, D7): without it the two answers would produce
    /// identical machines and the question would be theatre.
    UseInstalled,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request from a client that has never heard of an answer still decodes: the field is
    /// defaulted, which is what stops this task breaking every caller T77 shipped.
    #[test]
    fn a_request_without_answers_still_decodes() {
        let asked: BlueprintApply = serde_json::from_value(serde_json::json!({
            "blueprint": "blog-stack",
            "project": "shop",
            "root": "/tmp/shop"
        }))
        .expect("a request older than this task");

        assert!(asked.answers.is_empty());
        assert!(!asked.dry_run);
    }

    /// **The consent names what was read** — roadmap task **T78a**, its design's D4: the exact
    /// command, and whether the person was told nobody had vouched for the blueprint.
    #[test]
    fn a_consent_carries_the_command_and_what_was_said_about_it() {
        let consent = ScaffoldConsent {
            command: "composer create-project laravel/laravel shop".to_owned(),
            archive: None,
            untrusted: true,
        };

        let json = serde_json::to_value(&consent).expect("it encodes");
        assert_eq!(
            json["command"],
            "composer create-project laravel/laravel shop"
        );
        assert_eq!(json["untrusted"], true);

        let back: ScaffoldConsent = serde_json::from_value(json).expect("and decodes");
        assert_eq!(back, consent);
    }

    /// A request carrying no consent is a request nobody answered the question in, which is the
    /// reading T78 shipped and the safe one.
    #[test]
    fn a_request_without_a_consent_agrees_to_nothing() {
        let asked: BlueprintApply = serde_json::from_value(serde_json::json!({
            "blueprint": "borrowed",
            "project": "shop",
            "root": "/tmp/shop"
        }))
        .expect("a request older than this task");

        assert!(asked.scaffold.is_none());
    }

    /// **Two namespaces, told apart by a tag.** `php` is a language and `mariadb@main` is an
    /// instance, and one string field holding both is one collision away from applying an answer to
    /// the wrong thing.
    #[test]
    fn an_answer_names_which_kind_of_subject_it_is_about() {
        let answer = VersionAnswer {
            subject: AnswerSubject::Runtime {
                kind: RuntimeKind::Php,
            },
            answer: MismatchAnswer::UseInstalled,
        };

        let json = serde_json::to_value(&answer).expect("it encodes");
        assert_eq!(json["subject"]["subject"], "runtime");
        assert_eq!(json["subject"]["kind"], "php");
        assert_eq!(json["answer"], "use_installed");

        let back: VersionAnswer = serde_json::from_value(json).expect("and decodes");
        assert_eq!(back, answer);
    }

    /// The subject is what a refusal names, so it has one spelling and it is the one a person typed.
    #[test]
    fn a_subject_prints_as_the_thing_a_person_would_type() {
        assert_eq!(
            AnswerSubject::Runtime {
                kind: RuntimeKind::Php
            }
            .to_string(),
            "php"
        );
        assert_eq!(
            AnswerSubject::Service {
                id: ServiceId::parse("mariadb@main").expect("an id")
            }
            .to_string(),
            "mariadb@main"
        );
    }
    #[test]
    fn a_consent_to_an_archive_names_the_url_and_no_command() {
        let consent: ScaffoldConsent =
            serde_json::from_str(r#"{"archive":"https://wordpress.org/latest.zip"}"#)
                .expect("reads");

        assert_eq!(consent.command, "");
        assert_eq!(
            consent.archive.as_deref(),
            Some("https://wordpress.org/latest.zip")
        );
        assert!(!consent.untrusted);
    }
}
