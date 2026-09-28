---
status: implemented
date: 2026-09-28
task: T190a
---

# T190a — What the daemon spends while it waits

Phase 7, after [T190](2026-09-28-t190-a-reading-on-windows-does-not-list-the-machine-design.md).
2026-09-28.

## The case

T190 made a metrics reading cost 41 µs on Windows. The daemon still shows **0.8–1.4% of a core**
while the tray panel or the Dashboard holds `/metrics` open, and **0.63–0.70%** with nobody watching
at all. Neither is the reading. The person looking at the tray still sees a number that moves
between 0 and 3%, and cannot tell it apart from a daemon that is busy.

What is known, measured on the real home (seven services, four running):

- **The idle floor is spread across tokio's workers.** Sampling each thread's CPU time over 20
  seconds found about 4.7 ms/s spread over five threads, with none of them dominant. A new thread
  appeared inside the window, which is the blocking pool starting a thread: some periodic work
  blocks (SQLite, file I/O or a process spawn) and is handed to `spawn_blocking`.
- **The DNS server is quiet.** The daemon's log held 631 lines for the whole session.
- **Four running services mean four supervisors waking every 250 ms** (`runner.rs` `WATCH`) to ask
  whether their process has exited, plus a health probe every interval. Each wake is small. Whether
  sixteen a second add up to anything has not been measured.

Thread sampling cannot say more. Windows counts thread time in 15.6 ms quanta, so a thread that ran
for 2 ms reads as 0 or 15.6. **A sampling profile with stacks is the only instrument that answers
this.** Guessing would be the thing T190's spec refused to do.

## D1. The instrument

**ETW, recorded with `wpr`**, which ships with Windows (`C:\Windows\System32\wpr.exe`), sampling
CPU with stacks:

```
wpr -start CPU -filemode
(60 seconds nobody watching, then 60 seconds with one `mix metrics --watch`)
wpr -stop t190a.etl
```

`wpr` needs an administrator token. It runs through `Start-Process -Verb RunAs`, so **the person
clicks one UAC prompt to start and one to stop**. Nothing else in this task needs elevation.

**The daemon under the trace is the build the release ships, with its symbols.** It is built as
`packaging/stage.sh` builds it (`MIXENGINE_RELEASE=1`, `crt-static`), so it resolves the real home.
Its `mixengined.pdb` sits beside the executable, so every stack names Rust functions. It is swapped
in the same way T190 measured: rename the installed binary, copy the build in, restart from MixLab,
then put the original back.

**Reading the trace needs a tool the machine does not have.** There are two candidates:

| Tool | Install | What it gives |
| --- | --- | --- |
| **Windows Performance Toolkit** (`xperf`, `wpaexporter`) | Windows ADK installer, WPT feature only: 459 MB installed | CPU-by-stack tables as CSV, with the `.pdb` symbols. The tool this trace format was made for. |
| `samply` | `cargo install samply` | Records and symbolises itself (it also needs admin on Windows). Output is a Firefox Profiler JSON, which is harder to summarise as text. |

**Recommended: the Windows Performance Toolkit.** `wpaexporter` writes tables a reader can check
line by line, and it reads the `.etl` that the built-in `wpr` records. The install is a download
from Microsoft, and it happens only on the person's say-so.

## D2. What the task delivers

**A list, not a fix.** For each of the two states (nobody watching, one stream open), the task
records:

- the daemon's CPU over the window, measured as T190 measured it;
- the top stacks by CPU inside `mixengined.exe`, grouped by the task or loop they belong to
  (supervisor watch, health probe, metrics tick, `services::records`, event stream, log writer, the
  blocking pool);
- for each group, milliseconds per second and the share of the total.

These go into this spec's *Findings* section. **Every group that costs more than 0.1% of a core
becomes a task of its own** in `phase-7-efficiency.md`, right after this one, each with its own
spec. A fix to a supervisor loop and a fix to the metrics tick have nothing in common, and one
branch carrying both would be reviewed as neither.

## MixLab

**No screen changes.** The tray panel and the Dashboard read the same frames. This task only
measures the number they draw. The one part MixLab plays is the setup: the daemon under the trace
is started from MixLab, so the home and credentials are the ones a user has.

## Testing

Nothing is shipped, so nothing is tested. The result is checked by the numbers adding up: the
groups found in the stacks sum to the CPU measured over the same window, within the sampler's
resolution (1 ms samples). If a large share lands in frames with no symbols, the trace is redone
with symbols fixed, and not written up as "unknown".

## Out of scope

- **Any fix.** Each finding becomes its own task (D2).
- **macOS and Linux.** The report was about Windows, and the tools differ. If a Windows finding
  turns out to be cross-platform (a loop, not a syscall), its own task says so.

## Findings

Measured on 2026-09-28 on the real home: seven services, four running (caddy, mysql@5.7,
php-fpm@7.3.33, redis@main). The MixLab window was closed.

### MixLab is not the floor

With the window closed and the daemon started by `mix`, six 20-second windows read 0.31, 0.31,
0.86, 0.70, 1.17 and 0.86% of a core. That averages 0.7%, the same as with the window open
(0.63–0.70%). Whatever the daemon spends while it waits, it spends on its own.

### The floor comes in bursts every thirty seconds

A per-second timeline of the daemon's CPU time, taken without any trace running, shows bursts of
**62–94 ms** at t=4, 34, 63 and 93 s. That is one every 30 s, on top of a few milliseconds scattered
across tokio's workers. Two ETW traces put the bursts at the same period (t=13 and t=43 in the
second trace).

**The burst is the idle sweep rendering every service's configuration.** `services::idle` sweeps
every `idle_check_seconds` (30), and its first step is `registry.graph()`. `graph()` asks the spec
source for `declared()`, and the spec source is `Rendered(Generator)`. So each sweep runs
`Generator::prepare` and `Generator::documents` for every service. That means reading every
certificate pair (`certs::leaf::read_pair`), rendering each recipe (`recipes::caddy`,
`generate::ca::render`, `recipes::postgres::address`), parsing TOML, creating directories, and
opening a SQLite transaction. All of this runs whether or not anything has changed since the last
sweep. The stacks in the burst seconds name these functions, and file-system time
(`wcifs.sys`, `DirBuilder::mkdir`, `File::open`) sits under them.

**Cost: about 2–3 ms/s, or 0.2–0.3% of a core**: 60–95 ms every 30 s, measured without a trace.
That is close to half of the floor. It is **T190b**.

### What else was looked at, and why it is not a task

| Suspect | Measured | Verdict |
| --- | --- | --- |
| The per-minute metrics listing (`Sampler::measure` with a machine-wide snapshot) | 6.9 ms once a minute (T190's bench), about 0.01% | known, below the line |
| `mixengine_platform::network::Network::interfaces` (`if-addrs`), from the sharing check every 30 s | **2.6 ms** a call on this machine, about 0.01% | below the line |
| The idle probe of a php-fpm pool (`HttpCounter`) | one `hyper` request on a fresh TCP connection | the cost is in caddy and php, not the daemon |
| The DNS server | 631 log lines for the whole session | quiet |
| A process spawned around the metrics tick (`restricted::spawn_from`) | about 9 samples in the burst second at t=31 | caller not resolved (async frames carry no symbol); well under 1 ms/s |
| The rest: `mio` select, tokio park, the supervisors' 250 ms `WATCH`, spread over five worker threads | a few ms/s under ETW, which inflates kernel-heavy code | no single function reaches 1 ms/s outside the burst |

### How far the instrument can be trusted

- **ETW inflates what it measures.** With stack-walking on every kernel event, the daemon read
  1.2% idle and 2.0% with a stream, where it reads 0.7% and 0.8–1.4% without a trace. A machine-wide
  snapshot cost about 33 ms under the trace against 6.9 ms without it. The traces say **who**; the
  plain CPU-time measurements say **how much**.
- **The first build folded identical functions** (`/OPT:ICF`, the MSVC release default), and many
  unrelated frames symbolised as one `drop_glue`. That made callers unreadable. The second trace
  used a build linked with `/OPT:NOICF` and frame pointers, and its leaf and near-leaf frames are
  the ones quoted above. Async state machines still resolve to the nearest exported symbol, which
  is why a caller several frames up can appear as an unrelated `type_id`.
