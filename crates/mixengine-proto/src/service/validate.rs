//! Whether a [`ServiceSpec`] can be run as written: [`ServiceSpec::validate`] and the checks it is
//! made of.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{
    EnvValue, HealthProbe, ReadyCheck, ReloadBehaviour, RestartPolicy, ServiceId, ServiceSpec,
    SpecError, StopBehaviour,
};

impl ServiceSpec {
    /// Check everything a spec can know about itself.
    ///
    /// [`ServiceSpecBuilder::build`] runs this, so a spec that came from the builder is already
    /// checked and calling it again is redundant. It is public for the other way a spec arrives — a
    /// `services` row or an `extension.toml`, through [`serde::Deserialize`], which deliberately does
    /// not validate because the error belongs to whoever knows which row or which file it came from.
    /// That caller runs this rather than restating the rules, which is what keeps the two paths from
    /// disagreeing about what a usable spec is.
    ///
    /// What is checked is what one spec can know on its own. Cycles across several specs belong to
    /// whoever assembles them (roadmap task T17), and whether `program` exists on disk belongs to the
    /// supervisor, which is the only layer allowed to look.
    ///
    /// # Errors
    ///
    /// [`SpecError::Invalid`] naming the field at fault, or [`SpecError::Missing`] for a program that
    /// is an empty path rather than a bad one.
    ///
    /// [`ServiceSpecBuilder::build`]: super::ServiceSpecBuilder::build
    pub fn validate(&self) -> Result<(), SpecError> {
        let invalid = |field: &str, reason: String| SpecError::Invalid {
            id: self.id.clone(),
            field: field.to_owned(),
            reason,
        };

        check_program(&self.id, "program", &self.program)?;

        if !self.cwd.is_absolute() {
            return Err(invalid(
                "cwd",
                format!("`{}` is relative", self.cwd.display()),
            ));
        }

        if self.ready.timeout().is_zero() {
            return Err(invalid("ready", "its timeout is zero".to_owned()));
        }

        if let ReadyCheck::Command { program, .. } = &self.ready {
            check_program(&self.id, "ready", program)?;
        }

        // Windows compares environment variable names case-insensitively and Unix does not, so a
        // spec holding both `Path` and `PATH` is two variables on one OS and one — whichever the
        // block happened to end with — on the other.
        let mut folded: BTreeMap<String, &String> = BTreeMap::new();
        for (key, value) in &self.env {
            // `Command::env` on Unix builds `KEY=VALUE`, so a key carrying `=` silently renames the
            // variable and a key carrying NUL truncates the block.
            if key.is_empty() {
                return Err(invalid(
                    "env",
                    "an environment variable has no name".to_owned(),
                ));
            }
            if key.contains('=') || key.contains('\0') {
                return Err(invalid(
                    "env",
                    format!("`{key}` may not contain `=` or a NUL byte"),
                ));
            }
            if let Some(earlier) = folded.insert(key.to_lowercase(), key) {
                return Err(invalid(
                    "env",
                    format!(
                        "`{earlier}` and `{key}` differ only by case, which is two variables on Unix and one on Windows"
                    ),
                ));
            }
            // A value is free to contain `=` — only the first one separates. A NUL is a different
            // matter: the Windows environment block is NUL-separated, so one there ends the
            // variable early rather than being part of it.
            if matches!(value, EnvValue::Literal { value } if value.contains('\0')) {
                return Err(invalid(
                    "env",
                    format!("the value of `{key}` may not contain a NUL byte"),
                ));
            }
        }

        if let Some(health) = &self.health {
            if health.interval.is_zero() {
                return Err(invalid("health", "its interval is zero".to_owned()));
            }
            if health.timeout.is_zero() {
                return Err(invalid("health", "its timeout is zero".to_owned()));
            }
            if health.timeout > health.interval {
                return Err(invalid(
                    "health",
                    format!(
                        "one probe may take {} but they are {} apart, so they would overlap",
                        health.timeout, health.interval
                    ),
                ));
            }
            if health.failures_before_degraded == 0 || health.successes_before_running == 0 {
                return Err(invalid(
                    "health",
                    "a threshold of zero would decide before it measured".to_owned(),
                ));
            }
            if let HealthProbe::Command { program, .. } = &health.probe {
                check_program(&self.id, "health", program)?;
            }
        }

        match &self.stop {
            StopBehaviour::Signal { grace } => {
                if grace.is_zero() {
                    return Err(invalid("stop", ZERO_GRACE.to_owned()));
                }
            }
            StopBehaviour::Command { program, grace, .. } => {
                check_program(&self.id, "stop", program)?;
                if grace.is_zero() {
                    return Err(invalid("stop", ZERO_GRACE.to_owned()));
                }
            }
            StopBehaviour::Kill => {}
        }

        // Said once because two variants carry it: zero is a window closed before anything could
        // have happened in it, which would report every reload as not done while every reload was
        // in fact happening.
        const ZERO_PATIENCE: &str =
            "it is given no time at all, so it could only ever be abandoned";

        match &self.reload {
            Some(ReloadBehaviour::Command {
                program, patience, ..
            }) => {
                check_program(&self.id, "reload", program)?;

                if patience.is_zero() {
                    return Err(invalid("reload", ZERO_PATIENCE.to_owned()));
                }
            }

            Some(ReloadBehaviour::Signal { patience, .. }) if patience.is_zero() => {
                return Err(invalid("reload", ZERO_PATIENCE.to_owned()));
            }
            Some(ReloadBehaviour::Signal { .. }) => {}

            // `#[non_exhaustive]` is this crate's own promise to its consumers and does not bind a
            // match written here, so a third variant added later fails to compile at this line —
            // which is the reminder it should be.
            None => {}
        }

        if let Some(backoff) = self.restart.backoff() {
            if backoff.initial.is_zero() {
                return Err(invalid("restart", "its initial backoff is zero".to_owned()));
            }
            if backoff.max < backoff.initial {
                return Err(invalid(
                    "restart",
                    format!(
                        "its backoff starts at {} and is capped at {}",
                        backoff.initial, backoff.max
                    ),
                ));
            }
            if backoff.multiplier_percent < 100 {
                return Err(invalid(
                    "restart",
                    format!(
                        "a multiplier of {}% shrinks the wait on every retry",
                        backoff.multiplier_percent
                    ),
                ));
            }
            // At 100 the low end of the spread is already zero, which is the same tight retry loop
            // an `initial` of zero would be — refused two lines up.
            if backoff.jitter_percent >= 100 {
                return Err(invalid(
                    "restart",
                    format!(
                        "a jitter of ±{}% spreads a wait down to nothing, which is the retry loop the backoff exists to prevent",
                        backoff.jitter_percent
                    ),
                ));
            }
        }
        if let RestartPolicy::OnFailure {
            max_retries,
            window,
            ..
        } = self.restart
        {
            if window.is_zero() {
                return Err(invalid(
                    "restart",
                    "its crash-loop window is zero".to_owned(),
                ));
            }
            if max_retries == 0 {
                return Err(invalid(
                    "restart",
                    "it allows no retries, which is `never` said in a way that reports a crash loop"
                        .to_owned(),
                ));
            }
        }

        let mut seen = BTreeSet::new();
        for dependency in &self.depends_on {
            if *dependency == self.id {
                return Err(invalid("depends_on", "it depends on itself".to_owned()));
            }
            if !seen.insert(dependency) {
                return Err(invalid(
                    "depends_on",
                    format!("`{dependency}` is listed twice"),
                ));
            }
        }

        if let Some(percent) = self.limits.cpu_percent {
            if percent == 0 {
                return Err(invalid(
                    "limits",
                    "a CPU cap of 0% would never let it run".to_owned(),
                ));
            }
            if percent > 100 {
                return Err(invalid(
                    "limits",
                    format!(
                        "a CPU cap of {percent}% is more than the one core it is a percentage of"
                    ),
                ));
            }
        }
        if matches!(self.limits.memory_mb, Some(0)) {
            return Err(invalid(
                "limits",
                "a memory cap of 0 MB would never let it start".to_owned(),
            ));
        }

        if let Some(idle) = &self.idle
            && idle.after.is_zero()
        {
            return Err(invalid(
                "idle",
                "it would be stopped the moment it went quiet".to_owned(),
            ));
        }

        if self.logs.max_file_bytes == 0 || self.logs.max_files == 0 || self.logs.ring_lines == 0 {
            return Err(invalid(
                "logs",
                "a zero here throws output away rather than limiting it".to_owned(),
            ));
        }

        Ok(())
    }
}

/// Why a grace period of zero is refused, said once because two variants carry one.
const ZERO_GRACE: &str =
    "its grace period is zero, which is `kill` said in a way that promises a graceful stop";

/// Check one of the programs a spec asks the supervisor to spawn.
///
/// Applied to all four of them — [`ServiceSpec::program`], the command a
/// [`StopBehaviour::Command`] runs, the one a [`HealthProbe::Command`] runs and the one a
/// [`ReloadBehaviour::Command`] runs — because they are the same thing: a path the supervisor hands
/// to the OS. A relative one is resolved against the
/// child's `PATH` and working directory at the moment it runs, which is the ambiguity the rule
/// exists to refuse, and enforcing it on the first only would leave two unguarded ways to run
/// whatever happens to be first on a `PATH`.
///
/// # Errors
///
/// [`SpecError::Missing`] for an empty path, [`SpecError::Invalid`] for a relative one, naming
/// `field` either way.
fn check_program(id: &ServiceId, field: &str, program: &Path) -> Result<(), SpecError> {
    if program.as_os_str().is_empty() {
        return Err(SpecError::Missing {
            id: id.clone(),
            field: field.to_owned(),
        });
    }
    if !program.is_absolute() {
        return Err(SpecError::Invalid {
            id: id.clone(),
            field: field.to_owned(),
            reason: format!(
                "`{}` is relative; a spec is never resolved against a PATH",
                program.display()
            ),
        });
    }

    Ok(())
}
