//! Re-setting a superuser credential in a data directory that already exists — roadmap task
//! **T127**.
//!
//! # Why this is not `first_run`
//!
//! [`first_run`](super::first_run) creates a data directory and puts a credential in it. This finds
//! one that is already there, holding databases somebody cares about, and changes the credential
//! alone. The two differ in what they may destroy, which is why they are two modules and why each
//! refuses the other's cases: `first_run` passes over a `Ready` directory without touching it — so a
//! server whose password has drifted is never repaired by restarting it — and this refuses `Empty`
//! and `Unfinished` by naming the command that performs a first run.
//!
//! # Why nothing reaches it on anybody's behalf
//!
//! T126's collision means the credential this home holds may be *another home's*. So the design's D3
//! — a keyring entry is the deed of ownership — is exactly wrong in the one case this module exists
//! for. Writing that entry into a server's data directory is the right repair and is also
//! irreversible, on the strength of a claim known to be unreliable here, which is why it happens
//! only when somebody types the command.
//!
//! # What is here and what is not
//!
//! [`perform`] is everything between the stop and the start, and it stops nothing. The walk belongs
//! to the API, which has the graph, and to the registry, which has the supervisors; what a reset
//! *is* — a stop, these steps, and putting back what went down — is `api::rpc`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mixengine_core::generate::first_run::{self, DataDirectory, FirstRun};
use mixengine_platform::Host;
use mixengine_proto::{Error, ErrorCode, JobKind, JobOutcome, Millis};

use crate::error::ToWire as _;
use crate::jobs::{JobHandle, Jobs};

/// What this job is called in `mix job list`, and the method that asks for it.
const KIND: &str = mixengine_proto::rpc::method::SERVICE_RESET_CREDENTIAL;

/// How much longer than the steps themselves the job is waited for.
///
/// [`first_run`](super::first_run)'s constant and its reasoning: the steps carry their own
/// deadlines, so this covers only what is around them — a keyring read, a writability check, and a
/// machine that is compiling something else at the same time.
const SLACK: Millis = Millis(60_000);

/// The file written and removed to prove the data directory can be written to.
///
/// Named like the markers beside it so that anything finding one knows whose it is. It is removed
/// immediately; one left behind means this process died between the two calls, and the next attempt
/// overwrites it.
const PROBE: &str = ".mixengine-reset-probe";

/// Re-set this service's superuser credential. **The caller has already stopped it.**
///
/// # Errors
///
/// A wire error a client renders: a service whose recipe declares no repair, a data directory that
/// is not ours or has never been bootstrapped, a machine with no credential store, a directory
/// something is still holding, a step that failed. The durable account is the job row, which holds
/// the step and its output and outlives this call.
pub(crate) async fn perform(
    host: &Arc<dyn Host>,
    jobs: &Arc<Jobs>,
    plan: &FirstRun,
) -> Result<(), Error> {
    if !plan.has_reset() {
        // **`invalid_argument` and not `unsupported_platform`**, on `NoDatabaseVocabulary`'s own
        // reasoning: `unsupported` says *this operating system cannot*, which would be a lie about
        // the machine. Every system this ships to can repair a MariaDB; what has no credential of
        // its own is the service that was named.
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "this service's first run declares no way to re-set a credential in place".to_owned(),
        )
        .with_hint(
            "only the database servers keep a superuser password of their own; `mix service list` \
             shows what this home runs",
        ));
    }

    repairable(plan.data()).await?;

    let kind = JobKind::parse(KIND).expect("a constant that is a valid job kind");
    let patience = Millis(plan.reset_budget().0.saturating_add(SLACK.0));

    let (host, work) = (Arc::clone(host), plan.clone());
    let started = jobs
        .begin(&kind, move |handle| async move {
            run(&host, &work, &handle).await
        })
        .await?;

    match jobs.wait(started.id, patience).await?.outcome {
        Some(JobOutcome::Succeeded { .. }) => Ok(()),
        Some(JobOutcome::Failed { error }) => Err(error),

        // Cancelled, or still running when the wait gave up. The service stays stopped either way,
        // which is the honest state: a database whose credential is half re-set is not one to put
        // back in front of an application.
        other => Err(Error::new(
            ErrorCode::Internal,
            format!(
                "the credential reset did not finish; `mix job status {}` has the account of it \
                 ({other:?})",
                started.id
            ),
        )),
    }
}

/// Whether this directory is one a reset may act on.
///
/// **Refused rather than bootstrapped, and that is the decision rather than a check.** The two
/// operations differ in what they may destroy, so a reset that quietly created what it could not
/// find would destroy nothing today and would be the command that one day runs against the wrong
/// path. `first_run::inspect` already answers the whole question, and its four variants are the
/// whole contract.
async fn repairable(data: &Path) -> Result<(), Error> {
    match first_run::inspect(data)
        .await
        .map_err(|error| error.to_wire())?
    {
        DataDirectory::Ready { .. } => Ok(()),

        DataDirectory::Empty | DataDirectory::Unfinished => Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!(
                "{} holds no finished bootstrap, so there is no credential in it to re-set; this \
                 is a first run rather than a repair",
                data.display()
            ),
        )
        .with_hint("`mix service start` performs a first run, and clears one that did not finish")),

        // **Not cleared, although `first_run` clears an `Unfinished` one.** Deleting a data
        // directory happens in one place, on one path, for one reason; a second caller able to do it
        // is a second answer to when MixEngine removes somebody's database.
        DataDirectory::Foreign => Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!(
                "{} has contents and was not created by MixEngine, so nothing here will rewrite \
                 its superuser",
                data.display()
            ),
        )),
    }
}

/// Prove nothing else is holding the data directory.
///
/// **Measured, and the reason it is a check rather than a hope.** `mysqld.exe` forks a child on
/// Windows; a child that outlived its parent held the directory and answered `The innodb_system data
/// file 'ibdata1' must be writable` — a sentence about a live process wearing the words of a file
/// permission, which sends whoever reads it to `icacls`. The supervisor kills the whole process
/// group, so this should never fire, and the sentence it produces when it does is the whole value of
/// having it.
async fn writable(data: &Path) -> Result<(), Error> {
    let probe = data.join(PROBE);

    match tokio::fs::write(&probe, b"").await {
        Ok(()) => {
            // Best effort: the answer the caller wanted is already known, and a removal that failed
            // must not replace it.
            if let Err(error) = tokio::fs::remove_file(&probe).await {
                tracing::warn!(path = %probe.display(), %error, "a reset probe file could not be removed");
            }

            Ok(())
        }

        Err(source) => Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!(
                "{} could not be written to, which for a service that has been stopped means \
                 something is still running against it: {source}",
                data.display()
            ),
        )
        .with_hint(
            "look for a server process this daemon does not supervise (one left by an older run, \
             or started by hand) and stop it. On Windows a database server can outlive the process \
             that started it",
        )),
    }
}

/// The credentials this ritual names, read from the keyring and generated where there is none.
///
/// **Read rather than rotated.** The repair changes as little as it can: the value written into the
/// data directory is the one this home already holds, so nothing else on the machine has to be told
/// anything. Rotating would in fact be safe — nothing outside the keyring keeps a copy, which was
/// measured — and it is still not what a repair should do by default.
///
/// **Generated where the entry is missing**, which is the other failure `services::databases`
/// reports, so one command answers both shapes. In `first_run::store_the_secrets`' order and for its
/// reason: a machine with no credential store fails here, before a step has changed anything.
async fn credentials(
    host: &Arc<dyn Host>,
    plan: &FirstRun,
) -> Result<BTreeMap<String, String>, Error> {
    let mut held = BTreeMap::new();

    for spec in plan.secrets() {
        let address = plan.secret_address(spec.key);

        // **Through `crate::secrets`**, which is what gives an entry written before addresses named
        // their home somewhere to be found — roadmap task **T126**.
        let secret = match crate::secrets::read(host, &address).await? {
            Some(found) => found,
            None => {
                let made = mixengine_platform::generate_secret(spec.length)
                    .map_err(|error| error.to_wire())?;

                super::databases::write(host, &address, &made).await?;

                made
            }
        };

        held.insert(spec.key.to_owned(), secret);
    }

    Ok(held)
}

/// The steps, inside the job.
async fn run(
    host: &Arc<dyn Host>,
    plan: &FirstRun,
    handle: &JobHandle,
) -> Result<serde_json::Value, Error> {
    handle
        .progress(0, "reading this service's credential")
        .await;

    let secrets = credentials(host, plan).await?;

    handle
        .progress(10, "checking that nothing is holding the data directory")
        .await;

    writable(plan.data()).await?;

    let steps = plan
        .reset_steps(secrets)
        .map_err(|error| error.to_wire())?
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Internal,
                "this service declared a credential reset and then built none".to_owned(),
            )
        })?;

    for (index, step) in steps.iter().enumerate() {
        if handle.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Internal,
                "the credential reset was cancelled".to_owned(),
            ));
        }

        let percent = u8::try_from(20 + index * 70 / steps.len().max(1)).unwrap_or(90);
        handle.progress(percent, step.label.clone()).await;

        super::step::run(step).await?;
    }

    // **Capped below a hundred deliberately.** What proves this worked is the service starting and
    // its readiness check passing — an authenticated query, for all three of these recipes — and
    // that happens after this job has returned. `postgres --single` exits 0 on a syntax error, so a
    // job reporting success here would be reporting an exit code and calling it a repair.
    handle
        .progress(95, "the credential is set; the service is started next")
        .await;

    Ok(serde_json::json!({ "data_dir": plan.data() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A [`FirstRun`] can only be built by `mixengine-core`'s generator, so what is provable here is
    /// what needs no plan: which directories a repair may act on, and what it says about the ones it
    /// refuses. The whole repair against a real server is `crates/mixengine-cli/tests/mariadb.rs`.
    ///
    /// A directory that is not ours is refused, and is still there afterwards.
    #[tokio::test]
    async fn a_foreign_data_directory_is_refused_and_left_alone() {
        let home = tempfile::tempdir().expect("a directory");
        let data = home.path().join("data");

        std::fs::create_dir_all(&data).expect("a directory");
        std::fs::write(data.join("ibdata1"), b"somebody's database").expect("contents");

        let refused = repairable(&data)
            .await
            .expect_err("that directory is not ours");

        assert_eq!(refused.code, ErrorCode::PreconditionFailed, "{refused:?}");
        assert!(
            refused.message.contains("was not created by MixEngine"),
            "{refused:?}"
        );
        assert!(
            data.join("ibdata1").is_file(),
            "the refusal removed something"
        );
    }

    /// One that has never been bootstrapped is a first run, and the refusal names the command.
    ///
    /// **The message is the assertion.** A reset that quietly bootstrapped an empty directory is how
    /// a repair turns into a data-loss report, and the sentence is what keeps somebody from going
    /// looking for the other command.
    #[tokio::test]
    async fn an_empty_data_directory_is_refused_as_a_first_run() {
        let home = tempfile::tempdir().expect("a directory");
        let data = home.path().join("data");

        let refused = repairable(&data)
            .await
            .expect_err("there is nothing to repair");

        assert_eq!(refused.code, ErrorCode::PreconditionFailed, "{refused:?}");
        assert!(refused.message.contains("first run"), "{refused:?}");
        assert!(
            refused
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("mix service start")),
            "{refused:?}"
        );
    }

    /// One we began and did not finish is refused too, and **is not cleared**.
    #[tokio::test]
    async fn an_unfinished_data_directory_is_refused_and_not_cleared() {
        let home = tempfile::tempdir().expect("a directory");
        let data = home.path().join("data");

        first_run::mark_started(&data).await.expect("the marker");
        std::fs::write(data.join("ibdata1"), b"half a database").expect("contents");

        let refused = repairable(&data)
            .await
            .expect_err("there is nothing to repair");

        assert_eq!(refused.code, ErrorCode::PreconditionFailed, "{refused:?}");
        assert!(
            data.join("ibdata1").is_file(),
            "the refusal cleared a directory it should have left alone"
        );
    }

    /// A finished one is what a reset is for.
    #[tokio::test]
    async fn a_ready_data_directory_is_repairable() {
        let home = tempfile::tempdir().expect("a directory");
        let data = home.path().join("data");

        first_run::mark_started(&data).await.expect("the marker");
        first_run::mark_ready(&data, "11.4.9")
            .await
            .expect("and the second");

        repairable(&data)
            .await
            .expect("a ready directory is repairable");
    }

    /// A writable directory answers yes and is left exactly as it was found.
    #[tokio::test]
    async fn a_writable_data_directory_keeps_no_probe_behind() {
        let home = tempfile::tempdir().expect("a directory");
        let data = home.path().to_path_buf();

        writable(&data)
            .await
            .expect("a temporary directory is ours");

        assert!(
            !data.join(PROBE).exists(),
            "the writability check left its probe behind"
        );
    }

    /// One that cannot be written to is reported as something holding it, not as a permission.
    #[tokio::test]
    async fn a_directory_that_cannot_be_written_names_what_that_means() {
        let home = tempfile::tempdir().expect("a directory");
        let data = home.path().join("not").join("there");

        let refused = writable(&data)
            .await
            .expect_err("it is not there to write in");

        assert_eq!(refused.code, ErrorCode::PreconditionFailed, "{refused:?}");
        assert!(
            refused.message.contains("still running against it"),
            "{refused:?}"
        );
    }
}
