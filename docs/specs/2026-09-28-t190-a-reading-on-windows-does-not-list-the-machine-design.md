---
status: approved
date: 2026-09-28
task: T190
---

# T190 — A reading on Windows does not list the machine

Phase 7, after [T181](2026-09-24-t181-a-reading-refreshes-only-the-groups-it-measures-design.md).
2026-09-28.

## The case

A person opens the tray panel and sees the daemon at **4–7% CPU**, next to seven services that
are mostly resting. That is the number they judge MixEngine by, and "the daemon samples every second
while you watch" does not excuse it. Whatever the panel shows while it is open is what the product
costs, as far as that person is concerned.

Measured on one developer's Windows 11 machine (12 logical CPUs, 446 processes) against the
installed release `mixengined`, over 20 seconds of its `TotalProcessorTime`:

| State | Daemon CPU |
| --- | --- |
| Nobody watching (60 s rate) | 3.9 ms/s, **0.39%** of one core |
| One `mix metrics --watch` open (1 s rate) | 47.7 ms/s, **4.8%** of one core |

The ~44 ms a second between the two rows is what one reading costs. Almost all of it is the reading
listing every process on the machine, **twice**:

1. `process::parent_table()` takes a `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` to learn
   every process's parent (T181's discovery half).
2. `sysinfo` 0.39.6 then takes **another** `CreateToolhelp32Snapshot` inside
   `refresh_processes_specifics`, even when given `ProcessesToUpdate::Some(&members)`. It walks
   the whole snapshot and filters it against the list afterwards (`src/windows/system.rs`).

On the same machine, measured in release:

| Operation | Cost |
| --- | --- |
| One `CreateToolhelp32Snapshot`, walked | 10–20 ms |
| `sysinfo` refresh of `Some([one pid])`, CPU + memory | ~10 ms |
| `OpenProcess` + `GetProcessTimes` for one pid | ~4 µs |
| `one_refresh_costs` (one root, today) | 16 ms |

T181 predicted that on Windows `sysinfo` would take "one `NtQuerySystemInformation` of the whole
system" and gain by opening fewer processes. It gained that, but the system-wide snapshot stayed,
and discovery added a second one. `traits/metrics.rs` already says that "Windows and Linux have not
been re-measured". This is that measurement.

## Principle

**A tick at the fast rate pays for the processes it measures, not for the machine.** Listing
every process on the machine is discovery, and discovery answers a question that rarely changes:
who belongs to which group. Measuring answers a question that changes every second: what those
processes spend.

## D1. Discovery is kept between ticks

The sampler (`mixengine-platform/src/metrics.rs`) keeps the last parent table and the members it
walked, and lists the machine again only when one of these is true:

- **the roots changed**: a subject was added or removed, or a root's pid or start time differs from
  the last tick (a service was started, stopped or restarted);
- **a member that was read after the last listing can no longer be read**: it ended, so the group
  has changed shape. A member that could not be read on the first reading after a listing, such as
  a process this account may not open, does not count. Otherwise it would force a listing on every
  tick, which is the cost this task removes;
- **the table is older than `REDISCOVER`, 10 seconds**: this is how a worker a group spawned on
  its own (a php-fpm child, a Node cluster worker) gets found.

Every other tick measures the members it already knows. At the 60-second rate this changes nothing,
because every tick is older than 10 seconds and lists the machine as it does today. At the
1-second rate it lists the machine about one tick in ten.

**The cost, stated:** a child a group spawned by itself is counted up to 10 seconds late, and a child
that ended is dropped on the tick that notices, not later. A root is never late, because a
changed root forces discovery. Measured once a second, the charts are not moved by a worker that
lives under 10 seconds.

This applies to all three systems. It keeps `metrics.rs` one file, and Linux's `/proc` walk is a
cost of the same kind, only smaller.

## D2. On Windows, members are read one by one

`sysinfo` cannot refresh a list of pids on Windows without listing the machine, so on Windows the
members are read directly. `windows/process.rs` gains a crate-private read per pid:

- `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`;
- `GetProcessTimes` gives kernel + user time;
- `K32GetProcessMemoryInfo` gives `WorkingSetSize`.

The handle is closed before returning. A pid that cannot be opened, or has an exit time, is absent,
exactly as a process `sysinfo` no longer lists is absent today.

**These are the same calls `sysinfo` makes**, so the property T71 and T181 defended still holds.
`sysinfo` reads `GetProcessTimes` for CPU and `GetProcessMemoryInfo`'s `WorkingSetSize` for
memory. The numbers are the same quantities from the same API. Only the snapshot around them is
gone. This is not the Job Object reading that the module comment rejected.

**CPU is the same figure.** `sysinfo` computes
`100 × Δ(process kernel+user) / Δ(system kernel+user) × logical CPUs`. System kernel + user time
across all CPUs advances at wall-clock time × logical CPUs, so this is `100 × Δprocess / Δwall`:
percent of one core, and able to go over 100 on a busy group, as today. The sampler keeps each
pid's last CPU time and the `Instant` it was read. A pid with no previous reading adds nothing, and a
root with none has `cpu_percent: None`, as today.

Linux and macOS keep `sysinfo` with `ProcessesToUpdate::Some`. There it reads only the pids asked
for, and T181 measured macOS at ~0.5 ms.

**The fallback is unchanged.** If `parent_table()` fails, that tick refreshes `All` through
`sysinfo` and walks `sysinfo`'s own parents, on every system.

## D3. The documents

- `traits/metrics.rs` *What one call costs* and `features/resource-isolation.md` *Measuring, not
  guessing* get the Windows numbers before and after, and lose "not re-measured".
- `phase-7-efficiency.md` gets **T190** directly after T181.
- `CHANGELOG.md` `### Fixed`: the MixEngine daemon no longer uses several percent of a CPU while
  the tray panel or the Dashboard is open on Windows.

## MixLab

**No screen changes, and no method is added.** The tray panel's Daemon/Services row and the
Dashboard's CPU and memory columns draw the same `/metrics` frames. What changes is that the
daemon's own figure in them no longer shows the cost of taking the figure. `client-surface.md` is
unaffected.

## Testing

- **Discovery and reuse, as table tests.** A sampler that is given the same roots twice lists the
  machine once. Changed roots, a member that cannot be read, or a table older than `REDISCOVER`
  each make it list the machine again. The table is injected as a function, so these run as pure
  tests like the walk's.
- **The per-pid read on Windows**, against the test process: it has memory at once, a CPU figure
  from the second reading on, and a pid that has exited is absent.
- `this_process_is_measured_and_has_a_cpu_figure_the_second_time` still holds on all three systems.
- **The measurement is the acceptance test.** `one_refresh_costs` is re-run in release, and is
  extended to seven roots, the shape of the report that led to this task. The 20-second daemon
  measurement above is repeated with `mix metrics --watch` open, **on the same real home and its
  seven services**, not a sandbox. The installed daemon is stopped, the new build runs against
  that home, and the installed one is started again afterwards.

| Target on the machine above | Today | After |
| --- | --- | --- |
| `one_refresh_costs`, a tick that reuses discovery | 16 ms | **< 1 ms** |
| Daemon CPU with one stream open | 4.8% | **≤ 1%** of one core |

If the daemon does not reach 1%, what remains is profiled and named in this spec before the task
is ticked, rather than the target being moved.

## Out of scope

- **The sampling periods** (1 s / 60 s). This makes the fast rate cheap and leaves it the same
  length.
- **Replacing `sysinfo` on Linux or macOS.** Neither lists the machine per tick, apart from
  discovery, which D1 already reduces.
- **Keeping process handles open between ticks.** At ~4 µs an open this saves nothing, and a held
  handle keeps an exited process's object alive, which the start-time check would then have to
  reason about.
