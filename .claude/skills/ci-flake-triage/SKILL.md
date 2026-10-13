---
name: ci-flake-triage
description: Use when a CI job is red and the failure looks intermittent, environmental or "probably a flake" — before rerunning anything, and whenever tempted to rerun a second time.
---

# A red job is evidence, and a rerun destroys it

Rerunning is not triage. It is what is done **after** the evidence has been read and after the next
occurrence has been made readable. A job rerun without either is a failure this project paid for and
then threw away.

## The order

**1. Read the failure before anything else.** Not the summary — the log.

```bash
gh run view <run> --json jobs --jq '.jobs[] | select(.conclusion == "failure") | "\(.name)\t\(.databaseId)"'
gh api --allow-escape-sequences repos/mixnz/mixlab/actions/jobs/<id>/logs \
  | sed 's/\x1b\[[0-9;]*m//g' > /tmp/job.log
```

`--allow-escape-sequences` is not optional: without it the API returns a refusal, and a script that
greps the result silently compares nothing.

**2. Ask what the product already told you.** This daemon distinguishes *a port held by another
program*, *a process that exited*, *a superuser that refused* and *a timeout*. Which one it chose
rules out the others. A `ReadyTimeout` means the process was alive and not listening — so "the port
was taken" is already answered, and so is "it crashed".

**3. Decide which of three things this is.**

| It is | Signature | Do |
| --- | --- | --- |
| a real bug with a narrow window | a race in the test or the product; the log names both sides | fix it, with the failing test first (testing.md rule 6) |
| a real limit measured too tight | a timeout, a retry count, a buffer, on the slowest runner | move the limit where the measurement says, and say in the comment which run measured it |
| the infrastructure | GitHub's own API 5xx, a runner lost, a network refusal from `actions/*`, a queue that never started | rerun, and say so in the report |

Only the third row justifies a rerun on its own.

**4. If the log cannot decide it, that is the finding.** Do not keep guessing and do not keep
rerunning. Make the next occurrence readable — testing.md rule 8, and the `tests-that-say-why`
skill — and say plainly that the cause is still unknown. A fix that only improves the next report
is a fix; an afternoon of reruns is not.

## Two worked examples from this repository

**A race that looked like a flake.** `test (macos-latest)` failed once on
`a_body_larger_than_the_limit_is_refused_rather_than_read`. The log named `hyper::Error(BodyWrite,
BrokenPipe)`: the daemon answered `413` and closed while the client was still writing, and which
side won was timing. The test was rewritten to write on a task of its own and read the answer as it
arrives. Rerunning would have passed, and the race would still be there.

**A limit measured too tight.** `services (windows, web)` failed twice on a php-fpm `ReadyTimeout`,
two months apart, and passed on both reruns. The fifteen-second limit existed for exactly the case
that was failing — a first start on Windows — so it went to thirty, *and* the daemon now logs what
the service last printed, because neither of the two failures had left anything to read.

## Two known signatures

**`scripts/watch-ci.sh` prints an extract, and the extract can be the noise.** cargo-deny's container
prints a `rustup` backtrace ("override toolchain … is not installed") on every run, green ones
included, then installs the toolchain and carries on. Once it was taken for the failure while the
real one — a banned crate — sat thirty lines lower. Read the full log the script saved
(`$TMPDIR/mixengine-ci-<run>.log`), grep to the end of the job's part (`error[`, `banned`,
`FAILED`), and compare with a recent green run on master to know which lines are always there.

**`warm_start` on `bench (ubuntu-latest)` is bimodal.** Each round of
`three_services_start_together_inside_the_budget` lands near 4 s or near 12 s, and the 10 s budget
sits between the two, so the median of five is decided by how many rounds fell slow. Red here is
not yet a regression: dispatch a run of master at the same time and compare medians. Only a fast
master beside a slow branch is a regression to look for.

## What to write in the report

Name the run, the job, the line in the log, and which of the three rows above it was. When it is the
third, say that the evidence was infrastructure, not a shrug. When the cause is still unknown, say
that too, and name what was changed so the next one is readable.

## The budget

Time-box the investigation. When it runs out and the cause is still open: land the readability
change, rerun, and write down what is known. That is a complete answer. "Rerun and move on" is not,
and neither is an open-ended hunt.
