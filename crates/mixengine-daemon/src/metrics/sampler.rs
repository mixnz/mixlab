//! One loop, two rates — roadmap task **T71**.
//!
//! Sixty seconds by default, one second while a client holds `GET /metrics` open. **One loop and not
//! two**: a slow loop for the history beside a fast one for the stream would measure the same
//! processes at two different moments and hand a client two answers to one question, and while
//! somebody watched, every minute stored would have been measured twice.
//!
//! **What is measured is decided from the rows, not from the registry's own list.** A service is
//! measurable when its row names a pid *and* the moment that pid began — the pair T18 stores for
//! adoption — because a pid alone would let a service that exited between two ticks be drawn as
//! whatever program the system handed its number to next.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use mixengine_core::Store;
use mixengine_core::config::Metrics as Config;
use mixengine_platform::process::StartTime;
use mixengine_platform::{GroupRoot, Host};
use mixengine_proto::{
    MetricsFrame, MetricsSample, MetricsSubject, ServiceId, ServiceState, Timestamp,
};
use tokio::sync::{broadcast, mpsc, oneshot};

use super::minutes::Accumulator;
use super::watchers::Watchers;

/// How recent a reading has to be for [`Handle::snapshot`] to serve it rather than take another.
///
/// One second, which is the fast rate: a script looping on `metrics.snapshot` cannot drive this
/// machine harder than a client that opened the stream and asked for the fast rate properly.
const FRESH_ENOUGH: Duration = Duration::from_secs(1);

/// How many frames a stream may fall behind by.
///
/// **Eight, where the event bus holds 1024, and a lagging reader is given the newest frame rather
/// than a resync.** For a metric the old value is worth nothing — the next one is a second away — so
/// there is nothing to replay and nothing to tell the client to re-fetch.
const STREAM_CAPACITY: usize = 8;

/// How many snapshot requests may be waiting for the loop at once.
const REQUESTS: usize = 8;

/// How many minutes the watchdog may fall behind by — roadmap task **T71a**.
///
/// Eight, on [`STREAM_CAPACITY`]'s reasoning and with a safer failure: a consumer that missed a
/// minute treats it as *nobody measured*, which resets a count. A watchdog eight minutes behind has
/// a larger problem than the minute it lost, and the direction it errs in is towards leaving a
/// service alone.
const MINUTE_CAPACITY: usize = 8;

/// How many milliseconds an hour is, for the retention arithmetic.
const HOUR: i64 = 3_600_000;

/// How far back the figure on the stream reaches — roadmap task **T190c**.
///
/// **Five seconds, because a second is a count of quanta.** Windows adds CPU time to a process
/// 15.6 ms at a time, so a one-second reading can only be 0, 1.56, 3.12… percent of a core: a
/// daemon spending half a percent reads as a figure jumping between 0 and 3. Over five readings a
/// quantum moves the figure by 0.31%. At the idle rate a subject has one reading in five seconds,
/// and the figure is that reading.
const SMOOTHING: Duration = Duration::from_secs(5);

/// The readings of the last [`SMOOTHING`], per subject, and the mean the stream publishes.
///
/// **The stream's figure, not the history's.** The minute accumulator is fed the raw frame, so a
/// minute's `cpu_peak` stays the highest single reading rather than the highest mean.
#[derive(Debug, Default)]
struct Smoother {
    recent: std::collections::BTreeMap<
        MetricsSubject,
        std::collections::VecDeque<(Instant, Option<f32>)>,
    >,
}

impl Smoother {
    /// The frame the stream publishes for `raw`, taken at `now`: each sample's CPU figure is the
    /// mean of its readings in the last [`SMOOTHING`].
    ///
    /// A subject absent from `raw` is forgotten, so one that comes back starts again. A reading of
    /// [`None`] is not averaged in as a zero, and a subject with no figure at all publishes `None`.
    fn publish(&mut self, raw: &MetricsFrame, now: Instant) -> MetricsFrame {
        self.recent
            .retain(|subject, _| raw.samples.iter().any(|sample| &sample.subject == subject));

        let samples =
            raw.samples
                .iter()
                .map(|sample| {
                    let readings = self.recent.entry(sample.subject.clone()).or_default();
                    readings.push_back((now, sample.cpu_percent));

                    while readings.front().is_some_and(|(taken, _)| {
                        now.saturating_duration_since(*taken) >= SMOOTHING
                    }) {
                        readings.pop_front();
                    }

                    let figures: Vec<f32> = readings.iter().filter_map(|(_, cpu)| *cpu).collect();
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "at most a handful of readings, far below f32's exact integers"
                    )]
                    let mean = (!figures.is_empty())
                        .then(|| figures.iter().sum::<f32>() / figures.len() as f32);

                    MetricsSample {
                        cpu_percent: mean,
                        ..sample.clone()
                    }
                })
                .collect();

        MetricsFrame {
            samples,
            ..raw.clone()
        }
    }
}

/// One subject and the process at the head of its group.
type Subject = (MetricsSubject, GroupRoot);

/// Measure these subjects, and say what came back.
///
/// **Absence is the only way a subject goes unreported.** A root that has ended, or whose pid the
/// system handed to something else, produces no sample — never a sample of zero, because a minute
/// with no row has to keep meaning *nobody measured*.
fn frame_from(host: &dyn Host, at: Timestamp, subjects: &[Subject]) -> MetricsFrame {
    let roots: Vec<GroupRoot> = subjects.iter().map(|(_, root)| *root).collect();
    let readings = host.process_metrics().measure(&roots);

    let samples = subjects
        .iter()
        .filter_map(|(subject, root)| {
            let reading = readings.iter().find(|reading| reading.pid == root.pid)?;

            Some(MetricsSample {
                subject: subject.clone(),
                cpu_percent: reading.cpu_percent,
                rss_bytes: reading.rss_bytes,
                processes: reading.processes,
            })
        })
        .collect();

    MetricsFrame {
        at,
        samples,
        // What one core is worth here, so every client divides by the same number (T190c).
        cores: host.resource_control().support().cores,
    }
}

/// The daemon's own group, or [`None`] where this process cannot be identified.
///
/// **Left out rather than measured without an identity.** Every other subject is checked against the
/// moment its process began, and the daemon giving itself an exemption would be the one row on the
/// chart nothing verified.
fn daemon_subject() -> Option<Subject> {
    let pid = std::process::id();
    let started = mixengine_platform::process::started_at(pid)
        .ok()
        .flatten()?;

    Some((MetricsSubject::Daemon, GroupRoot { pid, started }))
}

/// The loop's state: what to measure, what it has assembled, and what it last said.
#[derive(Debug)]
pub(crate) struct Sampler {
    store: Store,
    host: Arc<dyn Host>,
    watchers: Watchers,
    accumulator: Accumulator,

    /// The periods and the retention, read once at boot.
    fast: Duration,
    idle: Duration,
    retention_hours: u32,

    /// The last frame taken and when.
    ///
    /// Owned by the loop rather than shared, because the loop is the only thing that measures: a
    /// second reader taking its own readings would be a second consumer of the CPU state a
    /// difference is taken against, and each would see the interval since the *other's* refresh.
    latest: Option<(Instant, MetricsFrame)>,

    /// The last five seconds of readings, for the figure the stream publishes (T190c).
    smoother: Smoother,

    /// Where an open stream reads its frames from.
    frames: broadcast::Sender<MetricsFrame>,

    /// Where the memory watchdog reads finished minutes from — roadmap task **T71a**.
    ///
    /// **The rows, not the frames**, because the watchdog counts *minutes* and would otherwise have
    /// to assemble them a second time from the stream — at a rate that changes with who is watching.
    /// A second accumulator would also be a second answer to one question.
    minutes: broadcast::Sender<Vec<mixengine_proto::MetricsMinute>>,

    /// Snapshots somebody is waiting for. See [`Handle::snapshot`].
    requests: mpsc::Receiver<oneshot::Sender<MetricsFrame>>,

    /// Kept so that [`Sampler::handle`] can hand out more senders after construction.
    asking: mpsc::Sender<oneshot::Sender<MetricsFrame>>,
}

impl Sampler {
    /// A sampler over this home.
    pub(crate) fn new(
        store: Store,
        host: Arc<dyn Host>,
        watchers: Watchers,
        config: &Config,
    ) -> Self {
        // One in flight per waiting client is enough: a snapshot is answered by the next turn of the
        // loop, and a queue deeper than that would only let requests pile up behind a reading that
        // is already being taken.
        let (asking, requests) = mpsc::channel(REQUESTS);

        Self {
            store,
            host,
            watchers,
            accumulator: Accumulator::default(),
            fast: Duration::from_secs(config.sample_seconds),
            idle: Duration::from_secs(config.idle_sample_seconds),
            retention_hours: config.retention_hours,
            latest: None,
            smoother: Smoother::default(),
            frames: broadcast::Sender::new(STREAM_CAPACITY),
            minutes: broadcast::Sender::new(MINUTE_CAPACITY),
            requests,
            asking,
        }
    }

    /// Where a memory watchdog reads the minutes this loop finishes — roadmap task **T71a**.
    ///
    /// Deliberately not on [`Handle`]: that is what the *API* holds, and a client has no business
    /// subscribing to the input of a decision the daemon makes about its own services.
    pub(crate) fn minutes(&self) -> broadcast::Receiver<Vec<mixengine_proto::MetricsMinute>> {
        self.minutes.subscribe()
    }

    /// The handle the API holds: what a stream reads and what a snapshot reuses.
    pub(crate) fn handle(&self) -> Handle {
        Handle {
            watchers: self.watchers.clone(),
            frames: self.frames.clone(),
            asking: self.asking.clone(),
            retention_hours: self.retention_hours,
        }
    }

    /// How long until the next reading, given who is watching.
    fn period(&self) -> Duration {
        if self.watchers.fast() {
            self.fast
        } else {
            self.idle
        }
    }

    /// Every subject this daemon can measure right now.
    ///
    /// A row whose state is not `running`, or which holds a pid without the moment it began, is not
    /// measurable and is left out — the second case is a row mid-write rather than a service to
    /// draw.
    async fn subjects(&self) -> Vec<Subject> {
        let mut subjects: Vec<Subject> = daemon_subject().into_iter().collect();

        let records = match mixengine_core::services::records(&self.store).await {
            Ok(records) => records,

            // The daemon's own reading still goes out: a database that cannot be read says nothing
            // about what this process costs, and dropping the frame entirely would blank a live
            // client's screen over a failure that has its own log line.
            Err(error) => {
                tracing::warn!(%error, "this home's services could not be read, so only the daemon is measured");
                return subjects;
            }
        };

        subjects.extend(records.into_iter().filter_map(|(id, record)| {
            if record.state != ServiceState::Running {
                return None;
            }

            let root = GroupRoot {
                pid: record.pid?,
                started: StartTime::from_stored(record.pid_start_time?),
            };

            Some((MetricsSubject::Service(ServiceId::parse(id).ok()?), root))
        }));

        subjects
    }

    /// Take one reading, fold it into the minute, and publish it.
    pub(crate) async fn take(&mut self) -> MetricsFrame {
        let subjects = self.subjects().await;
        let at = Timestamp::from_system_time(SystemTime::now());
        let frame = frame_from(self.host.as_ref(), at, &subjects);

        // **The stream and a snapshot get the five-second mean; the history gets the reading** —
        // roadmap task T190c. See `Smoother`.
        let now = Instant::now();
        let published = self.smoother.publish(&frame, now);

        self.latest = Some((now, published.clone()));

        // Nobody listening is the ordinary state of a daemon with no client attached.
        let _ = self.frames.send(published.clone());

        let rolled = self.accumulator.observe(&frame);

        // **Published before it is written, and not conditional on the write** — roadmap task T71a.
        // A database that will not take a metrics row is not a reason to stop watching a ceiling:
        // the watchdog's decision is made from the minute itself, and the row is the history.
        // Nobody listening is the ordinary state of a daemon whose home watches nothing.
        if !rolled.is_empty() {
            let _ = self.minutes.send(rolled.clone());
        }

        self.write(rolled, at).await;

        published
    }

    /// Write the minutes a tick completed, and trim what has aged out.
    ///
    /// **A write that fails is logged and the tick continues.** A database that will not take a
    /// metrics row is not a reason to stop measuring, and the live stream does not depend on it.
    async fn write(&self, rows: Vec<mixengine_proto::MetricsMinute>, now: Timestamp) {
        if rows.is_empty() {
            return;
        }

        for row in rows {
            if let Err(error) = mixengine_core::metrics::write_minute(&self.store, &row).await {
                tracing::warn!(%error, subject = %row.subject, "a metrics row could not be written");
            }
        }

        // **From the wall clock, never from an elapsed `Instant`.** A laptop that slept eight hours
        // has to trim eight hours of rows on the tick after it wakes, and tokio's clock counted none
        // of that time.
        let oldest = Timestamp(now.0.saturating_sub(i64::from(self.retention_hours) * HOUR));

        if let Err(error) = mixengine_core::metrics::trim(&self.store, oldest).await {
            tracing::warn!(%error, "the metrics history could not be trimmed");
        }
    }

    /// Answer a snapshot: the last reading if it is recent enough, or a new one.
    ///
    /// **A reading rather than the last one taken.** With nobody watching this daemon samples once a
    /// minute, so serving the cached tick would answer a person with a number up to a minute old and
    /// would not mention a service that started ten seconds ago. Reusing one younger than
    /// [`FRESH_ENOUGH`] is what stops a script looping on the method from driving this machine at a
    /// rate it never opened a stream to ask for.
    async fn answer(&mut self, waiting: oneshot::Sender<MetricsFrame>) {
        let fresh = self
            .latest
            .as_ref()
            .filter(|(taken, _)| taken.elapsed() < FRESH_ENOUGH)
            .map(|(_, frame)| frame.clone());

        let frame = match fresh {
            Some(frame) => frame,
            None => self.take().await,
        };

        // The caller gave up between asking and now, which is a client that closed its connection.
        let _ = waiting.send(frame);
    }

    /// Finish the minute in hand. What a shutdown calls.
    async fn flush(&mut self) {
        let rows = self.accumulator.drain();
        let now = Timestamp::from_system_time(SystemTime::now());

        self.write(rows, now).await;
    }
}

/// What the API holds: enough to serve a stream and a snapshot, and nothing that could take a
/// reading of its own.
#[derive(Debug, Clone)]
pub(crate) struct Handle {
    watchers: Watchers,
    frames: broadcast::Sender<MetricsFrame>,
    asking: mpsc::Sender<oneshot::Sender<MetricsFrame>>,

    /// How long this home keeps a minute row, so a history answer can say why its chart begins
    /// where it does. Read from the same config the loop trims against, rather than a second copy
    /// somewhere else that could disagree with it.
    retention_hours: u32,
}

impl Handle {
    /// Register one open stream, and hand back the frames from now on.
    ///
    /// The [`Watch`](super::watchers::Watch) travels with the receiver so that the two cannot be
    /// separated: the subscription *is* the reason this daemon is sampling every second, and a
    /// stream that dropped one and kept the other would leave the machine on the fast rate with
    /// nobody reading it.
    pub(crate) fn stream(&self) -> (super::watchers::Watch, broadcast::Receiver<MetricsFrame>) {
        (self.watchers.watch(), self.frames.subscribe())
    }

    /// How long a minute row is kept here.
    pub(crate) const fn retention_hours(&self) -> u32 {
        self.retention_hours
    }

    /// One reading, taken by the loop.
    ///
    /// **Asked of the loop rather than taken here**, because the loop is the only thing that
    /// measures: a CPU figure is a difference against the previous refresh, and a second caller
    /// refreshing on its own would leave each of them measuring the interval since the other.
    ///
    /// [`None`] means the loop is gone, which happens only while the daemon is shutting down.
    pub(crate) async fn snapshot(&self) -> Option<MetricsFrame> {
        let (answer, waiting) = oneshot::channel();

        self.asking.send(answer).await.ok()?;

        waiting.await.ok()
    }
}

/// Run the loop until the daemon shuts down.
pub(crate) fn start(mut sampler: Sampler, shutdown: tokio_util::sync::CancellationToken) {
    let mut signal = sampler.watchers.signal();

    tokio::spawn(async move {
        loop {
            let period = sampler.period();

            tokio::select! {
                () = shutdown.cancelled() => {
                    // A daemon stopping at forty seconds past would otherwise throw away two thirds
                    // of a minute it had already measured.
                    sampler.flush().await;
                    return;
                }

                // A client opening the stream must not wait out a sixty-second sleep for its first
                // frame, and one closing the last stream must not leave the machine on the fast
                // rate. Either way the period is recomputed and the wait begins again.
                () = signal.changed() => continue,

                // Somebody called `metrics.snapshot` and is holding a connection open for the
                // answer. Served here rather than by a reader of its own so that this loop stays
                // the only thing that measures.
                Some(waiting) = sampler.requests.recv() => {
                    sampler.answer(waiting).await;
                    continue;
                }

                () = tokio::time::sleep(period) => {}
            }

            sampler.take().await;
        }
    });
}

#[cfg(test)]
mod tests {
    use mixengine_platform::mock;

    use super::*;

    fn reading(pid: u32, cpu: Option<f32>, rss: u64) -> mixengine_platform::GroupReading {
        mixengine_platform::GroupReading {
            pid,
            cpu_percent: cpu,
            rss_bytes: rss,
            processes: 2,
        }
    }

    fn service(id: &str, pid: u32, stored: i64) -> Subject {
        (
            MetricsSubject::Service(ServiceId::parse(id).expect("an id")),
            GroupRoot {
                pid,
                started: StartTime::from_stored(stored),
            },
        )
    }

    // ---- The five-second mean on the stream — roadmap task T190c. ----

    /// A frame with the daemon alone, at `cpu`.
    fn daemon_at(cpu: Option<f32>) -> MetricsFrame {
        MetricsFrame {
            at: Timestamp(0),
            samples: vec![MetricsSample {
                subject: MetricsSubject::Daemon,
                cpu_percent: cpu,
                rss_bytes: 1,
                processes: 1,
            }],
            cores: 12,
        }
    }

    /// Feed `readings` one second apart, and return the last published figure.
    fn published(readings: &[Option<f32>]) -> Option<f32> {
        let mut smoother = Smoother::default();
        let start = Instant::now();
        let mut last = None;

        for (second, cpu) in readings.iter().enumerate() {
            let now = start + Duration::from_secs(second as u64);
            last = smoother.publish(&daemon_at(*cpu), now).samples[0].cpu_percent;
        }

        last
    }

    #[test]
    fn five_quantised_seconds_publish_their_mean() {
        let mean = published(&[Some(0.0), Some(1.56), Some(0.0), Some(3.12), Some(0.0)])
            .expect("a figure");

        assert!((mean - 0.936).abs() < 0.001, "{mean}");
    }

    #[test]
    fn a_reading_older_than_five_seconds_is_dropped() {
        // The 10.0 is six seconds before the last reading, so it is out of the window.
        let mean = published(&[
            Some(10.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
        ])
        .expect("a figure");

        assert!(mean.abs() < 0.001, "{mean}");
    }

    #[test]
    fn one_reading_publishes_itself() {
        assert_eq!(published(&[Some(2.5)]), Some(2.5));
    }

    #[test]
    fn a_missing_reading_is_not_a_zero() {
        assert_eq!(published(&[None, None]), None);
        assert_eq!(published(&[None, Some(2.0)]), Some(2.0));
    }

    #[test]
    fn the_raw_frame_is_left_as_it_was_for_the_history() {
        let mut smoother = Smoother::default();
        let start = Instant::now();
        smoother.publish(&daemon_at(Some(0.0)), start);

        let raw = daemon_at(Some(3.12));
        let out = smoother.publish(&raw, start + Duration::from_secs(1));

        assert_eq!(
            raw.samples[0].cpu_percent,
            Some(3.12),
            "the peak the minute keeps"
        );
        assert_eq!(
            out.samples[0].cpu_percent,
            Some(1.56),
            "the mean the stream shows"
        );
        assert_eq!(out.cores, 12);
    }

    #[test]
    fn a_subject_absent_from_a_frame_is_forgotten() {
        let mut smoother = Smoother::default();
        let start = Instant::now();
        smoother.publish(&daemon_at(Some(9.0)), start);

        let empty = MetricsFrame {
            at: Timestamp(0),
            samples: vec![],
            cores: 12,
        };
        smoother.publish(&empty, start + Duration::from_secs(1));

        let back = smoother.publish(&daemon_at(Some(1.0)), start + Duration::from_secs(2));
        assert_eq!(
            back.samples[0].cpu_percent,
            Some(1.0),
            "a subject that went and came back starts again"
        );
    }

    #[test]
    fn a_measurable_subject_becomes_a_sample() {
        let host = mock::Host::with_home("/mixengine");
        host.set_group_reading(
            41,
            StartTime::from_stored(1),
            reading(41, Some(3.0), 40_000),
        );

        let frame = frame_from(&host, Timestamp(60_000), &[service("mariadb@main", 41, 1)]);

        assert_eq!(frame.at, Timestamp(60_000));
        assert_eq!(frame.samples.len(), 1);
        assert_eq!(frame.samples[0].rss_bytes, 40_000);
        assert_eq!(frame.samples[0].cpu_percent, Some(3.0));
    }

    #[test]
    fn a_subject_whose_process_is_gone_is_absent_from_the_frame() {
        let host = mock::Host::with_home("/mixengine");

        let frame = frame_from(
            &host,
            Timestamp(60_000),
            &[service("mariadb@main", 4_242, 1)],
        );

        assert!(
            frame.samples.is_empty(),
            "absent, never a sample of zero: a subject that cannot be measured has no row"
        );
    }

    #[test]
    fn a_subject_whose_pid_was_handed_round_is_absent() {
        let host = mock::Host::with_home("/mixengine");
        host.set_group_reading(
            41,
            StartTime::from_stored(999),
            reading(41, Some(3.0), 40_000),
        );

        let frame = frame_from(&host, Timestamp(60_000), &[service("mariadb@main", 41, 1)]);

        assert!(frame.samples.is_empty());
    }

    #[test]
    fn one_unmeasurable_subject_does_not_take_the_others_with_it() {
        let host = mock::Host::with_home("/mixengine");
        host.set_group_reading(41, StartTime::from_stored(1), reading(41, None, 40_000));

        let frame = frame_from(
            &host,
            Timestamp(60_000),
            &[service("mariadb@main", 41, 1), service("redis@main", 42, 1)],
        );

        assert_eq!(frame.samples.len(), 1);
        assert_eq!(
            frame.samples[0].cpu_percent, None,
            "and a group with no CPU figure yet is still a group that was measured"
        );
    }
}
