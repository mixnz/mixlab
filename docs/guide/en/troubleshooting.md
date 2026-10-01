+++
title = "When something is wrong"
slug = "troubleshooting"
order = 14
summary = "mix doctor first, then the four commands that answer the questions people actually have, and one file that holds everything a bug report needs."
+++

# When something is wrong

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

## Start here

```bash
mix doctor
```

It examines the machine and says what is wrong with it. It **reports and repairs nothing** unless
you ask, and it exits non-zero when it found a problem, so a script can ask too.

```bash
mix doctor --repair
```

Repairs everything that can be repaired. Anything inside MixEngine's own home is fixed at once;
anything needing an administrator is queued, shown to you, and then granted in **one** prompt for
the whole batch. `--yes` skips the confirmation before that prompt.

## The four questions people actually have

### "Is anything running?"

```bash
mix status
mix service list
```

`status` is the daemon: its version, its home, and what it is supervising. `service list` is the row
per service with what each one is doing.

### "Why does this name not open?"

```bash
mix domain status blog.test
```

Four facts, answered separately rather than as one verdict: whether the name is declared, how it is
routed, whether it resolves on this machine right now, and whether anything answers on it. The one
that is `no` is the one to fix.

### "Why is the padlock not green?"

```bash
mix cert status
mix cert ca-status
```

`cert status` opens a real connection and reports the certificate that was actually presented, which
is the only thing a browser ever sees. `ca-status` says what the authority is. If the authority is
not trusted, `mix doctor --repair` is what puts it back.

### "Which PHP is this, and why?"

```bash
mix runtime resolve php
```

The version this directory gets, **and which of the four sources decided it**. That second half is
what you want when the answer is not the one you expected.

## Reading the logs

```bash
mix service logs caddy --follow
mix service logs mariadb@main -n 200
```

`--follow` survives the service crashing and being restarted: what is being followed is the service,
not one run of its process. The daemon's own log is `logs/daemon.log` inside MixEngine's home.

For a long operation, such as an install or a blueprint apply, the job is where to look:

```bash
mix job list
mix job status <id>
mix job logs <id>
```

`mix job logs` only answers for a job that runs somebody else's program, which today means a
blueprint running its own scaffold command. Everything else a job does is reported as progress and
as a result, and this says so rather than pretending output was lost.

## Common situations

**A port is already in use.** Something else on your machine has it. `mix service create --port`
picks another for a new service; for one that exists, delete it and create it again on a different
port. The data directory is kept.

**The daemon will not start.** Read `logs/daemon.log` in the home. `mix status --no-autostart` asks
whether one is running without starting one, which is the right question when you are diagnosing
rather than working.

**A command needs a version that is not installed.** MixLab says so and names the exact
`mix runtime install` command to type. When a *range* was asked for, it cannot know which version
satisfies it and points at `mix runtime available` instead.

**Something asked for an administrator and you said no.** Nothing is half-applied. `mix elevation
status` shows what is still waiting, and `mix elevation grant` asks again.

## MixLab is using too much disk

`mix disk` breaks this home down into five categories and says, for each one, what would take it
back:

```
          size      reclaimed by
runtimes  700 MiB   runtime.uninstall — one runtime at a time, and never one a running pool is using
data      1200 MiB  these are your databases, and nothing in MixLab deletes them
logs      40 MiB    `mix cleanup` — 30 MiB in 4 file(s)
certs     < 1 MiB   every site would lose HTTPS until `cert.issue` ran again …
cache     90 MiB    `mix cleanup` — 90 MiB in 12 file(s)
other     310 MiB   packages, generated config, the database
```

`mix cleanup` takes back the last two and nothing else. It removes rotated log files
(`daemon.log.1`, a service's `current.log.2`) and empties the download cache. It does not touch the
log files being written right now, this home's crash reports, your databases, your installed
runtimes or your certificates: it matches file names rather than sweeping the home, so there is no
argument you can give it that would reach them.

`--keep-logs` and `--keep-cache` leave one of the two alone. `--yes` answers the confirmation in
advance, which is how a script says yes.

It refuses while another job is running, because emptying the cache would delete the file a download
is resuming from. Wait for the job, or cancel it with `mix job cancel <id>`.

To free more than that: `mix runtime list` and `mix runtime uninstall <runtime> <version>` are what
reclaim `runtimes/`, and `mix package list` and `mix package uninstall <package> <version>` most of
*other*.

## When MixLab itself hits a bug

If the daemon runs into a bug in its own code, it writes a small file into `logs/crashes/` inside
MixEngine's home. `mix doctor` tells you one is there as a note, never as a problem, so it does
not change the command's exit code.

**What is in it**: where in MixLab's own source the bug happened, the positions in the program of
the functions around it, which version was running and which operating system. That is the whole
list. The positions are plain numbers that mean something only for that exact version, and
MixEngine's maintainers turn them back into function names.

**What is not in it**: none of your file paths, none of your site or project names, and no
passwords. That is true because of what the file is *allowed to hold* rather than because something
was filtered out of it afterwards. So you can attach one to a public bug report as it is, without
reading it first.

The message the crash printed is the one part that can mention a path of yours, so it goes to
`logs/daemon.log` instead. That file is worth sending too, but read it first. See below.

**Nothing sends any of this anywhere.** There is no server to send it to. The twenty newest are kept
and older ones are removed. If you would rather no such file was written at all, put this in
`config.toml`:

```toml
[crash]
enabled = false
```

The daemon log still records that a crash happened.

## Reporting a bug

```bash
mix doctor --bundle
```

One archive with everything a bug report needs: what `doctor` found, this daemon's status, what this
machine is, any crash reports, and the tail of the log. `--out` copies it somewhere of your
choosing.

**What it deliberately leaves out is named in the archive itself**, so nobody has to guess whether a
missing section is a redaction or a failure. It is a plain archive: open it and look before you send
it anywhere.

Every `mix` command also takes `--json`, which is often the fastest way to show somebody exactly
what you saw.
