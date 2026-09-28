//! The real reading, taken with `sysinfo` — roadmap task **T71**.
//!
//! **One file for three operating systems**, which is the exception this crate allows itself where a
//! dependency has already done the per-OS work: `sysinfo` reads `/proc` on Linux, `proc_pidinfo` on
//! macOS and a toolhelp snapshot on Windows, and nothing here names which. The `#[cfg]` rule exists
//! to keep operating-system differences out of the crates *above* this one, and this is inside it.
//! The one per-system read this file needs of its own — every process's parent, which is what
//! decides who a reading refreshes (T181) — lives with the other per-system process questions, in
//! `crate::process::parent_table`.
//!
//! **The walk is a pure function over a table.** A test that had to grow a real process tree would
//! be a test that only runs where unsigned children are allowed to start, which on a developer's
//! Windows machine is nowhere — so the table comes in as data and the arithmetic is asserted on
//! that.
//!
//! # A group is a tree, and the alternative was measured against
//!
//! Every supervised service on Windows already runs in a Job Object, and `QueryInformationJobObject`
//! reports the job's total CPU time and peak memory directly — more accurate than this walk and
//! immune to a process that reparents itself. It is deliberately not used: it would measure Windows
//! through a different mechanism than the other two systems, at the moment **T72** is about to hold
//! all three to one threshold, and three numbers that cannot be compared are worse than three
//! numbers wrong in the same direction. This walk overstates shared pages identically everywhere.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use crate::process::StartTime;
use crate::{GroupReading, GroupRoot, ProcessMetrics};

/// One process, as a refresh saw it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Row {
    /// Who started it, where the system still says.
    parent: Option<u32>,

    /// Percentage of one core since the previous refresh.
    cpu_percent: f32,

    /// Resident bytes.
    rss_bytes: u64,
}

/// Everything one refresh saw, beside the pids the refresh before it had seen.
#[derive(Debug, Default)]
struct Snapshot {
    /// This refresh.
    rows: BTreeMap<u32, Row>,

    /// What the previous refresh held, which is the whole of what makes a CPU figure possible.
    previous: BTreeSet<u32>,
}

impl Snapshot {
    /// Sum each root's group out of this snapshot.
    ///
    /// `identify` is asked what the process bearing a pid says about when it began; a root whose
    /// answer differs from what the caller recorded is a pid the system handed round, and produces
    /// no reading at all.
    fn aggregate(
        &self,
        roots: &[GroupRoot],
        identify: &dyn Fn(u32) -> Option<StartTime>,
    ) -> Vec<GroupReading> {
        let children = self.children();

        // **Every other subject's root, so a walk can stop when it reaches one** — found by T72.
        // Every supervised service is a child of the daemon, so a walk that descended through
        // everything would make the daemon's row *the daemon and everything it runs*: the largest
        // consumer on every chart, and a set of rows that counts each service twice when summed.
        let boundaries: BTreeSet<u32> = roots.iter().map(|root| root.pid).collect();

        roots
            .iter()
            .filter(|root| self.rows.contains_key(&root.pid))
            .filter(|root| identify(root.pid) == Some(root.started))
            .map(|root| {
                let mut rss_bytes: u64 = 0;
                let mut cpu = 0.0;
                let mut processes = 0;

                for pid in walk(root.pid, &children, &boundaries) {
                    let Some(row) = self.rows.get(&pid) else {
                        continue;
                    };

                    rss_bytes = rss_bytes.saturating_add(row.rss_bytes);
                    cpu += row.cpu_percent;
                    processes += 1;
                }

                GroupReading {
                    pid: root.pid,
                    // A group whose root this sampler had not yet seen has no difference to report.
                    cpu_percent: self.previous.contains(&root.pid).then_some(cpu),
                    rss_bytes,
                    processes,
                }
            })
            .collect()
    }

    /// Who each process started, inverted from each row's parent.
    fn children(&self) -> BTreeMap<u32, Vec<u32>> {
        invert(
            self.rows
                .iter()
                .filter_map(|(pid, row)| Some((*pid, row.parent?))),
        )
    }
}

/// Who each process started, from `(pid, parent)` pairs.
fn invert(pairs: impl Iterator<Item = (u32, u32)>) -> BTreeMap<u32, Vec<u32>> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();

    for (pid, parent) in pairs {
        children.entry(parent).or_default().push(pid);
    }

    children
}

/// `root` and every process under it, stopping at another subject's root.
///
/// The one walk both halves of a reading take: [`members`] to decide what to refresh, and
/// [`Snapshot::aggregate`] to sum what the refresh saw. Each pid comes out once, in no promised
/// order; whether a table holds a row for it is the caller's question.
fn walk(root: u32, children: &BTreeMap<u32, Vec<u32>>, boundaries: &BTreeSet<u32>) -> Vec<u32> {
    let mut stack = vec![root];

    // Iterative rather than recursive: a process table is a graph this crate did not build, and a
    // cycle in one must not be a stack overflow in the daemon. `seen` is what makes that true rather
    // than hoped for.
    let mut seen = BTreeSet::new();
    let mut visited = Vec::new();

    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }

        visited.push(pid);

        if let Some(kids) = children.get(&pid) {
            // A child that is a subject of its own belongs to that subject and not to this one. Its
            // own descendants go with it, which is why this prunes rather than skips: the boundary is
            // the whole subtree, not one process.
            stack.extend(kids.iter().copied().filter(|kid| !boundaries.contains(kid)));
        }
    }

    visited
}

/// Every process in any of these groups, read from a parent table — roadmap task **T181**.
///
/// What a reading refreshes, and so the whole of what it pays for. A root the table does not hold
/// has ended, and contributes nothing. Whether a root is still the process the caller recorded is
/// [`Snapshot::aggregate`]'s question, asked after the refresh; a recycled pid here costs one
/// process refreshed for nothing.
fn members(roots: &[GroupRoot], parents: &BTreeMap<u32, u32>) -> BTreeSet<u32> {
    let children = invert(parents.iter().map(|(pid, parent)| (*pid, *parent)));
    let boundaries: BTreeSet<u32> = roots.iter().map(|root| root.pid).collect();

    roots
        .iter()
        .filter(|root| parents.contains_key(&root.pid))
        .flat_map(|root| walk(root.pid, &children, &boundaries))
        .collect()
}

/// This machine's own answer, with the state a CPU figure is a difference from.
#[derive(Debug)]
pub(crate) struct Sampler {
    /// The `sysinfo` state and the pids the last refresh saw, together because they are only ever
    /// read and written together.
    state: Mutex<(sysinfo::System, BTreeSet<u32>)>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self {
            state: Mutex::new((sysinfo::System::new(), BTreeSet::new())),
        }
    }
}

impl ProcessMetrics for Sampler {
    fn measure(&self, roots: &[GroupRoot]) -> Vec<GroupReading> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let (system, previous) = &mut *state;

        // **Only the processes in a group, where the machine says who they are** — roadmap task
        // T181. Refreshing everything to learn six parents cost ~12 ms a tick on macOS, where
        // `sysinfo` reads every process's arguments on each refresh. A table that cannot be read
        // falls back to exactly that, with `sysinfo`'s own parents.
        let parents = crate::process::parent_table().ok();

        let rows: BTreeMap<u32, Row> = match &parents {
            Some(parents) => {
                let wanted: Vec<sysinfo::Pid> = members(roots, parents)
                    .into_iter()
                    .map(sysinfo::Pid::from_u32)
                    .collect();

                // A process that left every group keeps running and is never asked about again, so
                // `sysinfo` would hold it forever. Starting again costs this tick its CPU figures —
                // the same as a daemon start — and nothing else.
                if system.processes().len() > 2 * wanted.len() + 16 {
                    *system = sysinfo::System::new();
                    previous.clear();
                }

                system.refresh_processes_specifics(
                    sysinfo::ProcessesToUpdate::Some(&wanted),
                    true,
                    sysinfo::ProcessRefreshKind::nothing()
                        .with_cpu()
                        .with_memory(),
                );

                wanted
                    .iter()
                    .filter_map(|pid| Some((pid.as_u32(), system.process(*pid)?)))
                    .filter(|(_, process)| process.thread_kind().is_none())
                    .map(|(pid, process)| {
                        let row = Row {
                            parent: parents.get(&pid).copied(),
                            cpu_percent: process.cpu_usage(),
                            rss_bytes: process.memory(),
                        };
                        (pid, row)
                    })
                    .collect()
            }

            None => {
                system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

                system
                    .processes()
                    .iter()
                    // **Threads are not processes, and on Linux this list holds both** — found by
                    // T72, which read a single-binary Caddy as 445 MB. `sysinfo` reports each thread
                    // with its process's parent pid *and* its process's whole resident size, so a
                    // group walked over them counts one process once per thread and multiplies its
                    // memory by the thread count. `thread_kind` answers `Some` only for a thread,
                    // and only on Linux and Android; everywhere else this filter passes everything
                    // through, which is why Windows and macOS never showed the fault.
                    .filter(|(_, process)| process.thread_kind().is_none())
                    .map(|(pid, process)| {
                        (
                            pid.as_u32(),
                            Row {
                                parent: process.parent().map(sysinfo::Pid::as_u32),
                                cpu_percent: process.cpu_usage(),
                                rss_bytes: process.memory(),
                            },
                        )
                    })
                    .collect()
            }
        };

        let snapshot = Snapshot {
            rows,
            previous: std::mem::take(previous),
        };

        let readings =
            snapshot.aggregate(roots, &|pid| crate::process::started_at(pid).ok().flatten());

        *previous = snapshot.rows.keys().copied().collect();

        readings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(parent: Option<u32>, cpu_percent: f32, rss_bytes: u64) -> Row {
        Row {
            parent,
            cpu_percent,
            rss_bytes,
        }
    }

    /// A root, its two workers, and an unrelated process that must not be counted.
    fn snapshot() -> Snapshot {
        Snapshot {
            rows: BTreeMap::from([
                (10, row(Some(1), 5.0, 100)),
                (11, row(Some(10), 2.5, 50)),
                (12, row(Some(10), 2.5, 50)),
                (99, row(Some(1), 90.0, 9_000)),
            ]),
            previous: BTreeSet::from([10, 11, 12, 99]),
        }
    }

    fn born_at(stored: i64) -> impl Fn(u32) -> Option<StartTime> {
        move |_| Some(StartTime::from_stored(stored))
    }

    fn root(pid: u32, stored: i64) -> GroupRoot {
        GroupRoot {
            pid,
            started: StartTime::from_stored(stored),
        }
    }

    #[test]
    fn a_group_is_the_root_and_everything_under_it() {
        let measured = snapshot().aggregate(&[root(10, 7)], &born_at(7));

        assert_eq!(measured.len(), 1);
        assert_eq!(
            measured[0].rss_bytes, 200,
            "the root and its two workers, and not the unrelated process"
        );
        assert_eq!(measured[0].processes, 3);
        assert_eq!(measured[0].cpu_percent, Some(10.0));
    }

    /// **A group stops where another group begins** — found by T72.
    ///
    /// Every supervised service is a child of the daemon, so a walk that descended through
    /// everything would report the daemon's row as *the daemon and every service it runs* — which
    /// makes the daemon the largest consumer on every chart, and makes summing the rows count each
    /// service twice. The rows are meant to be disjoint: `resource-isolation.md` says the sampler
    /// measures each supervised group "and `mixengined` itself".
    #[test]
    fn a_group_stops_where_another_group_begins() {
        let measured = snapshot().aggregate(&[root(10, 7), root(11, 7)], &born_at(7));

        let parent = measured
            .iter()
            .find(|reading| reading.pid == 10)
            .expect("the outer group");

        assert_eq!(
            parent.rss_bytes, 150,
            "the root and the worker that is not a subject of its own, and not the one that is"
        );
        assert_eq!(parent.processes, 2);

        let inner = measured
            .iter()
            .find(|reading| reading.pid == 11)
            .expect("the inner group");

        assert_eq!(inner.rss_bytes, 50, "counted once, under itself");
        assert_eq!(inner.processes, 1);
    }

    #[test]
    fn a_group_seen_for_the_first_time_has_no_cpu_figure() {
        let mut snapshot = snapshot();
        snapshot.previous = BTreeSet::new();

        let measured = snapshot.aggregate(&[root(10, 7)], &born_at(7));

        assert_eq!(
            measured[0].cpu_percent, None,
            "a difference needs a previous reading, and a zero would draw an idle service"
        );
        assert_eq!(measured[0].rss_bytes, 200, "memory needs no history");
    }

    #[test]
    fn a_recycled_pid_is_not_measured() {
        let measured = snapshot().aggregate(&[root(10, 7)], &born_at(8));

        assert!(
            measured.is_empty(),
            "the pid is live, but it is not the process the caller recorded"
        );
    }

    #[test]
    fn a_root_that_is_gone_produces_no_reading() {
        let measured = snapshot().aggregate(&[root(4_242, 7)], &born_at(7));

        assert!(measured.is_empty());
    }

    #[test]
    fn a_cycle_in_the_table_is_walked_once_rather_than_forever() {
        // Never seen on a healthy machine. Asserted because the table is the operating system's and
        // a stack overflow in the daemon is not an acceptable way to find that out.
        let snapshot = Snapshot {
            rows: BTreeMap::from([(10, row(Some(11), 1.0, 10)), (11, row(Some(10), 1.0, 10))]),
            previous: BTreeSet::from([10, 11]),
        };

        let measured = snapshot.aggregate(&[root(10, 7)], &born_at(7));

        assert_eq!(measured[0].processes, 2);
        assert_eq!(measured[0].rss_bytes, 20);
    }

    /// The parent table [`snapshot`] describes: a root, its two workers, and a stranger.
    fn parents() -> BTreeMap<u32, u32> {
        BTreeMap::from([(10, 1), (11, 10), (12, 10), (99, 1)])
    }

    /// **The members are what the walk would sum, and nothing else** — roadmap task **T181**. They
    /// are the only processes a reading refreshes, so one missing is a worker drawn as free and one
    /// extra is the machine-wide refresh this exists to avoid.
    #[test]
    fn the_members_are_each_root_and_everything_under_it() {
        assert_eq!(
            members(&[root(10, 7)], &parents()),
            BTreeSet::from([10, 11, 12])
        );
    }

    #[test]
    fn the_members_of_two_groups_are_both_groups() {
        let parents = BTreeMap::from([(10, 1), (11, 10), (12, 11), (20, 1), (21, 20)]);

        assert_eq!(
            members(&[root(10, 7), root(20, 7)], &parents),
            BTreeSet::from([10, 11, 12, 20, 21])
        );
    }

    #[test]
    fn a_root_the_table_does_not_hold_has_no_members() {
        assert!(members(&[root(4_242, 7)], &parents()).is_empty());
    }

    #[test]
    fn a_cycle_in_the_parent_table_is_walked_once_rather_than_forever() {
        let parents = BTreeMap::from([(10, 11), (11, 10)]);

        assert_eq!(members(&[root(10, 7)], &parents), BTreeSet::from([10, 11]));
    }

    /// **A real reading, twice, of the process running the test** — roadmap task **T181**.
    ///
    /// The narrow refresh reaches `sysinfo` through a filter and a kind the table tests above never
    /// see, so this is the one test that a group measured through them still has memory at once and
    /// a CPU figure from its second reading on.
    #[test]
    fn this_process_is_measured_and_has_a_cpu_figure_the_second_time() {
        let sampler = Sampler::default();
        let mine = std::process::id();
        let started = crate::process::started_at(mine)
            .expect("this process can be asked about")
            .expect("this process is running");
        let roots = [GroupRoot { pid: mine, started }];

        let first = sampler.measure(&roots);
        assert_eq!(first.len(), 1, "{first:?}");
        assert!(first[0].rss_bytes > 0, "{first:?}");
        assert_eq!(first[0].cpu_percent, None, "no previous reading yet");

        let second = sampler.measure(&roots);
        assert_eq!(second.len(), 1, "{second:?}");
        assert!(second[0].cpu_percent.is_some(), "{second:?}");
        assert!(second[0].processes >= 1, "{second:?}");
    }

    /// What one reading costs, with seven groups — the shape of the report that led to **T190**.
    ///
    /// **Ignored by default and asserting nothing** — a timing taken on a shared runner is not a
    /// fact to fail a build on. It exists because T71 chose to sample a machine nobody is watching,
    /// and the number belongs beside the periods that spend it, which is
    /// [`ProcessMetrics::measure`]'s documentation. Run it with `--release --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn one_refresh_costs() {
        // This process and six children that do nothing for a minute.
        let mut children: Vec<std::process::Child> = (0..6)
            .map(|_| {
                let mut command = if cfg!(windows) {
                    let mut command = std::process::Command::new("ping");
                    command.args(["-n", "60", "127.0.0.1"]);
                    command
                } else {
                    let mut command = std::process::Command::new("sleep");
                    command.arg("60");
                    command
                };
                command
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .expect("a child to measure")
            })
            .collect();

        let roots: Vec<GroupRoot> = std::iter::once(std::process::id())
            .chain(children.iter().map(std::process::Child::id))
            .map(|pid| GroupRoot {
                pid,
                started: crate::process::started_at(pid)
                    .expect("a child can be asked about")
                    .expect("a child is running"),
            })
            .collect();

        // A new sampler lists the machine on its first reading.
        let began = std::time::Instant::now();
        for _ in 0..10 {
            Sampler::default().measure(&roots);
        }
        println!(
            "a reading that lists the machine: {:?}",
            began.elapsed() / 10
        );

        // The same sampler again, within a second: what a tick at the fast rate pays.
        let sampler = Sampler::default();
        sampler.measure(&roots);
        let began = std::time::Instant::now();
        for _ in 0..10 {
            sampler.measure(&roots);
        }
        println!("a reading at the fast rate: {:?}", began.elapsed() / 10);

        for child in &mut children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
