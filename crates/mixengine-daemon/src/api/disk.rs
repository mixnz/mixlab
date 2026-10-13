//! `daemon.cleanup` — roadmap task **T96**: the job, and the refusal while anything else is
//! running. What is measured and removed is [`crate::disk`]'s; `daemon.disk_usage` is an arm in
//! [`super::rpc`] that asks it directly.

use std::sync::Arc;

use mixengine_proto::rpc;
use mixengine_proto::{
    CleanupQuery, Error, ErrorCode, JobFilter, JobId, JobKind, JobState, JobSummary,
};

use super::Api;

/// How many running jobs a cleanup looks at before it decides — roadmap task **T96**.
///
/// The question is *is anything running*, so one would do; five is what makes the message able to
/// name what it found even when a client started several things at once.
const JOBS_LOOKED_AT: u32 = 5;

/// Refuse when any job other than `except` is running — roadmap task **T96**.
///
/// A cleanup empties `cache/downloads/`, which is where a download resumes from, and
/// `cache/updates/`, which is where a verified payload waits to be run. On Windows the unlink would
/// fail and be reported; on Linux and macOS it succeeds and the install breaks at its final rename,
/// which is the case this exists to prevent.
async fn busy(jobs: &crate::jobs::Jobs, except: Option<JobId>) -> Result<(), Error> {
    let running = jobs
        .list(&JobFilter {
            state: Some(JobState::Running),
            limit: JOBS_LOOKED_AT,
        })
        .await?;

    let Some(other) = running.into_iter().find(|job| Some(job.id) != except) else {
        return Ok(());
    };

    Err(Error::new(
        ErrorCode::PreconditionFailed,
        format!(
            "{} is running, and a cleanup empties the directory a download resumes from. Wait for \
             it to finish, or cancel it with `mix job cancel {}`",
            other.kind, other.id
        ),
    ))
}

impl Api {
    /// `daemon.cleanup` — the job, and the one refusal that has to happen before it exists.
    ///
    /// **Nothing else may be running.** `cache/downloads/` holds a resumable download
    /// `runtime.install` may be writing to and `cache/updates/` a payload `update.apply` is about to
    /// run; unlinking either mid-flight succeeds on Linux and macOS and breaks the install at its
    /// final rename, for a reason nothing in its own log explains (the T96 design, D6).
    ///
    /// **Checked twice.** Here, so that the refusal is a plain error rather than a job somebody has
    /// to go and read; and again as the job's first step, so a job that started in between is
    /// caught. A cleanup is never urgent, and the alternative is a rule that is correct on one
    /// operating system.
    pub(super) async fn cleanup_now(&self, query: CleanupQuery) -> Result<JobSummary, Error> {
        busy(&self.jobs, None).await?;

        let disk = Arc::clone(&self.disk);
        let jobs = Arc::clone(&self.jobs);

        self.jobs
            .begin(
                &JobKind::parse(rpc::method::DAEMON_CLEANUP).expect("a valid kind"),
                move |handle| async move {
                    busy(&jobs, Some(handle.id())).await?;

                    handle
                        .progress(10, "taking back what is safe to lose")
                        .await;
                    let report = disk.cleanup(&query).await?;

                    handle.progress(90, "reading this home back").await;

                    serde_json::to_value(report).map_err(|error| {
                        Error::new(
                            ErrorCode::Internal,
                            format!("a cleanup report could not be encoded: {error}"),
                        )
                    })
                },
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::api::test_daemon::undeclared;

    /// T96. The disk read answers five rows in one order and asks for nothing — the same strictness
    /// `daemon.uninstall_plan` has, and for the same reason: it is what a screen re-reads.
    #[tokio::test]
    async fn the_disk_read_answers_five_rows_and_asks_for_nothing() {
        let daemon = undeclared().await;

        let usage: mixengine_proto::DiskUsage = daemon
            .expect(rpc::method::DAEMON_DISK_USAGE, Value::Null)
            .await;

        assert_eq!(
            usage
                .categories
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            mixengine_proto::DiskCategory::ALL.to_vec()
        );

        for row in &usage.categories {
            assert!(!row.location.is_empty(), "{row:?}");
        }

        let waiting = daemon
            .expect::<mixengine_proto::ElevationStatus>(rpc::method::ELEVATION_STATUS, Value::Null)
            .await;

        assert!(waiting.pending.is_empty(), "the read enqueued something");
    }

    /// T96. A category this method may not reach is not a field, so asking for one is refused before
    /// anything is opened — the refusal cannot be forgotten in a later edit because there is no
    /// code path to forget.
    #[tokio::test]
    async fn a_cleanup_cannot_be_asked_to_reach_a_category_it_may_not() {
        let daemon = undeclared().await;

        let answer = daemon
            .ask(
                rpc::method::DAEMON_CLEANUP,
                serde_json::json!({ "runtimes": true }),
            )
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "invalid_argument",
            "{answer}"
        );
    }
}
