---
status: approved
date: 2026-10-13
task:
  - T207
  - T207a
---

# Files that fit in one read: a size ratchet, and `mix` split by command (design)

Thirty-four source files in this repository are over 1,500 lines, and the two largest —
`crates/mixengine-cli/src/render.rs` (7,741) and `main.rs` (7,651) — are each more than a person or a
coding agent can hold at once. This design does two things, in this order:

1. **T207 — a ratchet.** A check, in `scripts/gate.sh` and CI's `lint` job, that lets no file grow
   past a limit and no file already past it grow at all. It is green the day it lands.
2. **T207a — `mix` split by command.** `main.rs` moved into one module per command, **without
   changing a single thing `mix` does**. Its line in the ratchet's baseline goes when it is done.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md). `render.rs` is the same
problem one file over and is a follow-up there (T207b) — see [Not in scope](#not-in-scope).

## Why

- **A change to one command reads the whole file.** Both a person and an agent read a 7,651-line
  file in windows, and the windows miss things. `mix site` alone is spread over four places in
  `main.rs`: its clap enum at line 962, its argument types at 1188–1227, its handler at 4129 and its
  route builder at 4330.
- **Large files grow.** The cheapest place to add a function is next to the one it resembles, so a
  file that is already the home of a capability keeps becoming more of it. Nothing says no today.
- [docs/standards/rust.md](../standards/rust.md) already says *modules by capability*; the size
  limit is the measurable half of that rule.

---

# Part 1 — T207, the ratchet

## D1 — a limit, and a baseline that only shrinks

`scripts/check-file-sizes.mjs` counts the lines of every tracked source file and compares each with
**1,500**. A file over the limit is allowed only if `scripts/file-size-baseline.json` names it, and
then only up to the count recorded there:

```json
{
  "limit": 1500,
  "files": {
    "crates/mixengine-cli/src/main.rs": 7651,
    "crates/mixengine-cli/src/render.rs": 7741
  }
}
```

| Situation | Answer |
| --- | --- |
| a file over the limit, not in the baseline | **red**: `is 1,612 lines; the limit is 1,500 — split it by capability (docs/standards/rust.md)` |
| a baselined file longer than its entry | **red**: `is 7,660 lines; the baseline allows 7,651 — it may not grow` |
| a baselined file shorter than its entry | **red**: `is 7,600 lines; the baseline still says 7,651 — run node scripts/check-file-sizes.mjs --shrink` |
| a baselined file at or under the limit, or gone | **red**: `is under the limit now — run … --shrink` |
| anything else | green |

The third row is what makes it a ratchet rather than a ceiling: lines a commit removes from a large
file cannot be spent again by the next one. It costs a one-line change to the baseline in a commit
that shrinks a large file, which `--shrink` writes.

It is the same shape as `apps/desktop/client-surface-exceptions.json`, whose `knownGaps` list only
shrinks, and for the same reason: a rule a repository cannot meet on the day it is written is met by
writing down where it stands and refusing to go backwards.

## D2 — what counts

- **Files:** what `git ls-files` lists with the extensions `.rs`, `.ts`, `.tsx`, `.js`, `.mjs` and
  `.css` — the code someone edits by hand.
- **Not counted:** `bindings/` (generated from `mixengine-proto`), `.sqlx/`, `migrations/` and
  `migrations-archive/` (a shipped migration is never edited), `fixtures/`, and `*.d.ts`.
- **Lines:** as `wc -l` counts them, the whole file, tests included. A `#[cfg(test)] mod tests` is
  part of what a reader opens, and `render.rs`'s 3,000 lines of tests are a large part of why it
  cannot be read in one go. Counting only the code before the test module would also be a second
  rule about where tests go.

Measured on `master` at `908e65e4` with these rules: **34 files** over 1,500 — 33 Rust files and one
`.tsx`. That is the baseline the check lands with.

## D3 — growing a baselined file is a decision, made in the open

`--shrink` only ever lowers an entry or removes one. Nothing writes a larger number. A commit that
genuinely has to grow a large file — a fix that cannot wait for the split — edits the JSON by hand,
and that line in the diff is the review question: *why here, and not in a new module?*

There is deliberately no `--write` that regenerates the baseline from the tree: it would turn the
ratchet into a formality one flag away.

## D4 — where it runs

- **`scripts/gate.sh`**, beside `check-docs.mjs`, so it is answered before every push.
- **CI's `lint` job** (`.github/workflows/_lint.yml`), as `node --test
  scripts/check-file-sizes.test.mjs` then `node scripts/check-file-sizes.mjs`, the pair every other
  `scripts/check-*.mjs` runs as.
- **Not the pre-commit hook.** The hook is enabled per clone (`core.hooksPath`) and so cannot be what
  enforces anything; and refusing a commit mid-work, before a split is finished, is the wrong moment.
  The gate and CI are where a branch is judged.

The check runs in well under a second: `git ls-files` and a newline count, no parsing.

## MixLab

No part in it: a repository check, with no daemon method, no type and no screen. It counts
`apps/desktop/src/**` and `apps/desktop/src-tauri/src/**` like any other source, and today one file
there is over the limit and goes into the baseline.

## Testing

`scripts/check-file-sizes.test.mjs`, beside the script, with a pure function over a file list, a
line count per file and a baseline:

- a new file over the limit is red; one at exactly the limit is green;
- a baselined file that grew is red, one that shrank is red until `--shrink`, one at its entry is
  green;
- a baselined file under the limit or deleted is red until `--shrink`, which removes it;
- `--shrink` never raises an entry;
- an excluded path (`bindings/`, a migration) over the limit is green.

---

# Part 2 — T207a, `mix` split by command

`main.rs` holds the clap definitions of every command, the handler of every command, the helpers
they share, and 300 lines of tests. Two more reasons it splits cleanly:

- **Every command's imports are every command's imports.** The single `use mixengine_proto::{…}` at
  the top names 182 types; nothing says which command needs which.
- **It already is modules, in all but name.** `run()` dispatches each `Command` variant to one async
  function that takes the endpoint, the autostart and `--json`, and the helpers each handler calls
  are, with a handful of exceptions, called by that handler only (measured: see D7).

## D5 — one module per command, under `commands/`

```
crates/mixengine-cli/src/
  main.rs            mod lines, VERSION, Args, Command, main(), docs_command(), run()
  client.rs          + ask(), encode()            — calling the daemon and decoding its answer
  output.rs          rendered(), emit(), report(), PLAIN_TEXT, plain_text()
  parse.rs           the clap value parsers: service_id, idle_after, runtime_kind, runtime_version,
                     version_constraint, job_state, for_seconds, pin
  confirm.rs         + unanswered()               — used by ten commands' questions
  jobs.rs            follow(), report_progress()
  prerequisites.rs   Agreement, agreed_to_prerequisites(), prerequisites_agreed()
  upgrades.rs        upgrade_confirmed(), finish_upgrade()   — runtime's and package's alike
  commands/
    mod.rs           `pub(crate) mod` lines, nothing else
    status.rs        status, storage
    daemon.rs        DaemonCommand, CredentialStoreArg; daemon_stop, daemon_credential_store
    runtime.rs       RuntimeCommand, ExtCommand, WhichPhp, Kind, Which; runtime, install,
                     said_what_it_lacks, which_php, question, target
    package.rs       PackageCommand, Named, WhichPackage
    project.rs       ProjectCommand, WhichProject; project, which, here, working_directory
    site.rs          SiteCommand, WhichSite, SiteKindArg, SiteStateArg; site, site_kind,
                     site_routes, which_site, whose
    blueprint.rs     BlueprintCommand; blueprint, watch_job_log, the scaffold and dotenv questions
                     (agreed_to_scaffold, agreed_to_dotenv, vouching, unasked), and the
                     questions an apply asks (answered, subject_of, unanswerable_question,
                     granted, asked_to_grant)
    extension.rs     ExtensionCommand; extension, origin, extension_id, agreed_to_install
    database.rs      DatabaseCommand; database, credential_store
    domain.rs        DomainCommand
    cert.rs          CertCommand; cert, agreed, job_answering
    path.rs          PathCommand
    autostart.rs     AutostartCommand; autostart_entry
    elevation.rs     ElevationCommand; elevation, confirmed (also asked by `mix doctor --repair`)
    service.rs       ServiceCommand, LimitsCommand, FrontEndServerArg, PriorityArg, Target,
                     StartTarget, Found; service, walk_target, the_front_end, set_front_end,
                     agreed_to_switch, agreed_to_reset
    metrics.rs       metrics, watch_metrics, since_moment, since_refusal
    logs.rs          logs, report_gap
    job.rs           JobCommand
    home.rs          HomeCommand; home, agreed_to_restore
    doctor.rs        doctor, bundle, self_repair
    disk.rs          disk, cleanup, agreed_to_cleanup
    uninstall.rs     UninstallAsk; uninstall and everything that waits for the removal to finish
    self_update.rs   SelfUpdateAsk, Choice; self_update, relaunch, the update lock, rollback report
```

Each `commands/<name>.rs` holds the command's clap enum, its argument types, its handler, the
functions only it calls, and the tests of those functions. Its entry point keeps the signature it
has today — `(command, &Endpoint, Option<&Autostart>, json) -> Result<ExitCode, Error>` — so
`run()` changes only by a path prefix.

Estimated sizes from today's line ranges: `main.rs` about 650 (the `Command` enum is 330 of them),
the largest command `runtime.rs` and `service.rs` about 750 each, most of the rest under 400. No
file over 1,000, so none enters the baseline, and `main.rs` leaves it.

`commands/autostart.rs` and `commands/home.rs` share a name with the crate-root `autostart.rs` (a
daemon started on demand) and `home.rs` (which home `mix` talks about). They are different things,
and `commands::` says which — the alternative, renaming a command's module away from the command's
name, makes the one lookup this split exists for slower.

## D6 — the `Command` enum and `run()` stay in `main.rs`

The top-level `Command` enum carries five variants with inline fields (`Metrics`, `Doctor`,
`Cleanup`, `Uninstall`, `SelfUpdate`). They stay as they are. Turning them into `Args` structs owned
by their module would read better, but it is a change to the clap derive, and the derive is what
produces `--help` and the command reference. Moving is this task; reshaping is not.

`run()` stays the one place that reads `Args`, decides autostart, and matches every variant —
including the `unreachable!` arm for `Docs` that keeps the match exhaustive.

## D7 — a helper goes where its callers are

Measured by call site, on `master` at `908e65e4` — the caller of every top-level item, mapped to
the item it sits in:

| Helper | Callers | Goes to |
| --- | --- | --- |
| `ask`, `encode` | every command, ~120 each | `client.rs` |
| `rendered`, `emit`, `report` | every command, and `main` | `output.rs` |
| `follow` | 12, in nine commands | `jobs.rs` |
| `unanswered` | 19, in ten commands | `confirm.rs` |
| `Agreement`, `prerequisites_agreed` | runtime, package | `prerequisites.rs` |
| `agreed_to_prerequisites` | blueprint, and `prerequisites_agreed` | `prerequisites.rs` |
| `upgrade_confirmed`, `finish_upgrade` | runtime, package | `upgrades.rs` |
| `here` | project, site, extension, blueprint | `commands/project.rs`, imported by the other three |
| `which`, `WhichProject` | project, blueprint | `commands/project.rs` |
| `confirmed` | elevation, doctor | `commands/elevation.rs`, imported by doctor |
| `install`, `said_what_it_lacks` | runtime only | `commands/runtime.rs` |
| `watch_job_log`, `answered`, `granted` | blueprint only | `commands/blueprint.rs` |
| `credential_store` | database only | `commands/database.rs` |
| `agreed`, `job_answering` | cert only | `commands/cert.rs` |

The rule for a helper found later: **one caller module, it lives there.** Several, and it is about
one command's subject — a project, an elevation — it lives in that command's module and the others
import it. Several, and it belongs to no command, it goes to the crate root under a name that says
what it does. `utils` and `helpers` stay banned.

Visibility is the narrowest that compiles: `pub(super)` for a command's entry point,
`pub(crate)` for a shared helper, private for everything else.

## D8 — a move, and only a move

What a commit in this task may change, in order of how easy it is to check:

1. Where an item is.
2. Its visibility.
3. `use` lines, and a path that names a moved item (`crate::Choice` in `confirm.rs` becomes
   `crate::commands::self_update::Choice`).
4. Intra-doc links that now cross a module (`[`report`]` → `[`report`](crate::output::report)`).
5. `main.rs`'s entry in the ratchet's baseline, lowered by `--shrink`.

Nothing else: no rename, no signature, no body, no comment reworded. A defect noticed while moving
is written down and fixed in its own commit afterwards, so that a reviewer can read every commit of
this task as "the same code, somewhere else" — `git diff --color-moved=dimmed-zebra` shows exactly
that, and anything it does not dim is a line to explain.

**One commit per module moved on the branch**, each one green on its own: a commit that moves one
command is small enough to review by eye, and a mistake is found by bisecting the branch to a single
module. **The pull request is squash-merged**, so `master` gets one commit for the whole split.
Because that commit only moves code, `git blame -C -C -C` still follows each line back across the
file it came from to the commit that last really changed it.

## MixLab

No part in it. This changes how `mix`'s source is laid out and nothing it does: no daemon method,
no type in `mixengine-proto` or `bindings/`, no flag, no output. MixLab draws from the API and
`bindings/`, never from `mix`, and `scripts/check-client-surface.mjs` gives the same answer before
and after.

## Testing

What proves nothing changed:

- **`bash packaging/docs.sh --check`** diffs the committed command reference
  (`docs/guide/en/cli.md`) against what `mix` generates from its clap definitions. Every flag,
  every help line and every subcommand is in it, so a clap attribute lost in a move is a diff
  there.
- **The unit tests keep their count.** `cargo test -p mixengine-cli --bins -- --list | grep -c ': test'`
  before the first commit and after the last gives the same number. Tests move with the function
  they test; `every_command_is_one_clap_can_build` stays in `main.rs`.
- **The integration suites** under `crates/mixengine-cli/tests/` drive the built `mix` and do not
  read its source. They run unchanged — CI's `test` and `system` legs.
- **`bash scripts/gate.sh`** before every push: clippy with `-D warnings` catches an import left
  unused, rustdoc with `-D warnings` catches a doc link that no longer resolves, and the ratchet
  catches a baseline entry left behind.
- `main.rs` has no `#[cfg]` on an operating system today, so what compiles here is what compiles
  on every leg; a `cfg` met while moving is checked on its OS before the push (the
  `before-pushing` skill).

No new tests: there is no new behaviour to test, and a test written for this would only be
testing the file layout.

---

## Not in scope

- **`render.rs`**, proposed as **T207b** after T207a. It is 87 public functions, one or more per
  answer type, and splits along the same lines (`render/site.rs` beside `commands/site.rs`). It is a
  task of its own because about 3,000 of its lines are tests organised by answer, not by command, so
  where each goes is a decision of its own.
- `mixengine-daemon/src/api/rpc.rs` (5,683 lines) and `services/mod.rs` (4,613): the same shape of
  problem in the daemon, where a move touches the code that owns state. Each needs its own design;
  until then the ratchet keeps them from growing.
- Any change to `Command`'s variants, as D6 says.
- A limit on Markdown. `docs/guide/en/cli.md` is generated and long by nature, and a document's
  length is a different question from a source file's.
- Lowering the limit below 1,500. Worth asking once the baseline is short; at 1,000 it would hold
  124 files today, too many for a list anyone reads.

## Is there an ADR here? No

The limit enforces a standard that exists (*modules by capability*, `docs/standards/rust.md`), and
the split brings one crate up to it. Nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/standards/rust.md`: the limit, the baseline, and D3's rule that growing a baselined file is a
  hand edit to be argued in review.
- `CLAUDE.md`, *Common commands*: `node scripts/check-file-sizes.mjs`, and `--shrink` after making a
  large file smaller.
- The `before-pushing` skill needs nothing: `gate.sh` runs the check.
