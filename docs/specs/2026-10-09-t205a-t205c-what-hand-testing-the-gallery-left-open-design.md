---
status: implemented
date: 2026-10-09
task:
  - T205a
  - T205b
  - T205c
---

# T205a–T205c — What hand-testing the gallery left open (design)

Applying every gallery blueprint by hand in MixLab (T205, 2026-10-08) ended with every site
answering, and with three things written down rather than fixed. This design covers all three.
Each one stands alone and lands as its own commit.

- **T205a:** Rails and Django start on a database of their own, not the PostgreSQL MixEngine made.
- **T205b:** nothing notices when a published starter drifts from the tree.
- **T205c:** Ctrl+C in a local PowerShell tab can be ignored, depending on how MixLab was started.

## MixLab

- **T205a:**
  - `ApplyDialog` gains one checkbox per `.env` key the plan would write, ticked by default for a
    signed blueprint.
  - The *Done* screen says, in its own words, when a key was left unwritten.
  - The steps panel (T205's D8) changes only its notes, which come from the manifests.
  - No new screen and no new daemon method; `blueprint.apply` gains one request field.
- **T205b:** no part. It is a workflow in `mixengine-packages` and a path in `gallery.yml`.
- **T205c:** the Terminal module's local sessions (`src-tauri/src/modules/terminal/local.rs` and
  `src-tauri/src/platform.rs`). Nothing in the `mixengine` module changes.

---

## T205a — With the person's agreement, the apply writes the database's address into `.env`

### What happens today

- `rails new . --database=postgresql` writes a `config/database.yml` whose `development` entry is
  `<name>_development` with no account. The database MixEngine made is called `{project}`, and its
  account is `{project}` with a generated password. Every page answers *ConnectionNotEstablished*
  until the person sets `DATABASE_URL` by hand.
- Django's template (T205) keeps SQLite, so the PostgreSQL database sits unused.
- The panel's credentials block shows host, port, name, user and a **Reveal password** control.
  The note then says to set `DATABASE_URL`, and the person types it into a shell.
- A variable typed into a shell lasts as long as that shell. Tomorrow, `rails server` started from
  any other terminal fails again.

### Options considered

| | Where the password ends up | Lasts past one shell | Verdict |
|---|---|---|---|
| A | MixLab passes it in the Run tab's environment | no | Breaks T205's D8 (*"credentials never enter a Terminal target, a tab state"*). Works only for the tab Run opened |
| B | The shim injects it into every `ruby`/`python` run inside the project | yes | Reads the keychain on every process start, and hides where the value comes from |
| C | The apply writes it into the project's `.env` by itself | yes | Works, but writes a credential somewhere new with nobody asking: a new decision about where credentials live, so a new ADR |
| **D** | **As C, only when the person ticks a box that says so** | **yes** | **Chosen.** It is what T77b exists for (*getting a database's password into a project's `.env`*), with the paste done for them. ADR 0025 already covers it; `security-model.md` gains a line |

### D1 — `[[services]] dotenv`

```toml
[[services]]
name = "postgres"
instance = "main"
database = "{project}"
user = "{project}"
dotenv = "DATABASE_URL"   # new: the .env key the apply offers to write
```

- **The value is the database's URL:**
  - `postgres://user:password@127.0.0.1:port/database` for PostgreSQL;
  - `mysql://…` for MySQL and MariaDB.
  - The user and password are percent-encoded. T77b's generator already avoids characters that
    break a `.env` line, and a password a person chose (`--password`) may not.
- **Read-time refusals:**
  - `dotenv` without `database` and `user`, since without an account there is no URL;
  - a key that is not ASCII letters, digits and `_`, or that starts with a digit;
  - `dotenv` on more than one service. An apply remembers one database, the one its answer
    reports (`BlueprintApplied.database`, T205's D6), and every gallery entry needs one.
- **Schema stays 2.** `dotenv` changes what an apply does, so by ADR 0061 it needs a schema that
  reads it. Schema 2 is unreleased (T205 is in `## [Unreleased]`), so no installed build reads
  schema 2 and ignores this key. If a release ships schema 2 before this lands, `dotenv` raises
  the schema to 3 instead.

### D2 — `PlanAction::WriteDotenv { key, path }`

- **A new plan action, ordered after the scaffold.** `composer create-project .` and the other
  `needs_empty_dir` scaffolds refuse a folder that already holds a `.env`.
- **Dispositions:**
  - `Confirm { what }` when `.env` is missing or has no line for `key`. `what` is `KEY in .env`,
    never the value.
  - `Satisfied` when `.env` already has a line for `key`, whatever its value. The person's own
    value wins, and a re-apply does not reset it.
- **The consent travels apart from the scaffold's.**
  - `BlueprintApply` gains `dotenv: Vec<String>`, the keys agreed to (`serde(default)`, skipped
    when empty).
  - A plan can carry a scaffold and a `.env` key at once. One `ScaffoldConsent` must not answer
    both, so `steps::untouched_with_consent` decides `WriteDotenv` from this list and
    `RunScaffold`/`FetchArchive` from `scaffold`.
  - A key in the list that the plan does not write is refused before anything is touched, the
    same rule `consent_refusal` applies to a command: a consent names what was read.
  - A key not agreed to ends `NotRun`, and everything else is applied.
- **Execution:**
  - **The account is the one the apply made**, not the plan's: T202 may have given the project
    `<user>-2`. The step reads `BlueprintApplied.database` as the `CreateDatabase` step left it,
    and fails, naming the key, when no database was made.
  - It reads the password through the same keychain read `database.credentials` uses.
  - It appends one line, `KEY=value`, to `.env`. It creates the file if needed, and adds a newline
    first if the file did not end with one.
  - It never rewrites or removes another line.
  - The file is written through `mixengine_platform::write_private`, the call that writes the
    CA's key. So `.env` ends up readable by this account only, on every system, and an existing
    one is restricted before the new line reaches it.
- **Nothing carries the value:** the plan, the job log, the step's result and MixLab's screens
  name only the key and the path. In `mix`, the action reads *write DATABASE_URL to .env*.
- **A failure is a failed step**, not a failed apply, like the scaffold.

### D3 — Asking: the checkbox, the flag, the question

**MixLab (`ApplyDialog`)**

- Under each `write_dotenv` step that is `confirm`, a checkbox naming the step's key:
  - en: *Write {{key}}, with the database password, to .env*
  - vi: *Ghi {{key}}, kèm mật khẩu database, vào .env*
- **Ticked by default when the plan is trusted** (`plan.trusted`), as the scaffold's box is. Left
  empty for a blueprint nobody vouches for: such a blueprint could name a key the project reads
  for something else.
- The request sends the ticked keys in `dotenv`.

**MixLab (*Done* screen)**

- A `write_dotenv` that ended `not_run` is drawn as a sentence of MixLab's own, not the daemon's
  `why`, whose hint names a `mix` flag. The sentence names the key: *{{key}} was not written to
  .env. Set it yourself, with the password shown below.*

**`mix blueprint apply`**

- `--write-dotenv` agrees to every key the plan would write.
- Without the flag, an interactive run asks once per key: *Write DATABASE_URL, with the database
  password, to .env? [y/N]*. `--run-untrusted-scaffold`'s split does not apply: the question
  already says what is written, and an untrusted blueprint is answered the same way.
- A `--json` run or a closed standard input writes nothing and says so in one line. This is the
  scaffold's rule for a question with a safe default.

**Contract and capture:**

- `packaging/bindings.sh` regenerates `bindings/` for `BlueprintApply::dotenv` and the new action.
- `mix blueprint capture` never writes `dotenv`. A project's `.env` says nothing about which key a
  blueprint should offer, and a captured manifest that wrote one would be a guess.

**Docs:** `security-model.md` gains the line: *the password reaches a project's `.env` only when
the person ticks the box or passes `--write-dotenv`, and nothing else MixEngine writes holds it.*

### D4 — The two blueprints

**`rails`**

- `dotenv = "DEVELOPMENT_DATABASE_URL"` on its `postgres` service.
  - **Not `DATABASE_URL`.** Rails merges `DATABASE_URL` into whichever environment is running.
    Loaded from `.env`, it would point `bin/rails test` at the development database, and the test
    run's `db:test:prepare` would purge it.
- The `rails new` step gains `-m <rails-template.rb URL>`. This is a Rails application template
  this repository writes, at `starters/rails-template.rb`. It does two things:
  1. In `config/database.yml`, `development`'s `database:` line becomes
     `url: <%= ENV.fetch("DEVELOPMENT_DATABASE_URL", "postgres:///<app>_development") %>`, with
     `<app>` the template's `app_name`. Only development reads the key; without it, the project
     behaves as `rails new` left it.
  2. It adds `config/dotenv.rb`, required at the top of `config/boot.rb`. That file reads the
     `.env` beside `config/` into `ENV`, line by line (`KEY=value`, `#` comments, blank lines),
     without overriding a variable already set.
- No gem is added, so `bundle install` and the devkit path are unchanged. Rails' own `.gitignore`
  already ignores `/.env*`.
- The server step's note says the address is in `.env` when the box was ticked, and otherwise to
  set `DEVELOPMENT_DATABASE_URL` from the account above.

**`django`**

- `dotenv = "DATABASE_URL"` on its `postgres` service. Django's test runner makes its own `test_`
  database, so the standard name is safe here.
- The `startproject` step gains `credentials = true`, so the panel draws the account and
  **Reveal password** for someone who left the box unticked. Rails' `rails new` step already
  carries it.
- **`starters/django/project_name/settings.py-tpl`:**
  - It reads `BASE_DIR / '.env'` for `DATABASE_URL`, in a few lines of standard library.
  - When the key is there, it parses it with `urllib.parse` into
    `ENGINE: django.db.backends.postgresql` (or `mysql`).
  - Otherwise it keeps SQLite, so a project started from the template outside MixEngine, or with the
    box unticked, still runs.
- **The template gains `.gitignore`** with `.env`.
  - Django copies a dot-file in a template as it is; it skips only dot-directories.
  - Django refuses to overwrite a file that is already there. A folder with a `.gitignore` of its
    own already holds a project, which `startproject` was never going to fit.
- **The first step becomes `python -m pip install django psycopg psycopg-binary`.**
  - Not `psycopg[binary]`: zsh reads `[…]` as a glob, and T205's D4 requires one meaning in
    `cmd.exe`, PowerShell and `sh`.
  - **Measured on 2026-10-09:** `psycopg-binary` 3.3.6 has CPython 3.13 wheels for Windows x64,
    macOS (Intel and Apple Silicon) and Linux (x64 and ARM64, glibc and musl), and **none for
    Windows ARM64**. There, `pip` fails the step with *no matching distribution*, where today's
    `pip install django` succeeds.
  - So the step's note says: *On Windows ARM64 there is no psycopg-binary yet; install Django
    alone with python -m pip install django, and leave the .env box unticked to stay on SQLite.*
  - A Windows ARM64 PostgreSQL driver is a roadmap follow-up (**T205d**), not a silent fall-back
    to SQLite. The pure `psycopg` finds `libpq.dll` on `PATH`, and MixEngine's PostgreSQL package
    carries one, so that follow-up starts from measuring whether that is enough.
  - **Measured at T205d (2026-10-09): it is not.** That `libpq.dll` is the x86_64 build ADR 0023
    installs beside a native ARM64 Python, which cannot load it, and the step's tab does not put
    it on `PATH`. The note stays; the details are in phase 43.

**Hosting.** `publish-blueprints` gains one rule: *a file directly under `starters/` is uploaded
as itself*, beside each folder's `<name>-starter.zip`. `every_starter_archive_is_in_the_tree`
accepts a URL to such a file.

### Testing

- **Manifest:** `dotenv` reads, renders and round-trips. It is refused without `database`/`user`,
  with a key that is not a variable name, and on a second service.
- **Plan:**
  - `WriteDotenv` comes after `FetchArchive`/`RunScaffold`.
  - It is `Confirm` with no `.env` and with a `.env` lacking the key, and `Satisfied` with the key
    present.
- **Apply:**
  - A key not agreed to ends `NotRun`.
  - A scaffold consent does not write the key, and a key consent does not run the scaffold.
  - A key agreed to that the plan does not write is refused before any step runs.
- **Execution:**
  - It appends to an existing `.env` with and without a trailing newline, and leaves every other
    line byte for byte.
  - A password with `@`, `:` and `/` is percent-encoded.
  - The result and the log carry no password (the T77a tests' shape).
  - The file is private afterwards: `0600` on Unix, `is_private_file` on Windows.
- **CLI:** `--write-dotenv` agrees to each key, and `--json` without it writes none.
- **MixLab:** the box is ticked for a trusted plan and empty for an untrusted one, and the
  request's `dotenv` holds exactly the ticked keys.
- **Gallery:** `rails` and `django` carry `dotenv`, and each has a step with `credentials`. The Rails template URL resolves to a file in
  the tree.
- **By hand:**
  - `rails` on Windows: the box is ticked; *Run the required steps* ends at Rails' welcome page
    with no *ConnectionNotEstablished*; `bin/rails db:migrate` works from a fresh terminal;
    `bin/rails test` leaves the development database alone.
  - `django`: `python manage.py migrate` creates its tables in `{project}` in PostgreSQL.
  - Either blueprint with the box unticked: no `.env`, and the *Done* screen says so.

---

## T205b — `check-blueprints` compares the starters too

- `tools/blueprints.py` (in `mixengine-packages`) gains a pass over
  `mixengine/crates/mixengine-core/src/blueprints/starters/`:
  - for each folder, download `<name>-starter.zip` from the release and compare its **entries**;
  - for each file directly under `starters/` (T205a's `rails-template.rb`), download the asset and
    compare its bytes.
- **Comparing a zip by entry:** the set of names, then each file's bytes. It compares neither the
  zip's own bytes nor timestamps. `zip -X` still records modification times, and a checkout sets
  them to the moment of checkout, so two zips of one tree never match byte for byte.
- A starter on the release that the tree no longer has is a drift too, the same rule as a removed
  gallery file.
- **The report** names the starter and the first differing entry. The workflow's summary already
  says to run `publish-blueprints` again.
- `mixlab`'s `.github/workflows/gallery.yml` dispatches `gallery-changed` on pushes that touch
  `gallery/**` or `trust.rs`. It gains `crates/mixengine-core/src/blueprints/starters/**`, so a
  starter edit is checked on the push that made it.
- **Testing:**
  - `tools/blueprints.py` against a fixture release of three starters: one matching, one with a
    changed file, one with an extra entry.
  - A starter on the fixture release that the fixture tree lacks.
  - A real run is green against the current release.

---

## T205c — Ctrl+C reaches a local shell however MixLab started

### What was measured

- Started from a background process with no console (2026-10-08), a local PowerShell tab ignored
  Ctrl+C while a command ran: `^C` printed nothing and the command went on. Git Bash in the same
  window stopped.
- Started by the person from a terminal, both stopped.

### Why

- On Windows a process can be told to ignore Ctrl+C. `CREATE_NEW_PROCESS_GROUP` sets this as if
  by `SetConsoleCtrlHandler(NULL, TRUE)`, and **the flag is inherited by every child**. A launcher
  that starts MixLab that way passes it through MixLab to each ConPTY child.
- PowerShell and native programs honour it.
- Git Bash's MSYS runtime turns the byte into its own signal, which is why bash still stopped.

### D5 — Clear the inherited flag before a local shell starts

- `platform.rs` gains `process_ctrl_c()`.
  - On Windows it calls `SetConsoleCtrlHandler(NULL, FALSE)` through a `#[link(name = "kernel32")]`
    declaration, as `show_maximized` already declares `ShowWindow`, since `windows-sys` is not a
    dependency for one call.
  - Elsewhere it does nothing, so callers need no `cfg`.
- `local::spawn` calls it before `spawn_command`, so the child inherits *processing* Ctrl+C. The
  call is cheap and idempotent, and calling it per spawn also covers a flag something set later.
- **Why it is safe for MixLab itself:** MixLab is a GUI process with no console, so no console
  control event can reach it. The flag only matters as an inheritance.
- **Measured before it is relied on:**
  - that the call succeeds in a process with no console attached;
  - that a ConPTY child started after it stops on Ctrl+C.
  If the call fails with no console, the fallback is to clear the flag in the child's own startup.
  For PowerShell, that is a `-Command` prelude that calls the same function through `Add-Type`.
  That fallback is a design change and comes back here before it is built.

### Testing

- **A Windows-only integration test** (`tests/ctrl_c.rs`), in its own test binary because it
  changes a process-wide flag:
  1. set the ignore flag with `SetConsoleCtrlHandler(NULL, TRUE)`;
  2. open a session through `local::spawn` running
     `powershell -NoProfile -Command "ping -n 30 127.0.0.1; 'after'"`;
  3. write `\x03` once the first reply is read;
  4. expect `after` within 5 s, well before `ping`'s 30.
  It fails without D5 and passes with it.
- **By hand:** start MixLab from the Start menu, from a shortcut, and from a background launch
  (`Start-Process -WindowStyle Hidden`). In each, `ping -t 127.0.0.1` in a PowerShell tab stops
  on Ctrl+C.

---

## Roadmap

- **T205d** is added after T205c: a PostgreSQL driver for Django on Windows ARM64.
- Each task is ticked in `phase-43-a-blueprint-ends-at-a-working-site.md` by the commit that lands
  it.
- The commit that lands the last of the three flips this design's `status` to `implemented`.
- T205a's roadmap line is rewritten to name D3's checkbox, since it still lists "an `.env`, or the
  server step's environment" as open.
