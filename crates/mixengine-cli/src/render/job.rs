//! `mix job`, and the one-job answer every command that follows a job prints.

use super::elevation::grant;
use super::runtime::runtime_summary;
use super::table::table;
use super::time::ago;
use mixengine_proto::{GrantOutcome, JobList, JobOutcome, JobState, JobSummary, RuntimeSummary};
use std::time::SystemTime;

/// `mix job list`, for a person.
pub(crate) fn job_list(list: &JobList) -> String {
    if list.jobs.is_empty() {
        return "this home has run no jobs\n".to_owned();
    }

    let now = SystemTime::now();
    let rows: Vec<[String; 5]> = list
        .jobs
        .iter()
        .map(|job| {
            [
                job.id.to_string(),
                job.kind.to_string(),
                job.state.to_string(),
                format!("{}%", job.percent),
                ago(job.started_at, now),
            ]
        })
        .collect();

    table(["JOB", "KIND", "STATE", "PROGRESS", "STARTED"], &rows)
}

/// One job, for a person: what `mix job status`, `wait` and `cancel` all answer with.
///
/// **A failed job's error is rendered as the daemon wrote it**, message and hint, rather than
/// summarised here — it is the same wire error the call would have been refused with had the work
/// been short enough to do inline, and rewording it would give one failure two spellings.
pub(crate) fn job_status(job: &JobSummary) -> String {
    let mut rendered = format!("job {}: {} ({})\n", job.id, job.state, job.kind);

    let mut field = |label: &str, value: &str| {
        rendered.push_str(&format!("  {label:9} {value}\n"));
    };

    // *doing* only while it is. A finished job's last step is history, and a failed one's is
    // where it stopped, which is where to look.
    if !job.message.is_empty() {
        let step = format!("{} ({}%)", job.message, job.percent);

        match job.state {
            JobState::Running => field("doing", &step),
            JobState::Failed | JobState::Cancelled => field("stopped at", &step),
            JobState::Succeeded => {}
        }
    }
    field("started", &ago(job.started_at, SystemTime::now()));

    match &job.outcome {
        Some(JobOutcome::Failed { error }) => {
            for line in error.to_string().lines() {
                rendered.push_str(&format!("  {line}\n"));
            }
        }

        // The result belongs to the method that produced the job, so this is the one place a
        // rendering has to branch on the kind rather than on the type. `runtime.install` is the only
        // producer there is; anything else prints nothing extra rather than guessing at a shape.
        Some(JobOutcome::Succeeded { result }) => {
            if let Ok(runtime) = serde_json::from_value::<RuntimeSummary>(result.clone()) {
                for line in runtime_summary(&runtime).lines() {
                    rendered.push_str(&format!("  {line}\n"));
                }
            }

            if let Ok(outcome) = serde_json::from_value::<GrantOutcome>(result.clone()) {
                rendered.push_str(&format!("  {}\n", grant(&outcome)));
            }
        }

        _ => {}
    }

    rendered
}

/// Whether a job that ended did what was asked, which is what an exit code is made of.
pub(crate) fn job_succeeded(job: &JobSummary) -> bool {
    job.state == JobState::Succeeded
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{Error, ErrorCode, JobId, JobKind, Timestamp};

    use super::*;

    fn job(state: JobState, outcome: Option<JobOutcome>) -> JobSummary {
        JobSummary {
            id: JobId(7),
            kind: JobKind::parse("elevation.grant").expect("a kind"),
            state,
            percent: 40,
            message: "verifying".to_owned(),
            started_at: Timestamp(0),
            finished_at: None,
            outcome,
        }
    }

    /// a job that is still running says what it is doing.
    #[test]
    fn a_running_job_prints_what_it_is_doing() {
        assert!(job_status(&job(JobState::Running, None)).contains("doing     verifying (40%)"));
    }

    /// a finished job is not doing anything any more.
    #[test]
    fn a_finished_job_prints_no_doing_line() {
        let done = job(
            JobState::Succeeded,
            Some(JobOutcome::Succeeded {
                result: serde_json::Value::Null,
            }),
        );

        assert!(
            !job_status(&done).contains("doing"),
            "{}",
            job_status(&done)
        );
    }

    /// a failed job says where it stopped, which is where to look.
    #[test]
    fn a_failed_job_says_where_it_stopped() {
        let failed = job(
            JobState::Failed,
            Some(JobOutcome::Failed {
                error: Error::new(ErrorCode::Internal, "it broke"),
            }),
        );

        let rendered = job_status(&failed);
        assert!(
            rendered.contains("stopped at verifying (40%)"),
            "{rendered}"
        );
        assert!(!rendered.contains("doing"), "{rendered}");
    }
}
