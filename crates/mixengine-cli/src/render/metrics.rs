//! `mix metrics`: one frame, and the history since a moment.

use super::table::{MISSING, table};
use super::time::ago;
use mixengine_proto::{MetricsFrame, MetricsHistory};
use std::time::SystemTime;

/// `mix metrics` — what everything is costing right now.
///
/// **A dash where a CPU figure could not be taken, and never a zero.** A group measured for the
/// first time has no difference to report yet, and printing `0.0%` there would say a service is
/// idling in the second it is most expensive.
pub(crate) fn metrics_frame(frame: &MetricsFrame) -> String {
    if frame.samples.is_empty() {
        return "nothing could be measured
"
        .to_owned();
    }

    let rows: Vec<[String; 4]> = frame
        .samples
        .iter()
        .map(|sample| {
            [
                sample.subject.to_string(),
                percent(sample.cpu_percent, frame.cores),
                memory(sample.rss_bytes),
                sample.processes.to_string(),
            ]
        })
        .collect();

    table(["SUBJECT", "CPU", "MEMORY", "PROCESSES"], &rows)
}

/// `mix metrics --since` — the per-minute history, oldest first.
///
/// **`SAMPLES` is on the table rather than only in `--json`.** A minute made of one reading and one
/// made of sixty are both averages, and a person comparing two rows has to be able to see which is
/// which. A row is only ever missing because nothing was measured that minute — the service was
/// stopped, or the machine was asleep — so the gaps are part of the answer.
///
/// **The minute is printed as an age rather than as a clock time**, which is [`ago`]'s rule and this
/// workspace's: turning epoch milliseconds into 14:03 needs a civil calendar, and nothing here has
/// one. `--json` carries the millisecond, which is what a chart wants anyway.
///
/// [`ago`]: crate::render::time::ago
pub(crate) fn metrics_history(history: &MetricsHistory, now: SystemTime) -> String {
    if history.minutes.is_empty() {
        return format!(
            "no readings in that window; this home keeps {} hours of them
",
            history.retention_hours
        );
    }

    let rows: Vec<[String; 6]> = history
        .minutes
        .iter()
        .map(|minute| {
            [
                ago(minute.minute, now),
                minute.subject.to_string(),
                percent(minute.cpu_avg, history.cores),
                percent(minute.cpu_peak, history.cores),
                memory(minute.rss_peak),
                minute.samples.to_string(),
            ]
        })
        .collect();

    table(
        [
            "MINUTE",
            "SUBJECT",
            "CPU AVG",
            "CPU PEAK",
            "MEMORY PEAK",
            "SAMPLES",
        ],
        &rows,
    )
}

/// A share of the whole machine with one decimal, as Task Manager shows it, or a dash where no
/// figure was taken.
///
/// `cpu` is percent of one core and `cores` what one core is worth here; `0` is read as one. A
/// figure above zero that rounds to `0.0` prints `<0.1%`, so a running service never reads as idle.
fn percent(cpu: Option<f32>, cores: u32) -> String {
    cpu.map_or_else(
        || MISSING.to_owned(),
        |cpu| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a count of logical processors, far below f32's exact integers"
            )]
            let share = cpu / cores.max(1) as f32;

            if share > 0.0 && share < 0.05 {
                "<0.1%".to_owned()
            } else {
                format!("{share:.1}%")
            }
        },
    )
}

/// Resident bytes, at the scale a person reads memory in.
///
/// Mebibytes with one decimal, unlike [`size`]: a download is tens or hundreds of them and a service
/// is often under ten, where whole numbers would round php-fpm and Redis to the same figure.
///
/// [`size`]: crate::render::size::size
fn memory(bytes: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "one decimal place of mebibytes")]
    let mib = bytes as f64 / (1u64 << 20) as f64;

    format!("{mib:.1} MiB")
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{MetricsMinute, MetricsSample, MetricsSubject, ServiceId, Timestamp};

    use super::*;

    fn sample(subject: MetricsSubject, cpu: Option<f32>, rss: u64) -> MetricsSample {
        MetricsSample {
            subject,
            cpu_percent: cpu,
            rss_bytes: rss,
            processes: 1,
        }
    }

    #[test]
    fn a_subject_with_no_cpu_figure_renders_a_dash_and_never_a_zero() {
        let rendered = metrics_frame(&MetricsFrame {
            at: Timestamp(60_000),
            samples: vec![sample(MetricsSubject::Daemon, None, 41_943_040)],
            cores: 1,
        });

        assert!(rendered.contains("40.0 MiB"), "{rendered}");
        assert!(rendered.contains(MISSING), "{rendered}");
        assert!(
            !rendered.contains("0.0%"),
            "a figure that could not be taken is not a service using no CPU: {rendered}"
        );
    }

    /// **CPU is a share of the machine with one decimal, as Task Manager shows it.** The frame's
    /// figure is percent of one core; `cores` is what one core is worth here.
    #[test]
    fn cpu_is_a_share_of_the_machine() {
        let rendered = metrics_frame(&MetricsFrame {
            at: Timestamp(60_000),
            samples: vec![
                sample(MetricsSubject::Daemon, Some(150.0), 1),
                sample(
                    MetricsSubject::Service(ServiceId::parse("redis@main").expect("an id")),
                    Some(0.53),
                    1,
                ),
            ],
            cores: 12,
        });

        assert!(rendered.contains("12.5%"), "{rendered}");
        assert!(
            rendered.contains("<0.1%"),
            "a running service never reads as idle: {rendered}"
        );
    }

    #[test]
    fn a_frame_that_measured_nothing_says_so_rather_than_printing_an_empty_table() {
        let rendered = metrics_frame(&MetricsFrame {
            at: Timestamp(60_000),
            samples: Vec::new(),
            cores: 1,
        });

        assert_eq!(
            rendered,
            "nothing could be measured
"
        );
    }

    #[test]
    fn a_history_row_shows_how_many_readings_it_is_made_of() {
        let now = SystemTime::now();
        let Timestamp(millis) = Timestamp::from_system_time(now);

        let rendered = metrics_history(
            &MetricsHistory {
                minutes: vec![MetricsMinute {
                    subject: MetricsSubject::Daemon,
                    minute: Timestamp(millis - 120_000),
                    cpu_avg: Some(1.5),
                    cpu_peak: Some(9.5),
                    rss_avg: 41_943_040,
                    rss_peak: 62_914_560,
                    samples: 60,
                }],
                retention_hours: 24,
                cores: 4,
            },
            now,
        );

        assert!(rendered.contains("SAMPLES"), "{rendered}");
        assert!(rendered.contains("60"), "{rendered}");
        assert!(
            rendered.contains("0.4%") && rendered.contains("2.4%"),
            "1.5 and 9.5 percent of one core on four cores: {rendered}"
        );
        assert!(
            rendered.contains("ago"),
            "a minute is printed as an age: {rendered}"
        );
    }

    #[test]
    fn an_empty_history_says_how_long_this_home_keeps_one() {
        let rendered = metrics_history(
            &MetricsHistory {
                minutes: Vec::new(),
                retention_hours: 24,
                cores: 1,
            },
            SystemTime::now(),
        );

        assert!(rendered.contains("24 hours"), "{rendered}");
    }
}
