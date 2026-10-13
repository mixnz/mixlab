//! How long ago, how long until, and how long up — one rounding for all three.

use mixengine_proto::{Timestamp, Uptime};
use std::time::SystemTime;

/// How long ago something happened, from this machine's clock.
///
/// **The client's own clock, and it is the daemon's too**: the endpoint is a local socket, so there
/// is exactly one clock involved. `daemon.status` carries an `Uptime` because the daemon knows how
/// long it has been up; nothing carries a "now" for a service, and asking for one would be a round
/// trip to learn what `SystemTime::now` already says.
///
/// A moment in the future — a clock moved backwards between the start and this call — reads as
/// `just now` rather than as a negative age.
pub(super) fn ago(Timestamp(happened): Timestamp, now: SystemTime) -> String {
    let Timestamp(now) = Timestamp::from_system_time(now);

    match u64::try_from(now.saturating_sub(happened) / 1_000) {
        Ok(0) | Err(_) => "just now".to_owned(),
        Ok(seconds) => format!("{} ago", units(seconds)),
    }
}

/// How long until something happens, from this machine's clock — roadmap task **T76**.
///
/// [`ago`]'s mirror, sharing [`units`] with it so that "in 1h 58m" and "1h 58m ago" round the same
/// way. A moment already gone reads as `any moment now` rather than as a negative wait: the loop
/// that ends a share runs on a period, so a deadline can be a few seconds past while the share is
/// still up, and that is a wait rather than a fault.
pub(super) fn in_time(Timestamp(happens): Timestamp, now: SystemTime) -> String {
    let Timestamp(now) = Timestamp::from_system_time(now);

    match u64::try_from(happens.saturating_sub(now) / 1_000) {
        Ok(0) | Err(_) => "any moment now".to_owned(),
        Ok(seconds) => format!("in {}", units(seconds)),
    }
}

/// How long the daemon has been up, in the two units that matter at that scale.
///
/// Two and never three: "up 3d 4h" is what somebody wants from a status line, and "3d 4h 17m 6s" is
/// a number nobody reads. The exact figure is in `--json`, in seconds, unrounded.
pub(super) fn uptime(Uptime(seconds): Uptime) -> String {
    units(seconds)
}

/// A number of seconds, at the scale a person reads.
///
/// Shared by [`uptime`] and [`ago`] rather than written twice: "up 13m 32s" and "started 13m 32s
/// ago" are the same rounding, and two copies of it would eventually round differently in one place.
fn units(seconds: u64) -> String {
    let (days, hours, minutes, seconds) = (
        seconds / 86_400,
        (seconds % 86_400) / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60,
    );

    match (days, hours, minutes) {
        (0, 0, 0) => format!("{seconds}s"),
        (0, 0, _) => format!("{minutes}m {seconds}s"),
        (0, _, _) => format!("{hours}h {minutes}m"),
        _ => format!("{days}d {hours}h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The figure itself, where the clock is an argument and not a reading — the same rounding
    /// `ago` and `uptime` use, so all three say `1h 59m` about the same span.
    #[test]
    fn a_wait_is_rounded_the_way_an_age_is() {
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);

        assert_eq!(in_time(Timestamp(1_000_000 + 7_200_000), now), "in 2h 0m");
        assert_eq!(in_time(Timestamp(1_000_000 + 90_000), now), "in 1m 30s");
    }

    /// A deadline the loop has not caught up with yet is a wait, not a negative number: the sweep
    /// runs on a period, so an instant a few seconds past is the ordinary state.
    #[test]
    fn a_deadline_already_gone_reads_as_a_wait() {
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10);

        assert_eq!(in_time(Timestamp(1_000), now), "any moment now");
    }

    #[test]
    fn an_age_is_rounded_the_way_an_uptime_is_and_never_reads_as_negative() {
        let started = Timestamp(1_723_000_000_000);
        let now = |offset: i64| {
            std::time::UNIX_EPOCH
                + std::time::Duration::from_millis((started.0 + offset).unsigned_abs())
        };

        assert_eq!(ago(started, now(812_000)), "13m 32s ago");
        assert_eq!(ago(started, now(500)), "just now");

        // A clock that went backwards between the start and this call. Rendering "-4s ago" would
        // make a user doubt the service rather than the clock.
        assert_eq!(ago(started, now(-4_000)), "just now");
    }

    #[test]
    fn uptime_stops_at_two_units_whichever_two_they_are() {
        assert_eq!(uptime(Uptime(0)), "0s");
        assert_eq!(uptime(Uptime(59)), "59s");
        assert_eq!(uptime(Uptime(60)), "1m 0s");
        assert_eq!(uptime(Uptime(3_599)), "59m 59s");
        assert_eq!(uptime(Uptime(3_600)), "1h 0m");
        assert_eq!(uptime(Uptime(86_399)), "23h 59m");
        assert_eq!(uptime(Uptime(86_400)), "1d 0h");
        assert_eq!(uptime(Uptime(9_000_000)), "104d 4h");
    }
}
