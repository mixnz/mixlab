//! What MixEngine costs when nobody is using it, measured.
//!
//! `../features/resource-isolation.md` publishes a number: *"Idle footprint (daemon + Caddy, nothing
//! else running): target < 60 MB RSS, ~0 % CPU."* Nothing in this repository measured it until this
//! file, and it is exactly the kind of number that decays quietly: no single commit makes a daemon
//! fat.
//!
//! # Two numbers, and only one of them is a gate
//!
//! **The total is reported and gated on nothing.** Measured in release, it is 58 MB on Windows,
//! 69 MB on Linux and 69 MB on macOS — and most of it is Caddy, a Go program this project neither
//! wrote nor can tune. A budget on that total would be a promise held hostage to next month's
//! release of somebody else's server, and it would go red for a reason no commit here could fix.
//!
//! **`mixengined` alone is the gate**, because it is the half that regresses when this code grows
//! and the half anybody here can do something about. That is `overhead.rs`'s shape — it gates the
//! resolution and prints the wall clock beside it — applied to a quantity rather than to a duration.
//!
//! # What is measured, and through what
//!
//! `rss_bytes` per subject from `mix metrics --json`, which is the daemon's own metrics sampler and
//! therefore the same number a person reads off their own machine. Not a reading of this machine
//! taken beside the daemon: one mechanism, one answer.
//!
//! **The subject set is asserted before either number is.** A home where Caddy failed to start
//! reports one subject and a very good total, and a budget alone would call that a pass — which is
//! this measurement's failure that reads as success.
//!
//! # What this measurement honestly is not
//!
//! It is not *"after 30 idle minutes"*. The daemon being read has just installed a package, rendered
//! configuration and walked a start plan, and its RSS carries the high-water mark of all of it — an
//! allocator returns that to the operating system slowly, or not at all. A daemon that has been up
//! for an afternoon holds less.
//!
//! **That makes the number worse than a real idle machine's, which is why it is acceptable**:
//! passing here means passing comfortably there. A budget that errs strict is a budget doing its job.
//!
//! Restarting the daemon and letting the next one adopt Caddy would be much closer to an idle
//! machine, and cannot be done on two of the three systems: per [ADR
//! 0007](../../../docs/decisions/0007-supervised-child-owns-a-process-group.md), a daemon leaving
//! takes its whole job down on Windows and its immediate children on Linux, so there would be
//! nothing left to adopt and this would measure a daemon standing alone.
//!
//! # Release, ignored
//!
//! `#[ignore]`d because this belongs to the `bench` job rather than to `test`, on `warm_start.rs`'s
//! reasoning: a number a loaded runner can move should not stand between a correctness suite and its
//! answer. The budget is asserted **only in a release build** — a debug daemon is a different
//! program, and a number measured there is about the profile rather than about the design. A debug
//! run still measures and still prints.

mod harness;

use std::time::Duration;

use harness::frontend::{CADDY, declared};
use harness::json;

/// What `mixengined` alone may hold, in bytes.
///
/// **The gate, and the only one here.** Set from what it measures rather than from a target nobody
/// had: 21 MB on Windows, 25 MB on Linux, **30 MB on macOS**, all in release. Thirty-six is about a
/// fifth above the worst of the three, which is room for a feature and not room for a leak.
///
/// **Chosen against the worst system rather than the average**, because one number for three is only
/// honest if it fits the one that fits worst — and the first draft of this constant was 32 MB, set
/// before macOS had been measured, which would have left that system eight per cent of headroom and
/// gone red at the next feature. A budget nobody can meet is raised rather than investigated, which
/// is how a guard stops guarding.
///
/// macOS being the largest is most likely its 16 KB pages against the others' 4 KB — the same
/// working set rounds up further — but that is an explanation nobody here has measured, and the
/// budget does not depend on it being right.
///
/// **Raised from 36 MB on 2026-09-07, and the reason is written down because it is the one the
/// paragraph above warns about.** A week on, the daemon measured 31 MB on
/// Windows, 30 MB on Linux and 35 MB on macOS — five to ten more than the day the budget was set,
/// on every system — and the macOS reading sat within a megabyte of the gate for ten runs before a
/// runner's noise took it over, on a commit that changed a test file in another crate. Forty-two
/// was the same rule applied to the new worst: about a fifth above it, and an explicit non-answer
/// to where the five megabytes went — that question was measured next.
///
/// **Lowered to 41 MB, on that measurement.** Of the growth, ~3.5 MB traced to `mix self-update`: a
/// `reqwest::Client` built and never shared, plus the feed it eagerly fetches at every start. The
/// rest — everything else that landed the same week — measured under 1.5 MB combined, too small and
/// too spread across too many changes to be one thing worth chasing; see the design below for its
/// reading of it.
///
/// The fix built one transport and handed clones of it to the package index, extension registry
/// and update feed clients instead of three independent ones — [the
/// design](../../../docs/specs/2026-09-07-t72b-one-transport-for-three-signed-documents-design.md).
/// Re-measured on the same machine, release, five runs: 30.7–34.2 MB. That spread is wider than the
/// paired A/B that design was measured with — most likely this machine under more background
/// load by the time of the reading rather than the fix itself — and it is written down rather than
/// smoothed over. Forty-one is about a fifth above the worst of the five, on the same rule as every
/// number above it.
///
/// **What the fix did not do is make the eager fetch free.** The transport is shared; the network
/// round trip at every start and the `Feed` cached from it are not, and most of what was measured
/// before the fix is plausibly still there — a live TLS connection inside `reqwest`'s 90-second idle
/// window at the moment this test reads it, which is not the same claim as a leak, and not one this
/// task's numbers prove either way. Chasing that further needs a profiler rather than a second guess
/// at `pool_idle_timeout`, and is deliberately left as a reading nobody has taken yet rather than a
/// change made without one.
const DAEMON_BUDGET: u64 = 41 * 1024 * 1024;

/// The total this project publishes, reported beside the gate and asserted nowhere.
const PUBLISHED_TOTAL: u64 = 60 * 1024 * 1024;

/// How long the home is left alone before the first reading.
///
/// Thirty seconds: the scaled-down version of the promise's *thirty minutes*, long enough that the
/// install and the start walk have returned what they are going to return, short enough that a bench
/// job can afford it. See the module note for what the difference costs, and why it costs it in the
/// safe direction.
const SETTLE: Duration = Duration::from_secs(30);

/// How many readings the median is taken over, a second apart.
///
/// Answering a snapshot walks the process table — about 10 ms on Windows, measured when the sampler
/// was built — and the daemon is one of the subjects, so each reading perturbs what it reads a
/// little. A median is what buys that off, and it is what both other budgets in this job take.
const READINGS: usize = 5;

/// What one `mix metrics --json` said: which subjects it named, and what each of them holds.
#[derive(Debug, Clone)]
struct Reading {
    /// Each subject and its `rss_bytes`, sorted by the wire spelling: `daemon`, `service:<id>`.
    ///
    /// **Kept apart rather than summed on the way in**, because what is gated is one of them: this
    /// project can do something about its own daemon and nothing at all about a web server's
    /// runtime, and one total cannot tell those apart.
    subjects: Vec<(String, u64)>,
}

impl Reading {
    /// Everything this home is holding.
    fn total(&self) -> u64 {
        self.subjects.iter().map(|(_, rss)| rss).sum()
    }

    /// What `mixengined` itself is holding, not counting the services it supervises.
    fn daemon(&self) -> u64 {
        self.subjects
            .iter()
            .find(|(subject, _)| subject == "daemon")
            .map(|(_, rss)| *rss)
            .expect("the daemon measures itself")
    }

    /// The wire spellings alone, for the assertion that this measured the right processes.
    fn named(&self) -> Vec<String> {
        self.subjects
            .iter()
            .map(|(subject, _)| subject.clone())
            .collect()
    }
}

/// One snapshot, through the shipped client.
///
/// **Through `mix` rather than through the socket**, because the number this gates should be the
/// number a person can read for themselves — a suite reaching past the client could gate a figure no
/// command prints.
fn reading(home: &harness::Home) -> Reading {
    let frame = json(&home.mix(&["metrics", "--json"]));

    let samples = frame["samples"]
        .as_array()
        .unwrap_or_else(|| panic!("a snapshot carries samples: {frame}"))
        .clone();

    let mut subjects: Vec<(String, u64)> = samples
        .iter()
        .map(|sample| {
            (
                sample["subject"].as_str().unwrap_or_default().to_owned(),
                sample["rss_bytes"].as_u64().unwrap_or_default(),
            )
        })
        .collect();
    subjects.sort();

    Reading { subjects }
}

/// **A daemon supervising a web server and nothing else stays inside what it is allowed.**
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a budget, measured by the bench job — see the module note and _bench.yml"]
async fn an_idle_daemon_stays_inside_its_budget_and_the_total_is_reported_beside_it() {
    let (home, _daemon, _registry, _site, _control) = declared(&CADDY).await;

    let started = home.mix(&["service", "start", CADDY.package, "--json"]);
    assert!(
        started.status.success(),
        "the web server has to be running for this to be the footprint we publish: {}\n{}",
        harness::stderr(&started),
        home.daemon_log()
    );

    tokio::time::sleep(SETTLE).await;

    let mut taken = Vec::with_capacity(READINGS);

    for round in 0..READINGS {
        let reading = reading(&home);

        // **Every round, not once.** A Caddy that died between the settle and the last reading
        // would otherwise leave a very good number behind it.
        assert_eq!(
            reading.named(),
            vec!["daemon".to_owned(), format!("service:{}", CADDY.package)],
            "round {round} measured the wrong set of processes, so its numbers are about something \
             else: {reading:?}"
        );

        taken.push(reading);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let mut daemons: Vec<u64> = taken.iter().map(Reading::daemon).collect();
    let mut totals: Vec<u64> = taken.iter().map(Reading::total).collect();
    daemons.sort_unstable();
    totals.sort_unstable();

    let daemon = daemons[daemons.len() / 2];
    let total = totals[totals.len() / 2];

    // **The split every time, not only when something fails.** The day the total climbs and the
    // daemon has not, the answer is in the other program, and this line is where that is visible.
    let split: Vec<String> = taken
        .last()
        .expect("a reading was taken")
        .subjects
        .iter()
        .map(|(subject, rss)| format!("{subject} {:.1} MB", as_mb(*rss)))
        .collect();

    println!(
        "\n[t72] mixengined, median of {READINGS}: {:.1} MB   (budget {:.0} MB)\n[t72]   total, \
         reported and gated on nothing: {:.1} MB   (published {:.0} MB)\n[t72]   split: {}\n",
        as_mb(daemon),
        as_mb(DAEMON_BUDGET),
        as_mb(total),
        as_mb(PUBLISHED_TOTAL),
        split.join(", "),
    );

    // **Release only**, on `warm_start.rs`'s rule: a debug daemon is a different program — it
    // measured half again as much in this same home — and a number taken there is about the profile
    // rather than about the design.
    if !cfg!(debug_assertions) {
        assert!(
            daemon <= DAEMON_BUDGET,
            "mixengined is holding {:.1} MB, over the {:.0} MB this budget allows it — the total \
             beside it was {:.1} MB",
            as_mb(daemon),
            as_mb(DAEMON_BUDGET),
            as_mb(total),
        );
    }
}

/// Bytes as megabytes, for the lines a person reads.
fn as_mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
