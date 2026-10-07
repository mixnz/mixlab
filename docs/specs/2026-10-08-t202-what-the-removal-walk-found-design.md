---
status: approved
date: 2026-10-08
task:
  - T202
  - T202a
  - T202b
  - T202c
---

# T202 — What the removal walk found (design)

Roadmap phase 40. Walking T182a's removal on a real Mac on 2026-10-07, and bringing a development
home that pre-dated v0.0.7 back up afterwards, met eight things that were not the removal's. Two are
product faults, two are gaps in what a failure says, two are tests and a script that read the machine
they run on, and two need no code. This design takes the six that do and records the two that do
not, so the walk's findings are not left in a conversation.

## Goal

A blueprint applied on a server somebody else has used finishes rather than stopping at the first
name that is taken; a service that cannot start says why in the sentence the person reads first,
and a start that fails for want of a credential names the command that repairs it; the test suite
and `scripts/watch-ci.sh` say what they found on the machine they ran on, instead of assuming it is
CI's.

## Not in scope

- **Taking over a foreign account.** T77a's D3 stands — a keyring entry is the deed of ownership,
  and a password a person knows is not one (T77b). This design routes *around* a foreign account; it
  never resets its password, and adds no `--adopt`.
- **A database somebody else made is never emptied.** T77a already grants it and reports it as
  `existing`; this design only makes the blueprint say so where a person reads.
- **Recovering a home whose `mixengine.db` was recreated beside initialised data directories.**
  `mix service reset-credential` (T127, D2: a missing entry is generated) and `mix service adopt`
  already are that recovery. What was missing is the pointer to them, which T202a adds.
- **Credentials from other homes left in the store.** Beside the two entries of the home it removed,
  the Mac's Keychain held `4621e284fc91/…`, `mariadb@main/root` and `mysql@main/root` from earlier
  homes. That is T182c's question asked of the credential store rather than the trust store, and
  T182c's roadmap line is widened to say so; nothing here claims them.
- **Keychain items a test suite wrote.** The same Keychain held twenty-five `mixengine` items under
  home ids no install ever had (`<id>/extensions/phpmyadmin/config`, one `…/mailpit/config`), dated
  2026-09-15 to 2026-09-24. T184 (2026-09-24) gave every build that is not a release a credential
  file of its own, and nothing newer than that date is there, so this is a leftover and not a fault.
  It is removed by hand, once:

  ```bash
  security dump-keychain | grep -o '"acct"<blob>="[0-9a-f]\{12\}/extensions/[a-z]*/config"' | cut -d'"' -f4 | while read -r account; do security delete-generic-password -s mixengine -a "$account"; done
  ```

- **A development home from before v0.0.7.** Its database carries the twenty-seven development
  migrations that `9f6748f1` folded into `0001_initial.sql`, and the daemon's refusal already says
  what to do (move it aside). No release ever wrote such a database. Nothing to build.

## Found on 2026-10-07

Mac14,3, macOS 15.7.3, a development home from 2026-09-19 brought up on a daemon built that day.

1. **The blueprint stopped at a taken name.** `laravel-1`'s apply ended with *an account called
   laravel-1 already exists on mariadb@main, and MixEngine has no credential for it; left in place:
   the directory …, the database laravel-1 on mariadb@main*. The account was one an earlier home of
   this machine had made — the same thing a person gets from a `mysql` prompt or phpMyAdmin on a
   server MixEngine runs, which it cannot see. The refusal is right; the dead end is not. The only
   way on was to drop the account by hand and apply again.
2. **The blueprint said the service could not start, and not why.** Before that, with `mariadb@main`
   unable to resolve its superuser password: *mariadb@main did not start: the process could not be
   started at all*. The runner had written the reason — *the environment entry MYSQL_PWD: no
   credential is stored at mixengine/e0272f2b7089/mariadb@main/root* — into T200b's `last_failure`,
   and `Registry::start` hands back only the `StateReason`, whose `Display` is that sentence.
3. **Nothing named the repair.** The reason above is exactly the case `mix service reset-credential`
   exists for (T127: the entry is generated when missing and written into the data directory), and
   nothing in `daemon.log`, `mix service list`, the Sites screen or the start error said so.
4. **Two tests assume the machine wires nothing.** `domains::lookup::a_name_nothing_routes_resolves_to_nothing`
   asks the system for `t46-<pid>.test`, and `doctor::a_name_nothing_resolves_is_a_problem…` declares
   `blog.test` under a home whose DNS is not wired and expects it unreachable. On a Mac with
   MixEngine installed, `/etc/resolver/test` routes every `.test` name to the installed daemon, and
   both answers are that daemon's to give. Both failed on this machine while the released build was
   installed, and passed once it was removed.
5. **One test is flaky under a full parallel run.** `uninstall::a_program_running_from_the_home_blocks_with_exit_code_three`
   starts a copy of `/bin/sleep 30` from inside the home and expects the dry run to name it. Under
   `cargo test --workspace` on a loaded machine the daemon's start and the plan took longer than
   thirty seconds once, and the occupant had exited before the plan looked.
6. **`watch-ci.sh --once` was misread.** With two jobs red it printed their `FAILED` lines among
   forty-three `ok` lines and closed with *run …: in_progress — 43/45 jobs settled*. A reader who
   keeps the tail sees a run that looks green. The completed form prints the failing steps, but
   never a count or the names in one line.

## Decisions

**D1 — A foreign account gets the blueprint a name of its own (T202).** In the apply executor's
`CreateDatabase` step, `database.create` is called with the plan's `user`. `Databases::create`
today answers the wire `Error`, whose `conflict` code other refusals share, so the work moves into
an inner `make()` that answers `mixengine_core::Error`, with `create()` left as the wire wrapper the
RPC uses. The step calls `make()`, and when it refuses with `Error::AccountNotOurs` — the typed
variant, never the code — the step tries again with `<user>-2`, then `-3`, up to `-9`, with the same database
name, and takes the first the server either does not have or MixEngine owns. The suffix is appended
to a base cut to fit `IDENTIFIER_LIMIT` (thirty-two), so a long name is shortened rather than refused;
`validated_identifier` accepts the result by construction (lower-case, digits, a hyphen not at either
end). Ten names all foreign is a failure with T77a's own sentence and a hint that names the last one
tried. The step reports what it did (D2). Nothing else changes: the ledger's `Kept::Database` is
written before the first try, the database is granted to whichever account was made, and a foreign
account's password is never touched.

**D2 — A step that did something other than the plan says so in its outcome (T202).**
`StepResult::Done` gains `note: Option<String>` (`#[serde(default, skip_serializing_if)]`, so a
reader that ignores it reads the wire as before). Two sentences can fill it, joined when both apply:
*the account laravel-1 is somebody else's, so this project's is laravel-1-2* and *the database
laravel-1 was already there and was not emptied* (`Provisioned.database == Made::Existing` in the
answer). The note is
also the job's progress line as the step completes, so a person watching sees it then, and the
outcome keeps it for whoever reads the report later. `mix blueprint apply` prints it indented under
the step; MixLab's apply progress shows it under the step's row. The hint `mix database create`
already carries (*`--user` picks another name*) stays; a blueprint has no `--user`, and after D1 it
needs none.

**D3 — A start that fails carries the note it wrote (T202a).** `Registry::start`'s `failed` becomes
`Option<(ServiceId, Option<StateReason>, Option<String>)>` — the detail `give_up` records as
`last_failure` travels with the reason — and `ensure_running`/`start_one` say
*`{id}` did not start: `{detail}`* when there is one, falling back to the reason's `Display` as now.
`service.start`, the blueprint's `EnsureService` step and the Sites screen's *Start again* all go
through it, so the sentence a person reads first is the runner's and not the state machine's.

**D4 — A missing credential names its repair, in the note itself (T202a).** The runner knows which
`EnvValue::Keyring` entry would not resolve. Its detail becomes *the environment entry MYSQL_PWD: no
credential is stored at mixengine/<home>/mariadb@main/root; `mix service reset-credential
mariadb@main` generates one for this home and writes it into the data directory, keeping every
database*. The same text reaches `last_failure` (T200b: Sites, Add-ons, `mix service list`,
`mix site show`), the start error (D3) and `daemon.log`, with no new `StateReason` — a variant
would change `mixengine-proto`, which the helper lock fingerprints, for a sentence. Only the entry a
recipe's `Ritual` declares as its own (`SecretSpec` — `root`, `postgres`) gets the second half,
because that is the one `reset-credential` can write; a missing credential of any other kind keeps
the first sentence alone.

**D5 — Tests that need "a name nothing routes" ask for one no machine routes (T202b).** The lookup
test asks for `t46-<pid>.invalid`: RFC 6761 reserves `.invalid` to resolve nowhere, and MixEngine
never wires it. The doctor test cannot move its site off `.test`, so it does what `sharing.rs` and
`limits.rs` do on a machine that lacks what they need: when `/etc/resolver/test` exists it prints
*skipped: this machine routes `.test` itself (`/etc/resolver/test`), so whether blog.test resolves is
not this home's to decide* and returns. CI's runners never carry that file; a developer's Mac with
MixEngine installed does, and now says so rather than failing. Linux and Windows machines with an
installed MixEngine would wire `.test` through `resolvectl` or an NRPT rule; this design does not
detect those, because nothing has measured the failure there, and says so here.

**D6 — The occupant outlives any plan (T202b).** The uninstall test's `sleep 30` and `ping -n 30`
become `600`; the test already kills the occupant on its way out. If the test flakes again the
cause is not this one, and the hypothesis recorded in finding 5 is wrong.

**D7 — `watch-ci.sh` counts what failed, in one line, both ways (T202c).** The poll counts the
`failure`/`cancelled` conclusions it prints and keeps their names. `--once` on a run still going
closes with *run N: in_progress — 43/45 jobs settled, 2 failed: lint / lint, test / test
(windows-latest)*; a finished run that did not succeed opens its report with *run N: failure — 2 of
45 jobs failed: …* before the extract. A green run's line is unchanged. The exit status is
unchanged: 2 while running under `--once`, CI's otherwise.

## MixLab

- **T202.** `screens/Blueprints/ApplyDialog.tsx` renders each `StepOutcome`; a `done` with a
  `note` shows it as a second line under the step, dimmed, where `not_run` shows its `why`
  (`mixengine.blueprints.apply.stepDoneNote`, en and vi). No new command; the window renders what
  the daemon returns.
- **T202a.** The Sites and Add-ons failure line already renders `last_failure.detail`, so the repair
  sentence of D4 appears with no window change; *Start again*'s error toast shows D3's sentence
  because it shows `service.start`'s error message. Nothing in MixLab runs `reset-credential`: it
  is `cliOnly` in `client-surface-exceptions.json` already, on the reasoning `cert.ca_rotate`'s
  entry gives — an irreversible repair is done deliberately, from `mix`.
- **T202b, T202c.** Tests and a script; the window has no part.

## Testing

- **D1.** The step's loop is written against a narrow trait, `MakesDatabases` (`make` only),
  which `Databases` implements and a test double in `apply.rs`'s tests implements too; the executor
  holds the `Arc<Databases>` as before. Unit tests on the loop: the first name foreign and the
  second free → `laravel-1-2`, one database, the note set; the first foreign and the second ours →
  reused, nothing written; ten foreign → the T77a error with the last name in the hint; a
  thirty-two-character base → cut to thirty, `-2` appended, `validated_identifier` accepts it; any
  other `mixengine_core::Error` → not retried. The pure name sequence is its own function with its
  own tests. `crates/mixengine-cli/tests/blueprint.rs` covers the unchanged happy path end to end.
- **D2.** Serde: `{"result":"done"}` still reads; a `note` round-trips. `mix blueprint apply`'s
  rendering test shows the note indented under its step.
- **D3.** The existing runner test for a missing keyring entry asserts the start error's message
  contains *no credential is stored at*; `Registry::start`'s unit tests carry the detail through.
- **D4.** The runner's env-resolution test asserts the note names `mix service reset-credential
  <id>` for a database service's superuser entry and not for another key.
- **D5, D6.** The tests themselves; D5's skip is exercised by hand on this Mac with the released
  build installed, and recorded in the roadmap line when it has been.
- **D7.** `scripts/` has no test harness; checked by hand against a run with a red job (one exists:
  `37647792800`) and a green one, and the two sentences pasted into the roadmap line.

## Documentation, when it lands

- `docs/features/blueprints.md`: the account a blueprint ends up with, D1's rule and the note.
- `docs/features/services.md`: the sentence a failed start carries (D3) and the pointer to
  `reset-credential` (D4), beside T127's paragraph.
- `docs/operations/build-and-release.md`: `watch-ci.sh`'s closing line.
- `CHANGELOG.md`, `### Fixed`: *A blueprint whose database account name is already taken on the
  server picks the next free one and says so, instead of stopping.* and *A service that cannot
  start says why in the first sentence, and a database missing its password names the command that
  repairs it.*
- Phase 40 in the roadmap, with T182c's line widened to the credential store.
