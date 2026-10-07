# Phase 40 — What the removal walk found

*Goal: a blueprint applied on a server somebody else has used finishes rather than stopping at a
taken name; a service that cannot start says why, and a missing credential names its repair; the
tests and `scripts/watch-ci.sh` say what they found on the machine they ran on.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-08-t202-what-the-removal-walk-found-design.md](../specs/2026-10-08-t202-what-the-removal-walk-found-design.md).
The walk itself is T182a's, in phase 9; the two findings that needed no code — a test suite's
leftover Keychain items, a development home from before v0.0.7 — are recorded there and not here.

---

- [x] **T202** A foreign account gets the blueprint a name of its own. When `database.create`
      refuses the plan's account as somebody else's (T77a, D3), the apply step tries `<user>-2`…`-9`
      against the same database and takes the first that is free or ours; `StepResult::Done` gains a
      `note` that names the account used and says when the database was already there, printed by
      `mix blueprint apply` and shown under the step in MixLab's apply dialog. No account's
      password is ever reset.
- [x] **T202a** A start that fails carries the note it wrote: `Registry::start` hands back the
      runner's detail with the reason, so `service.start`, the blueprint and *Start again* say
      *did not start: the environment entry MYSQL_PWD: no credential is stored at …* instead of *the
      process could not be started at all*; and a database service's missing superuser entry says
      in the same note that `mix service reset-credential <id>` generates one and keeps every
      database (T127).
- [x] **T202b** Tests that read the machine: the lookup test asks for a `.invalid` name (RFC 6761),
      the three `.test` diagnostic tests skip with a printed reason on a Mac that routes `.test` itself
      (`/etc/resolver/test`), and the uninstall occupant sleeps six hundred seconds, not thirty.
      The skip was seen on the Mac that found it, with the released build installed.
- [x] **T202c** `scripts/watch-ci.sh` closes `--once` with the failed jobs counted and named, and
      opens a failed run's report with the same line, so a reader of the last line cannot take a red
      run for green. Checked against run 37647792800 (*2 of 45 jobs failed: test / test
      (windows-latest), lint / lint*) and 37651545855 (*success*).

**M40** `laravel-1` applies on a server that already has a foreign `laravel-1` account, finishes,
and names the account it used.
