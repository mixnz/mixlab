---
status: approved
date: 2026-09-28
task: T190b
---

# T190b — The idle sweep does not render to look

Phase 7, after [T190a](2026-09-28-t190a-what-the-daemon-spends-while-it-waits-design.md).
2026-09-28.

## The case

T190a found that with nobody watching, `mixengined` on Windows spends 60–95 ms every 30 s, about
0.2–0.3% of a core and close to half of what it spends at rest. The burst is the idle sweep.

`services::idle::Sweeper::sweep` runs every `idle_check_seconds` (30 by default), and its first
line is `self.registry.graph()`. `graph()` asks the spec source for `declared()`. The spec source
is `Rendered(Generator)`, and `Generator::declared()` does not only render: **it installs**. For
every service it resolves certificates (`served::served`, reading every leaf pair), renders every
document, stages and compares each file against what is on disk, creates the directories the
configuration names, and records first-run and provisioning state. `graph()` then hands over,
remembers rituals and remembers provisioning. The registry needs that on every path that changes
something. The sweep needs none of it.

What the sweep actually reads from the graph (`idle.rs`):

- `graph.spec(id).idle()`: each running service's idle policy, which holds the probe and the
  delay;
- `exemptions(&graph, id, &running, &warm)`: who depends on whom, so a dependency of a running
  service is never stopped;
- `self.stop(&graph, id, after)`: the same graph again, when a verdict says stop.

Those change when a service, a site, a setting or a certificate changes, which happens a few times
a day. The sweep asks 2,880 times a day.

## Principle

**Looking is not changing.** A periodic question may be answered from what the last change left
behind. The one moment the sweep acts, stopping a service, it acts on the present.

## D1. The registry keeps the graph its last walk made

`ServicesRegistry` gains a cache of the last `ServiceGraph` that `graph()` produced, and the moment
it was produced. **Every existing call of `graph()` still renders and installs, and refreshes the
cache on the way out.** Nothing that changes a home skips a render because of this task.

The sweep asks a new `graph_for_sweep()` instead:

- **a graph younger than `SWEEP_GRAPH_AGE` (10 minutes) is returned as it is**;
- anything older, or no graph at all, goes through `graph()` as today.

Ten minutes is the most a sweep may observe with a stale policy. The cost of it is bounded by D2.
With nothing else happening, that is one render every ten minutes instead of every thirty seconds:
about 90 ms / 600 s, or 0.015% of a core.

**Every path that changes what the graph is made of already walks it.** Starting, stopping,
creating, a site change, `service idle` and the others reach `graph()` or `walk()` on their own way
(`services/mod.rs`, `api/create.rs`, `api/rpc.rs`, `extensions.rs`, `updates.rs`). So the cache is
refreshed by the change itself, not by a timer. The plan checks this path by path. A mutation that
turns out not to reach `graph()` gets an explicit `forget_graph()` call, and a test that says so.

## D2. A stop is decided on a fresh graph

When a sweep's verdict is `Stop`, **the sweep renders once more (`graph()`) before it acts**, and
recomputes the verdict's two inputs, the policy and the exemptions, from that graph. If the fresh
graph no longer has an idle policy for the service, or now exempts it, the stop does not happen and
the tally for that service is reset. Stopping is rare: it happens once per service per idle period,
thirty minutes for a PHP pool by default. So the extra render costs nothing that shows.

This is what makes the staleness in D1 safe. The worst a ten-minute-old graph can do is:

- **observe with an old probe** (an old port or URL). The observation comes back
  `Unmeasurable` or `Busy`, and the service is not stopped: the safe direction;
- **count towards a stop with an old delay.** The count is judged again against the fresh policy
  before anything is stopped.

## D3. What stops happening, said out loud

Today every sweep also **rewrites any generated file someone edited by hand within 30 s**, as a
side effect nobody designed. After this task that happens at the next change or within ten minutes.
`CLAUDE.md` already says generated configuration is disposable and never parsed back, and
`mix doctor` reports drift (T47b). Nothing documented promised a 30-second repair. The feature doc
for idle stopping gets one sentence saying the sweep does not regenerate configuration.

## MixLab

**No screen changes and no new method.** The Services screen and the tray draw the same states. A
PHP pool is still idle-stopped after the same delay, and started by the same request. The only
difference a person can see is the daemon's CPU figure in the tray and on the Dashboard with nobody
using anything.

## Testing

- **A sweep does not render when the graph is fresh.** Use a counting spec source (the pattern of
  `services/fixture.rs`): two sweeps a few seconds apart call `declared()` once. After
  `SWEEP_GRAPH_AGE`, driven by tokio's paused clock, the next sweep calls it again.
- **A change refreshes what the sweep sees.** A service started through the registry between two
  sweeps is in the second sweep's graph without the sweep rendering.
- **A stop renders first, and a changed policy saves the service.** Set up a fake service whose
  cached policy says stop after one period, and whose fresh render says no idle policy. The sweep
  does not stop it, and it resets the tally.
- **A stop on a fresh graph still happens.** The same setup with the fresh render agreeing: the
  service is stopped, and `declared()` was called exactly once for the stop.
- **The measurement.** A per-second timeline of the daemon's CPU time, as in T190a, on the real
  home with MixLab closed: the 30-second burst is gone, and the floor is recorded before and after
  in six 20-second windows.

| Target, the machine of T190a | Before | After |
| --- | --- | --- |
| Burst every 30 s | 60–95 ms | none |
| Daemon, nobody watching | 0.7% | **≤ 0.45%** of one core |

## Out of scope

- **Making `declared()` cheaper.** A render that installs is the right thing for every path that
  changes a home. This task stops calling it when nothing changed.
- **The other callers of `graph()`.** Each of them answers a request or follows a change, so it runs
  when somebody asked.
- **The sweep's period.** Thirty seconds stays. It is the observation that has to be frequent, not
  the rendering.
