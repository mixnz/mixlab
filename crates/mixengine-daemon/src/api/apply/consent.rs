//! Who a blueprint says it is, the questions it asks, and the refusals for a consent that does not
//! cover it.

use mixengine_proto::{
    AnswerSubject, BlueprintPlan, Disposition, Error, ErrorCode, PlanAction, PlanStep,
    ScaffoldConsent, ServiceId, VersionAnswer,
};

/// The id the plan's package and instance make, which the plan already checked they could.
///
/// Reaching the error here means the plan changed underneath this apply — a pair that could not be
/// spelled is `Blocked` at planning time, and a blocked plan never becomes a job.
pub(crate) fn identity(package: &str, instance: &str) -> Result<ServiceId, Error> {
    ServiceId::parse(package)
        .ok()
        .filter(|bare| bare.as_str() == instance)
        .or_else(|| ServiceId::parse(format!("{package}@{instance}")).ok())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Internal,
                format!("{package}@{instance} cannot be a service id"),
            )
        })
}

/// Every question a plan asks, which is every `Choice` left in it.
///
/// Read from a plan built with **no** answers, so that "was this asked" and "was this answered" are
/// two readings rather than one reading the other has already altered.
pub(super) fn questions(plan: &BlueprintPlan) -> Vec<AnswerSubject> {
    plan.steps
        .iter()
        .filter(|step| matches!(step.disposition, Disposition::Choice { .. }))
        .filter_map(|step| match &step.action {
            PlanAction::InstallRuntime { kind, .. } => Some(AnswerSubject::Runtime { kind: *kind }),

            // The pair travels apart in the action and together here, which is safe for exactly the
            // reason the answer type says: a service question only arises for an instance that
            // already exists, so the id was spellable before the question was asked.
            PlanAction::EnsureService {
                package, instance, ..
            } => ServiceId::parse(format!("{package}@{instance}"))
                .ok()
                .or_else(|| ServiceId::parse(package).ok())
                .map(|id| AnswerSubject::Service { id }),

            _ => None,
        })
        .collect()
}

/// Why a consent may not be spent on this plan, or [`None`].
///
/// **A consent names what was read.** A blueprint can be re-imported between the plan a person read
/// and the apply they sent, so a consent naming another command is consent to something else, and
/// one that thought the blueprint was signed is consent given under a fact that has changed. Both
/// are refused here, before anything is touched.
///
/// **No consent is not a refusal.** The scaffold step ends `NotRun` and everything else is applied,
/// which is the executor's position kept: a blueprint must not become worthless because nobody
/// answered one question.
pub(super) fn consent_refusal(
    plan: &BlueprintPlan,
    consent: Option<&ScaffoldConsent>,
) -> Option<Error> {
    let consent = consent?;

    // **A command or an archive, whichever this plan has.** Both are the one step a person agrees
    // to, and the consent names what was shown: the command, or the URL.
    let planned_step = plan.steps.iter().find(|step| {
        matches!(
            step.action,
            PlanAction::RunScaffold { .. } | PlanAction::FetchArchive { .. }
        )
    });
    let planned = planned_step.and_then(|step| match &step.action {
        PlanAction::RunScaffold { command } => Some(command.as_str()),
        PlanAction::FetchArchive { url, .. } => Some(url.as_str()),
        _ => None,
    });
    let agreed = consent
        .archive
        .as_deref()
        .unwrap_or(consent.command.as_str());

    let Some(planned) = planned else {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "nothing in this plan runs `{agreed}`; this agrees to a plan made against \
                     another blueprint, or another moment"
                ),
            )
            .with_hint("`mix blueprint apply --dry-run` prints the steps this plan does have"),
        );
    };

    if planned != agreed {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!("this plan runs `{planned}`, and what was agreed to was `{agreed}`"),
            )
            .with_hint("the blueprint changed since the plan was read; read it again"),
        );
    }

    if consent.untrusted == plan.trusted {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                match plan.trusted {
                    true => format!(
                        "`{planned}` was agreed to as an untrusted blueprint's command, and {} \
                         is signed",
                        plan.blueprint
                    ),
                    false => format!(
                        "`{planned}` was agreed to as a signed blueprint's command, and nothing \
                         vouches for {}",
                        plan.blueprint
                    ),
                },
            )
            .with_hint("the blueprint was imported again since the plan was read; read it again"),
        );
    }

    // **Agreed to, and cannot run.** Refused here, in the plan's own words, rather than as a
    // shell's complaint at the end of a job. The hint never says `bin/`: that directory is swept of
    // strangers at every start.
    if let Some(PlanStep {
        disposition: Disposition::Blocked { reason },
        ..
    }) = planned_step
    {
        return Some(
            Error::new(ErrorCode::PreconditionFailed, reason.clone()).with_hint(
                "install it, put it on your PATH and restart the daemon, or apply without \
                 agreeing to the command, and everything else is still applied",
            ),
        );
    }

    None
}

/// Why this apply may not start, or [`None`].
///
/// **Three refusals, in the order they are cheapest to explain**: something that cannot be done at
/// all, a question nobody answered, and an answer to a question nobody asked. All of them are
/// decided here rather than five actions into a project directory, which is the whole point of
/// having planned first.
pub(super) fn refusal(
    questions: &[AnswerSubject],
    plan: &BlueprintPlan,
    answers: &[VersionAnswer],
) -> Option<Error> {
    // **A blocked scaffold blocks the scaffold and not the apply**: the one optional
    // step, whose refusal — when a consent names it — is `consent_refusal`'s.
    let blocked: Vec<String> = plan
        .steps
        .iter()
        .filter(|step| !matches!(step.action, PlanAction::RunScaffold { .. }))
        .filter_map(|step| match &step.disposition {
            Disposition::Blocked { reason } | Disposition::Unsupported { reason } => {
                Some(reason.clone())
            }
            _ => None,
        })
        .collect();

    if !blocked.is_empty() {
        return Some(
            Error::new(
                ErrorCode::PreconditionFailed,
                format!(
                    "this blueprint cannot be applied here: {}",
                    blocked.join("; ")
                ),
            )
            .with_hint(
                "`mix blueprint apply --dry-run` prints every step and what stands in its way",
            ),
        );
    }

    let unanswered: Vec<String> = questions
        .iter()
        .filter(|subject| !answers.iter().any(|given| &&given.subject == subject))
        .map(ToString::to_string)
        .collect();

    if !unanswered.is_empty() {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "this blueprint asks for a version this machine does not have, and nobody \
                     answered for: {}",
                    unanswered.join(", ")
                ),
            )
            .with_hint(
                "`mix blueprint apply` asks each question at the terminal; `--install-missing` and \
                 `--use-installed` answer them all",
            ),
        );
    }

    let unasked: Vec<String> = answers
        .iter()
        .filter(|given| !questions.contains(&given.subject))
        .map(|given| given.subject.to_string())
        .collect();

    match unasked.is_empty() {
        true => None,
        false => Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "nothing in this plan asks about: {}; this answers a plan made against \
                     another machine, or another moment",
                    unasked.join(", ")
                ),
            )
            .with_hint("`mix blueprint apply --dry-run` prints the questions this plan does ask"),
        ),
    }
}
