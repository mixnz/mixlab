---
status: approved
date: 2026-09-29
task: T190c
---

# T190c — CPU is shown the way Task Manager shows it

Phase 7, after [T190b](2026-09-28-t190b-the-idle-sweep-does-not-render-to-look-design.md).
2026-09-29.

## The case

The report that started T190 was about a number: the tray panel showed the daemon at 7% and a
person concluded that MixEngine was expensive. T190 and T190b cut what the daemon spends from about
5–6% of a core to 0.8–1.4% with the tray open, and to 0.53% with nobody watching. Two things about
**how the number is shown** remain, and each does more harm to that person's judgement than the
milliseconds that are left.

1. **The unit is one core, and the person's reference is the whole machine.**
   `MetricsSample.cpu_percent` is a percentage of *one* core. That is deliberate: it is the unit
   `ResourceLimits::cpu_percent` is declared in, so a cap and a usage never need converting. Task
   Manager, and every other CPU figure a Windows user has seen, is a percentage of the whole
   machine. On the machine T190 was measured on
   (12 logical processors), the tray's 0.53% is **0.04%** in Task Manager, and the 7% that
   started this was 0.6%. The tray overstates MixEngine twelvefold against the scale its reader
   brings.
2. **The number jumps between 0%, 1.56% and 3.1% from one second to the next.** Windows adds CPU
   time to a process in 15.6 ms quanta, so a one-second reading can only ever be 0, 1, 2… quanta:
   0%, 1.56%, 3.12%. The average is about 0.5%, but that value is never shown. What is shown is a
   number that flickers up to six times its mean. `formatPercent` then prints it with four decimals
   (`7.3350%`), which claims a precision the measurement does not have.

## Principle

**The daemon says what it measured, in the unit limits are declared in. A person reads it on the
scale they already know.** The stored history and the API keep one core as their unit. What
changes is that every reply that carries CPU also carries what one core is worth on this machine,
so every client divides by the same number. And the live figure a person watches is taken over a
span long enough to be a measurement rather than a count of quanta.

## D1. Every reply that carries CPU carries the machine's logical processors

`MetricsFrame` and `MetricsHistory` each gain:

```rust
/// How many logical processors this machine has — roadmap task T190c.
///
/// What a client divides `cpu_percent` by to show a share of the whole machine, as Task Manager
/// does. The same count `LimitSupport::cores` reports, from `available_parallelism`.
pub cores: u32,
```

The daemon fills both from the host's `LimitSupport::cores`, the value `service.limits` already
returns. Nothing else in the proto moves:

- `MetricsSample.cpu_percent`, `MetricsMinute.cpu_avg` and `cpu_peak` keep their unit and their
  doc comments;
- **no migration**: the `metrics_minutes` rows are percentages of one core and stay so;
- `bindings/` is regenerated.

A frame, and not a separate call, because the tray, the Dashboard, `mix metrics --watch` and the
history chart each already hold the one reply they draw from. A second question would be a second
thing to be late or to fail.

## D2. The live figure is the last five seconds, not the last second

The sampler keeps, per subject, the readings of the last `SMOOTHING` = **5 seconds**. **The frame it
publishes on the stream carries their mean** as `cpu_percent`. At the fast rate that is five
readings, so one 15.6 ms quantum moves the figure by 0.31% of one core instead of 1.56%. At the
60-second rate a subject has one reading in any five seconds, and the figure is that reading,
unchanged.

- **The minute history is fed the raw readings**, as today. `cpu_avg` is unchanged by averaging
  twice, and `cpu_peak` keeps meaning the highest single reading. `Accumulator::observe` receives
  the frame from before the mean is taken.
- `metrics.snapshot` answers the published frame, so it carries the mean too.
- A subject seen for the first time in the window has only its readings since then. A subject
  whose readings are all `None` publishes `None`, never `0.0`, which is the rule `cpu_percent`'s
  doc comment states.
- `MetricsSample.cpu_percent`'s doc comment gains one sentence: on the stream it is the mean of the
  last five seconds of readings.

## D3. MixLab shows a share of the machine, with one decimal

A new `formatCpu(percentOfOneCore, cores)` in `metricsState.ts` replaces `formatPercent` wherever
usage is drawn. It works as follows:

- it divides by `cores`;
- it prints **one decimal**, as Task Manager does (`0.1%`, `12.5%`);
- a value above zero that rounds to `0.0` prints `<0.1%`, so a running daemon never reads as doing
  nothing and never reads as more than it is.

It is used in:

| Screen | What changes |
| --- | --- |
| Tray panel, `DaemonUsage` | the Daemon and Services rows, and the bar's width, are a share of the machine |
| Dashboard | the CPU column of the services table |
| Metrics | the chart's axis, *Peak* and *Average*, and the hover, from `MetricsHistory.cores` |

**The limits panel does not change unit.** A cap is declared per core (`--cpu 150` is one and a
half cores), and it is written and read that way. Its field gains a hint in both dictionaries:
*"% of one core: 100 is one full core."*

Every new or changed string is in `en.ts` and `vi.ts`.

## D4. `mix` shows the same number

`mix metrics`, `mix metrics --watch` and `mix metrics history` print CPU as a share of the machine
with one decimal, from the reply's `cores`, as MixLab does. `--json` prints the replies as they are,
`cores` included, so a script still gets the unit limits are declared in and the divisor beside
it. The command reference (`scripts/check-docs` reads it) states the unit.

## MixLab

**This task is MixLab's.** The tray panel, the Dashboard and the Metrics screen each show CPU as a
share of the whole machine with one decimal, over the last five seconds. The Services screen's
limits panel keeps its per-core unit and says so. No new method: the data arrives on the frames and
histories the screens already read.

## Testing

- **Proto:** `MetricsFrame` and `MetricsHistory` round-trip with `cores`.
- **The daemon's smoothing, as a pure function over readings with timestamps:**
  - five readings of `[0, 1.56, 0, 3.12, 0]` publish their mean, `0.936`;
  - a reading older than five seconds is dropped;
  - one reading publishes itself;
  - all `None` publishes `None`;
  - a subject absent from a tick is absent from the frame.
- **The daemon's two outputs:** the minute accumulator receives the raw readings, so its `cpu_peak`
  over the same sequence is `3.12`, not the mean; and the published frame carries `cores` equal to
  the host's.
- **MixLab (`vitest`):** `formatCpu`:
  - `(0.53, 12)` → `"<0.1%"`;
  - `(7.3, 12)` → `"0.6%"`;
  - `(150, 12)` → `"12.5%"`;
  - `(0, 12)` → `"0.0%"`;
  - `(null, 12)` → `"—"`.

  The `DaemonUsage` bar width is the share of the machine.
- **`mix`:** the render tests for the three commands print one decimal of the machine; the `--json`
  test shows `cores` beside the unchanged `cpu_percent`.
- **By eye:** `npm run dev:app`, the tray panel open for thirty seconds on a machine with the dev
  daemon and four services. The Daemon row stays within a tenth of a percent between refreshes.

## Out of scope

- **What the daemon spends.** T190 and T190b did that; this task changes what is shown, not what
  is measured.
- **Memory.** `rss_bytes` is already in bytes and read the same way everywhere.
- **The limits' unit.** Changing how a cap is declared would be a change to `ResourceLimits` and
  to every stored limit, for no gain in what a person sees about usage.
