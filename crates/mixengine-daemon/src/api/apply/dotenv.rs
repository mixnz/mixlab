//! Writing the `.env` a blueprint asks for, and refusing a value it cannot hold.

use mixengine_proto::{
    AppliedDatabase, BlueprintPlan, DatabaseCredentialsQuery, DatabaseProtocol, Error, ErrorCode,
    PlanAction, StepResult,
};

use super::super::Api;

use super::steps;

/// One line of an apply's own narration.
///
/// **A job's log carries what the apply did, and not only what somebody else's command printed.**
/// `LogSubject::Job` was once written by exactly one step, the `[scaffold]` command, so a failure
/// anywhere before it left a log with nothing in it — which is precisely what a client rendering
/// that log then had to show. `mix job logs <id>` gets the same narration, with no client change at
/// all.
///
/// [`Stream::Stderr`] for a step that did not go well, on the ordinary convention: a client
/// colouring its output has something to colour by.
///
/// [`Stream::Stderr`]: super::Stream::Stderr
impl Api {
    /// The `.env` line: the URL of the database this apply made, appended to the person's file.
    ///
    /// **A failure is a failed step**, as the scaffold's is: the project works, and the person can
    /// set the key by hand. No sentence here carries the value.
    pub(super) async fn write_dotenv(
        &self,
        context: &steps::Context,
        key: &str,
        path: &str,
    ) -> StepResult {
        // **The account this apply made**, not the plan's: the step may have renamed it.
        let Some(made) = context.database.clone() else {
            return StepResult::Failed {
                why: format!("{key} was not written to {path}: no database was made"),
            };
        };

        let outcome = async {
            let address = mixengine_core::services::handoff::address(&self.store, &made.service)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{} is not a database a client opens", made.service))?;
            let credentials = self
                .databases
                .credentials(&DatabaseCredentialsQuery {
                    service: made.service.clone(),
                    user: Some(made.user.clone()),
                })
                .await
                .map_err(|error| error.message)?;
            let value = dotenv_value(
                &made,
                address.protocol,
                &address.host.to_string(),
                address.port,
                &credentials.password,
            );
            mixengine_core::blueprints::dotenv::write(&context.root, key, &value)
                .map_err(|error| error.to_string())
        }
        .await;

        match outcome {
            Ok(true) => StepResult::Done { note: None },
            Ok(false) => StepResult::AlreadyTrue,
            Err(why) => StepResult::Failed {
                why: format!("{key} was not written to {path}: {why}"),
            },
        }
    }
}

/// A key agreed to that this plan does not write, or [`None`], on [`consent_refusal`]'s rule: a
/// consent names what was read.
///
/// [`consent_refusal`]: super::consent_refusal
pub(super) fn dotenv_refusal(plan: &BlueprintPlan, agreed: &[String]) -> Option<Error> {
    let offered: Vec<&str> = plan
        .steps
        .iter()
        .filter_map(|step| match &step.action {
            PlanAction::WriteDotenv { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    let stray = agreed.iter().find(|key| !offered.contains(&key.as_str()))?;

    Some(
        Error::new(
            ErrorCode::InvalidArgument,
            format!("this plan writes no {stray} to .env, and that is what was agreed to"),
        )
        .with_hint("the blueprint changed since the plan was read; read it again"),
    )
}

/// The URL written for the database this apply made.
pub(super) fn dotenv_value(
    made: &AppliedDatabase,
    protocol: DatabaseProtocol,
    host: &str,
    port: u16,
    password: &str,
) -> String {
    mixengine_core::blueprints::dotenv::database_url(
        protocol.as_str(),
        &made.user,
        password,
        host,
        port,
        &made.database,
    )
}
