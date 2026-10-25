//! What clap calls to read an argument it cannot read itself: a service id, a length of time, a
//! runtime and its version, a pin.

use mixengine_proto::{JobState, PackageVersion, RuntimeKind, ServiceId, VersionConstraint};

/// A service id from the command line, refused here rather than at the daemon.
///
/// Not the client deciding anything — [`ServiceId::parse`] is the daemon's own rule, from the crate
/// that owns the vocabulary — it is only where the answer is cheapest: a typo should not start a
/// daemon and travel over a socket to be told it is a typo.
pub(crate) fn service_id(value: &str) -> Result<ServiceId, String> {
    ServiceId::parse(value).map_err(|error| error.to_string())
}

/// A duration from the command line, as a whole number of minutes.
///
/// **Refused rather than rounded when it is not one.** `services.idle_minutes` stores minutes, so
/// `--after 90s` would have to become either one minute or two, and a setting that quietly becomes
/// something else is worse than one that says it cannot. `Millis::parse` is the daemon's own
/// syntax — `30m`, `2h`, `500ms` — rather than a second reading of it here.
///
/// Zero is refused too, and it has its own flag: `--after 0m` reads as *stop it immediately* and
/// means the opposite, so `--never` is what a person types for that.
pub(crate) fn idle_after(value: &str) -> Result<u32, String> {
    let millis = mixengine_proto::Millis::parse(value)
        .ok_or_else(|| format!("{value:?} is not a duration; write it as `30m`, `2h` or `90m`"))?;

    if millis.is_zero() {
        return Err(
            "an idle policy of zero would stop the service on the next sweep; `--never` is how you \
             switch idle stopping off"
                .to_owned(),
        );
    }

    let minutes = millis.0 / 60_000;

    if minutes * 60_000 != millis.0 {
        return Err(format!(
            "{value:?} is not a whole number of minutes, and that is what MixEngine stores; write \
             it as minutes or hours"
        ));
    }

    u32::try_from(minutes).map_err(|_| format!("{value:?} is longer than MixEngine can store"))
}

/// A runtime kind from the command line, refused here for [`service_id`]'s reason.
///
/// The list is in the message because this is a closed set of four and a typo is the whole of what
/// can go wrong: `mix runtime install pph 8.3.33` should say what the four are rather than send
/// somebody to `--help`.
pub(crate) fn runtime_kind(value: &str) -> Result<RuntimeKind, String> {
    RuntimeKind::parse(value).ok_or_else(|| {
        format!(
            "{value:?} is not a runtime MixEngine manages; it knows {}",
            RuntimeKind::ALL.map(RuntimeKind::as_str).join(", ")
        )
    })
}

/// A version from the command line. [`PackageVersion::parse`] is the daemon's own rule.
pub(crate) fn runtime_version(value: &str) -> Result<PackageVersion, String> {
    PackageVersion::parse(value).map_err(|error| error.to_string())
}

/// A version *or a range* from the command line, which only `mix runtime resolve` takes.
pub(crate) fn version_constraint(value: &str) -> Result<VersionConstraint, String> {
    VersionConstraint::parse(value).map_err(|error| error.to_string())
}

/// A job state from the command line, for `mix job list --state`.
pub(crate) fn job_state(value: &str) -> Result<JobState, String> {
    JobState::parse(value).ok_or_else(|| {
        format!(
            "{value:?} is not a job state; a job is {}",
            JobState::ALL.map(JobState::as_str).join(", ")
        )
    })
}

/// `php=^8.3` — one pin, as a person types it.
pub(crate) fn pin(value: &str) -> Result<(RuntimeKind, VersionConstraint), String> {
    let (kind, constraint) = value
        .split_once('=')
        .ok_or_else(|| format!("`{value}` is not `<runtime>=<version>`"))?;

    Ok((runtime_kind(kind)?, version_constraint(constraint)?))
}

/// `2h` as a number of seconds, for `--for`.
///
/// **Hand-written rather than a dependency.** Four suffixes and a bare number of seconds is the
/// whole grammar this flag needs, and every entry in this workspace's `Cargo.toml` carries a
/// paragraph justifying itself; a parser for `2h` cannot write one.
///
/// # Errors
///
/// The sentence a person reads, for anything that is not a positive length of time. Zero is refused
/// rather than read as "no limit": a share that ends when it begins is not a share, and a flag
/// silently ignored is the other way to be wrong.
pub(crate) fn for_seconds(text: &str) -> Result<u64, String> {
    let refusal = || format!("`{text}` is not a length of time; try `30s`, `90m`, `2h` or `1d`");

    let split = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);

    let value: u64 = digits.parse().map_err(|_| refusal())?;

    let multiplier = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return Err(refusal()),
    };

    match value.checked_mul(multiplier) {
        Some(0) | None => Err(refusal()),
        Some(seconds) => Ok(seconds),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_of_time_is_read_from_its_suffix() {
        assert_eq!(for_seconds("30s"), Ok(30));
        assert_eq!(for_seconds("30"), Ok(30));
        assert_eq!(for_seconds("90m"), Ok(5_400));
        assert_eq!(for_seconds("2h"), Ok(7_200));
        assert_eq!(for_seconds("1d"), Ok(86_400));
    }

    /// **Zero is refused rather than treated as "no limit".** A share that ends the instant it
    /// begins is not a share, and silently ignoring the flag would be the other way to be wrong.
    #[test]
    fn a_length_of_time_of_zero_or_a_word_is_refused() {
        assert!(for_seconds("0").is_err());
        assert!(for_seconds("0h").is_err());
        assert!(for_seconds("soon").is_err());
        assert!(for_seconds("2 hours").is_err());
        assert!(for_seconds("").is_err());
        assert!(for_seconds("-1").is_err());
    }

    /// A length large enough to overflow is a refusal and not a wrap.
    #[test]
    fn a_length_too_large_to_hold_is_refused() {
        assert!(for_seconds(&format!("{}d", u64::MAX)).is_err());
    }
}
