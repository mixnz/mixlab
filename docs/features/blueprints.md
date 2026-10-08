# Project blueprints

**Goal**: capture "PHP 8.2 + MariaDB 11.4 + Redis, Laravel layout, HTTPS on" once, then create the
next project with that stack in one action — and hand the same file to a teammate.

## Manifest

A blueprint is a single TOML file, human-readable and diffable, stored in `blueprints/<name>.toml`
and exportable anywhere:

```toml
schema = 1

[blueprint]
name = "laravel-php82"
description = "Laravel + MariaDB + Redis"
created_at = "2026-08-10T09:00:00Z"
created_on = { os = "windows", version = "0.1.0" }

[runtimes]
php = "8.2.23"          # exact when captured, range allowed when hand-written
node = "22.8.0"

[site]
kind = "php-fpm"
doc_root = "public"
https = true
domain_pattern = "{project}.test"

[[services]]
name = "mariadb"
version = "11.4.3"
instance = "main"        # "main" = reuse the shared instance; "per-project" = dedicated
database = "{project}"
user = "{project}"

[[services]]
name = "redis"
instance = "main"

[php]
extensions = ["redis", "imagick", "xdebug"]

[scaffold]
# optional, only runs with explicit user consent at apply time
command = "composer create-project laravel/laravel ."
needs_empty_dir = true   # optional, default false — this command refuses a directory with anything in it
needs_npm_safe_dir = true # optional, default false — this command takes its package name from the directory
```

`{project}` is the only templating token; substitution is literal and validated (slug charset).

## Capture

**The account a project ends up with is the plan's, or the next free one** — roadmap task **T202**.
`database.create` refuses an account that is on the server and that MixEngine holds no credential
for (T77a: a keyring entry is the deed of ownership, and a blueprint has nobody at the keyboard to
pick another name), so the apply tries `<user>-2` … `<user>-9` against the same database and takes
the first that is free or ours; a long name is cut so the suffix fits thirty-two characters. The
step's outcome carries a `note` saying which account the project got, and whether the database was
already there and left as it was — `mix blueprint apply` prints it under the step and MixLab shows
it there. Nine foreign names end in T77a's refusal, naming the last one tried. No account's
password is ever reset: the apply routes around a foreign account, never through it.

`blueprint.capture { project, name }` reads the project's resolved state — runtime versions, linked
services and their versions, PHP extensions, site kind and HTTPS — and writes the manifest. It
captures *what is actually in use*, not the global defaults, and it never captures data,
credentials, or absolute paths.

**There is no `[php] ini`, and this line used to promise one.** T77 went looking for the source and
there is none: every ini value MixEngine writes is a constant in `core::runtimes::extensions`,
identical on every machine, and the only per-pool override map a php-fpm service has holds
`max_children` and its siblings — process supervision, not ini. So "non-default ini values" named
nothing that deviates, and capturing them would have captured a global default, which is the one
thing this feature is defined against. The key arrives with the task that gives a project an ini of
its own. `extensions` stays, because a runtime's extension *choices* already are deviations.

Of those choices, capture takes only the ones turned **on**. A blueprint says what a project needs
loaded; turning something off on the receiving machine would change the PHP every other project
there runs, which is harm it was never asked to do.

A project with more than one site is **refused** rather than reduced to its first: a blueprint has one
`[site]`, and losing the others silently is worse than saying so. The project manifest gained
`[[sites]]` in T204 ([design](../specs/2026-10-08-t204-a-manifest-holds-every-site-design.md)), and
blueprints follow in T204a.

## Apply

`blueprint.apply { blueprint, project_name, root_path }` runs as a job with a **plan-then-execute**
shape:

**Who names the directory.** A folder somebody chose is the folder the source lands in, byte for
byte — typed into `mix --path`, typed into the Blueprints dialog, or browsed to in Quick Start, all
three are used exactly as spelled. Only one gesture names no directory at all: `mix blueprint apply`
with no `--path`. There the client sends the directory it is standing in with `root_is_parent`, and
the **daemon** composes the project's directory under it with `domains::slug` — the same handle
`{project}` expands through
([ADR 0030](../decisions/0030-the-project-token-expands-to-a-slug.md)), so `Next.js 1` gets the
domain `next-js-1.test`, the database `next-js-1` and the directory `next-js-1`. **The composing is
the daemon's and not a client's** because no client may hold that rule — `mix` cannot depend on
`mixengine-core`, and the desktop deliberately keeps no naming rule of its own — so a client
composing it would be a second copy of a charset to keep in step by hand. Nothing is renamed behind
anybody's back: the composed path is in the plan's `register_project` step, which every client shows
before anything is created. Roadmap tasks **T120a** and **T120c**;
designs: [2026-09-13-t120a-a-scaffolds-directory-is-a-name-too-design.md](../specs/2026-09-13-t120a-a-scaffolds-directory-is-a-name-too-design.md),
[2026-09-13-t120c-a-folder-somebody-chose-is-the-folder-design.md](../specs/2026-09-13-t120c-a-folder-somebody-chose-is-the-folder-design.md).

**Quick Start used to compose too, and no longer does** (T120c, withdrawing T120a's D3 and the whole
of T120b). Two windows meaning different things by *choose a folder* is a longer-lived confusion
than the friction of needing a folder whose name works, and the friction is answered by the check
below rather than by a default.

1. **Plan**: resolve every requirement against what is installed, and return the full list of actions
   (install PHP 8.2.23, create DB `blog`, add domain `blog.test`, issue cert…). The plan is returned
   before anything happens; `mix blueprint apply --dry-run` prints it.
2. **Execute**: run the actions with progress. Each action is idempotent, so a failed apply can be
   resumed rather than restarted.
3. **Rollback on failure removes what belongs to the project and keeps what belongs to the
   machine** — T78's design, D4. Undone: the site, a service instance dedicated to this project, and
   the project row. Kept, and each one *named* in the failure: **the database**, because by the time
   an apply has failed a scaffold may have migrated into it and destroying data to tidy up is the
   more expensive direction to be wrong in (`database.create` is idempotent, so running the apply
   again finds it and moves on, and there is no `database.drop` in this product — see
   [services.md](services.md)); **a runtime or package this apply installed**, which is what a
   resumed apply would otherwise download all over again; **a PHP extension it turned on**, which
   reaches every project on the machine; and **the project's directory**, on `project.delete`'s
   standing rule that the files were never ours.

   A shared instance that already existed is never removed either way. And `job.cancel` is not a
   failure: it stops where it is and leaves what was made, because running the apply again continues
   from there.

**Resuming is running it again.** Every action is an *ensure*, so a second apply plans against what
the first one left: everything already done comes back `Satisfied` and what remains is exactly what
remains. There is no ledger of half-finished applies to reconcile — the rows are the record.

Version mismatches are surfaced as choices, not silent decisions: *"PHP 8.2.23 is not installed.
Install it / use installed 8.2.29 / cancel."* **The question is asked by a client and answered in the
request**: a daemon has no keyboard, so `blueprint.apply` carries an answer per subject and refuses,
before anything happens, when one is missing. The answer decides the project's *pin* as well as the
download — without that, "install it" and "use the installed one" would leave identical machines
behind and the question would be theatre.

**An apply never raises an elevation prompt.** It queues what needs one, exactly as `site.create`
does, and the client spends the single prompt afterwards — `mix blueprint apply --grant`, or the
question it asks at the end.

**A manifest with a `[site]` can ask for a front end too** — T115, and it is asked for rather than
always. `core::sites` renders nothing for a home with no front end and succeeds while doing it, so
an apply that stopped at the site row left a project nothing serves — and nothing in MixEngine
installs a web server by itself ([services.md](services.md): *"a first run that offers to do it for
them is not built"*). `BlueprintApply.front_end`, `mix blueprint apply --with-front-end`: where this
home has no front end at all, the plan gains the two actions a `[[services]]` entry already produces
— install the package, ensure the instance — for this build's default, Caddy.

**Defaulted off, because an apply is about a project.** Provisioning the machine it runs on is a
wider thing, and doing it unasked would make every apply on a home with no web server download one,
including the ones deliberately about something else. The flag travels on the dry run as well, so
`--dry-run` keeps matching the real run action for action.

**A blueprint may not name which front end.** Exactly one runs at a time per home (T37), the choice
is the home's and lives in `service.set_front_end`, and a manifest that named one would apply
differently on a machine that had already chosen. A home that has one — nginx as much as Caddy — is
left alone whatever the flag says. The instance name comes from the recipe's own `Instancing`, which
is why it is `caddy` and not `caddy@main`.

**An apply that writes no source code still ends at a page.** Eight of the thirteen gallery
blueprints carry no `[scaffold]` on purpose, so the ordinary outcome of applying one is a configured site over
an empty directory — which a web server answers with a 404, or with a 502 for a kind that forwards to
a program the user runs. Since **T124** the front end answers with MixEngine's own page instead,
naming the site and the one thing that is missing, so a blueprint with no scaffold is a complete
blueprint rather than half of one. Nothing is written into the project: the page is served, and it
stops appearing the moment the site answers for itself.

## Scaffold commands

`[scaffold]` runs an arbitrary command in the new project directory, which is a real execution of
untrusted content when the blueprint came from someone else. **T78a** is what built the answer:

- **Never runs without agreement naming the exact command.** The consent travels in the
  `blueprint.apply` request — a daemon has no keyboard — and it carries *the command the person
  read*, so a blueprint re-imported between the plan and the apply cannot be run under an old yes.
  An apply carrying no consent applies everything else and reports the step as `NotRun`, because a
  blueprint must not become worthless over the one step nobody answered for.
- **Never runs on import**, only on apply, and never with an elevation: the command runs under the
  user's own account and nothing it does reaches the elevation queue.
- Runs in the project directory with `<home>/bin` in front of `PATH`, which is how the blueprint's
  own `[runtimes]` reaches it — the shims resolve a version from the project they are run in — and
  with `MIXENGINE_HOME` naming the daemon's own root, so those shims read *this* home's database
  and not the OS default's (T27c found the difference on a daemon started with `--home`).
  Output goes to the job's log (`GET /logs/job/{id}`, `mix job logs <job> -f`), which is the log
  surface a service's output already uses and not the event stream: how much a scaffold prints is
  decided by somebody else's program.
- **What the command printed is data, never control** — **T120a**. Every ANSI escape sequence is
  removed as the line is captured, so the job log, the stream and the sentence a failure writes are
  covered by one rule rather than by each renderer remembering. `create-next-app` colours a pipe, so
  its refusal reached a person as `[31m"Next.js 1"[39m` and would have reached a terminal as
  instructions to move the cursor. The command is also told `NO_COLOR=1` — a complement and never
  the guarantee, since it is a convention some programs read and others do not; measured,
  `FORCE_COLOR=0` does nothing to this one. A failure quotes the command's last lines **as lines**:
  they were joined with `" / "` until the slashes started reading like paths.
- **Its program is checked at plan time, by the shell's own rule** — T78b. The command's first
  word, when it is a bare name (no quotes, no shell syntax, no path separator, not a builtin of
  `cmd.exe` or `sh`), is looked for on exactly the `PATH` the command would run with — `<home>/bin`,
  then the daemon's own — and a name nothing answers to makes the step `blocked`, naming the
  program and both halves of that PATH. **A blocked scaffold blocks the scaffold, not the apply**:
  with a consent for it the apply is refused up front, in the plan's words; without one everything
  else is applied and the step is reported not run for that reason. `cmd.exe` never runs a bare
  file with no extension and `bin/` is swept of strangers at every start, so the hint says *put it
  on your PATH and restart the daemon* and never *copy it into `bin/`*. Design:
  [docs/specs/2026-09-08-t78b-a-scaffold-program-checked-at-plan-time-design.md](../specs/2026-09-08-t78b-a-scaffold-program-checked-at-plan-time-design.md).
- **A command that initialises a project says so, and the directory is checked at plan time.**
  `[scaffold] needs_empty_dir = true` means this command refuses a directory that already holds
  anything — `composer create-project .` stops at the first entry there is, a `.git` included — and
  the plan then `blocked`s the step for a root that is not empty, naming what is in the way.
  Declared rather than read out of the command, because `composer install` on a tree somebody
  cloned is a scaffold whose directory is *supposed* to hold something and no rule over the two
  strings tells them apart; the default is `false`, so a manifest written before the key existed
  runs exactly where it used to. A root that does not exist yet is empty — that is the ordinary
  case, since the apply is what creates it — and a root whose listing fails is not judged, on the
  same rule as the program check: a false `blocked` stops a blueprint that would have worked.
  Every gallery blueprint with a scaffold sets it, asserted over the shipped set in
  `crates/mixengine-core/tests/blueprint_gallery.rs`.
- **A command that names itself after its directory says so too, and the name is checked before the
  contents** — roadmap task **T120c**. `[scaffold] needs_npm_safe_dir = true` means this command
  takes its package name from the directory's basename, so the plan `blocked`s the step for a
  basename npm will not accept — empty or over 214 characters, an ASCII capital, a character outside
  `a-z 0-9 - _ .`, or a leading `.` or `_` — and the reason ends with the answer rather than the
  rule: *rename the folder to `next-js-1` and apply again*. **The ruler is npm's and not
  `domains::slug`'s**, and the row that decides it is `next_js_1`: `slug` turns every character
  outside `a-z0-9` into a hyphen, underscore included, so a check built on it would refuse a
  directory `create-next-app` accepts, installs into and writes `"name": "next_js_1"` for (measured
  2026-09-13). `slug` is the *generator* here, never the ruler — it always answers in a charset the
  rule accepts, which is why it can be what the refusal suggests. Where the rule is uncertain it
  permits, because refusing a directory that would have worked leaves somebody with no way round
  while missing one costs a wasted install; the leftover case — a blueprint somebody imported, whose
  author will never declare the flag — is covered late, by the failure text itself naming the folder,
  and **only for a command of the npm family** (`npx`, `npm`, `yarn`, `pnpm`), because
  `composer create-project` names itself from its argument and telling its failure that the folder
  is the problem would send somebody renaming one for nothing. That guess is allowed there and
  nowhere near the block, which is what D3's *declared, never inferred* is about: a guess may add a
  hint after a failure, never refuse somebody up front. Only `nextjs` declares it, asserted over the
  shipped set.
- **Or a release archive, downloaded and unpacked** — roadmap task **T205**
  ([design](../specs/2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md), D2). `[scaffold] archive = "https://…"`, with an optional `strip` naming the
  archive's single top-level folder, is the form for a project that ships as a zip and has no
  initialiser to run: WordPress is the one in the gallery. It is the other half of the same key, so
  `archive` and `command` exclude each other and a manifest with both or neither is unreadable;
  only `https://` is read, and no checksum is pinned, because `latest.zip` changes with every
  release. It is consented to exactly as a command is, since the PHP files it writes run the
  moment somebody opens the site: the plan shows the URL where it would show a command, and the
  `ScaffoldConsent` names it in `archive`. It unpacks into a staging directory under the home's
  `cache/` and moves the contents into the project root, refuses a download past 1 GiB, and keeps
  every other rule here: `needs_empty_dir`, cancellable, no timeout, a failed step rather than a
  failed apply. It needs schema 2, which ADR 0061 writes only for a blueprint that uses it.
- **No timeout.** Any number would kill a legitimate `composer install` on a slow line; the bound is
  that the job is visible and `job.cancel` stops it — killing the process *group*, so what a package
  manager forked goes with it.
- **A command that exits non-zero is a failed step, not a failed apply**, and rolls nothing back: the
  project it made works, and destroying it because a post-install script failed is the more
  expensive direction to be wrong in. `mix` exits non-zero on it.
- **Trust is decided when a blueprint arrives and is never raised.** `blueprint.import` verifies a
  detached minisign signature against the compiled-in gallery key; what verifies is trusted, and
  what does not — including a file with no signature at all — is untrusted for good. A signature
  that does not verify is not a refusal: the blueprint is still imported, and what it loses is the
  right to a quiet yes. `mix blueprint apply --run-scaffold` agrees for a signed blueprint and
  `--run-untrusted-scaffold` for one nobody vouches for; neither covers the other, so a script that
  runs somebody's unsigned command says so on the line that does it.
- **And it says which kind of untrusted** — T79b. A file that arrived with nothing to vouch for it
  and a file whose signature did not verify are both untrusted and are not the same event: the
  second is what the gallery key exists to catch. The row records which (`blueprints.signature`:
  `verified`, `missing`, `rejected`, or NULL where no check happened), and every client says it —
  at import, in the `TRUST` column of a listing (`signed`, `unsigned`, `mismatched`), and in the
  question asked before a `[scaffold]` command runs. Three sentences, one gate:
  `--run-untrusted-scaffold` still answers for both kinds, because a failed signature was never a
  refusal and this changes what is *said*, not what is allowed.
- **The reason is a record of what arrived, never a claim about the file on disk.** A row that says
  `signed` is saying a signature verified when the blueprint came in; the `.toml` rendered beside
  it is not the artifact that was signed, and nothing re-checks it. A check made later would be a
  check against bytes the signer never saw, and a check that can fail with no tampering behind it
  is a check somebody eventually turns off.
- **A word no build recognises costs a row its explanation and nothing else.** Where an unknown
  `source` is refused — it decides what a plan does — an unknown reason reads as "none recorded",
  in the column and on the wire both, so that a `mix` older than some later variant does not fail
  to parse a whole listing over the one field on it that is decoration.

## Built-in gallery

The gallery ships **inside the binary** and is seeded into every home as `builtin` rows the first
time a daemon starts there: `cakephp`, `codeigniter`, `craft`, `django`, `drupal`,
`express-mongodb`, `laravel`, `laravel-mongodb`, `nextjs`, `php-mysql`, `rails`, `statamic`,
`static`, `strapi`, `symfony`, `vite`, `wordpress`, `yii`. They are trusted without a signature check,
because a signature travelling in the same binary as the key it would be checked against proves
nothing the binary has not already proved — publishing them as signed files for hand import is T79a.

**The set is a coverage surface, not a list of favourites.** A blueprint earns its place by closing
a gap between what this build can run and what the gallery ever asks for, and by carrying at least
one setting a person would not have guessed. The second five — T125 — were chosen that way: `rails`
is the only Ruby in it, though the runtime has shipped with six shims since phase 2; `php-mysql` is
the only MySQL, beside three MariaDBs; `drupal` is the only `doc_root` that is neither the project
root nor `public`; `vite` is the only document root that is a *build output*, which is the question
this product is asked most often and had no answer for; and `strapi` is the only `node-app` with a
database, a pairing `nextjs` leaves untouched. The two MongoDB entries — T164 — close the gap phase
19 opened: every PHP MixEngine installs carries the `mongodb` extension and MixLab browses a
MongoDB, and nothing in the gallery asked for one. `laravel-mongodb` is the only entry that turns
that extension on, and `express-mongodb` the only `node-app` with a document store. The five PHP
frameworks of T205 each close one more: `cakephp` is the only `webroot`, `codeigniter` the only
framework on MySQL, `craft` the only PHP project on PostgreSQL, `statamic` the only PHP project with
no service at all (it keeps its content in flat files), and `yii` the only `web` that is an
application rather than a CMS. Both name the
service and **no `database`**: the recipe runs without accounts and makes no databases — one exists
once something writes to it — so the key would plan a step the apply is refused at. And both say in
their description that MongoDB needs a processor with AVX, which is the first requirement in the
gallery a machine can fail by being what it is; it is judged when the package installs, not in the
plan. `memcached` is still reached by nothing, and
deliberately: it is added after a performance problem, never at the moment a project is created, and
an entry that existed to complete a table is the change this section exists to argue against.

Seeding **compares before it writes**, so the ordinary daemon start touches nothing, and a row whose
source is `captured` or `imported` is never overwritten: capturing over `laravel` makes that slug
this machine's own for good. There is no `blueprint.delete` in this build, so every gallery entry
is in every home for good as well.

Most carry a `[scaffold]`: a command for `cakephp`, `codeigniter`, `craft`, `drupal`, `laravel`,
`laravel-mongodb`, `nextjs`, `statamic`, `symfony` and `yii`, and an archive for `wordpress`. The
rest deliberately do not. A gallery command has to be non-interactive (there is no timeout, so a prompt
would hang a job), spelled the same for `cmd.exe` and `sh`, with a program for its first word — the
plan reads it as one (T78b) — and it may not write into a shared runtime: that last rule is what
removes Django's `pip install django` and Rails' `gem install rails`, both of which reach every
project using that runtime. The first rule is what removes `vite` and `strapi`: `create-vite` and
`create-strapi-app` ask questions that no flag reliably silences, and a job with no timeout that is
waiting on a prompt waits for good. `php-mysql` has no initialiser to run at all, which is the whole
of what it offers. `express-mongodb` would have only `express-generator`, unmaintained and a major
version of Express behind, so it unpacks a **starter this repository writes** instead: an Express 5
server that reads `PORT` and `MONGODB_URI` and says on its page whether MongoDB answers. Its source
is `crates/mixengine-core/src/blueprints/starters/express-mongodb/`, and the packaging repository's
`publish-blueprints` zips it beside the signed gallery as `express-mongodb-starter.zip`, from the
same commit (`every_starter_archive_is_in_the_tree`). `django`'s step names a starter too: a
`startproject --template` that is Django's own project template plus `.test` in `ALLOWED_HOSTS` and
`https://*.test` in `CSRF_TRUSTED_ORIGINS`, since the stock template answers every page served
through MixEngine with *DisallowedHost*. `laravel-mongodb` runs `laravel`'s command and no more: the `composer require
mongodb/laravel-mongodb` after it would be a second command joined to the first, so it is a step
the person runs (below). The gallery sells a stack, not a scaffold.

**A blueprint with no framework still starts with a page.** `php-mysql` and `static` unpack a
starter page of this repository's (`starters/php-mysql/`, `starters/static/`) with
`[scaffold] when_empty = true`, so the two things people actually do with these entries are served
equally: apply onto an empty directory and get a page that names the blueprint and says what to
edit, or clone a repository and apply the stack over it, where the starter is skipped (`satisfied`)
and nothing of the clone is overwritten. `needs_empty_dir` would refuse that second flow. The three
starter pages (`express-mongodb` too) share one look, light and dark, with nothing loaded from
elsewhere.

They double as end-to-end tests of the whole system, but **not of the cross-OS criterion below** — a
hand-written manifest is byte-identical on all three systems, so what proves that one is a real
capture taken on Windows and committed as a fixture.

## What is left to do

**A blueprint says what to run after it, and MixEngine never runs it** — roadmap task **T205**
([design](../specs/2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md)). `[[next_steps]]` lists the commands that end at a working site, in three kinds:
`once` (run one time: `npm install`, `php artisan migrate`), `serve` (keeps running:
`npm run dev`) and `open` (finish in the browser: `/wp-admin/install.php`). Each takes a `note`,
`optional` when the site answers without it, and `credentials` when it needs the database account
the apply made. A `run` line follows the scaffold's rules, because it must mean the same thing in
`cmd.exe`, PowerShell and `sh`: one line with a program for its first word, no shell operators, no
`\`, and `{project}` as the only token. A step that breaks one makes the manifest unreadable,
named by its position.

**Guidance, not supervision.** The daemon starts no site program: a dev server belongs in a terminal
where a person sees it and stops it, and a supervised one would be a second process manager beside
the services. So the steps are data. The apply answers them as `BlueprintApplied.next_steps`, with
the blueprint's trust; `project.show` reads them again through the blueprint the project was made
from (`projects.blueprint_id`, no migration); `mix blueprint apply` prints them; and MixLab draws a
panel whose **Run** opens a Terminal tab with the project's toolchain on `PATH`. A step from a
blueprint nobody vouches for is typed there and left for the person's Enter, never pressed.

**The browser opens by itself when nothing is left.** MixLab sends `front_end: true` on every apply,
because its question is always *give me a site that works*, and opens the site when no step failed
and no step is needed; with an `open` step, on that step's page. Every gallery entry says what is
left: `nextjs`, `strapi`, `express-mongodb`, `django`, `rails` and `vite` need a step before the site
answers, and the PHP entries answer at once, on their installer where they have one.

## The gallery as signed files

The same thirteen are published from the packaging repository as `<slug>.toml` with a
`<slug>.toml.minisig` beside each —
`github.com/mixnz/mixengine-packages/releases/download/blueprints/` — signed by the gallery key whose
public half is `blueprints::trust::PUBLIC_KEY`. **T79a**, and the channel T79's compiled-in gallery
removed the need for and the reason for in the same stroke. The manifests are never copied into that
repository: its workflow checks out this one at a ref and reads them there, and it **proves
`blueprints.pub` against the compiled-in `PUBLIC_KEY` before it signs anything** — a signature no
installed MixEngine would accept is worse than no signature, because it looks published.

It is not how anybody gets `laravel` onto a machine: every home already holds all thirteen. It is how a
blueprint an installed build does *not* carry reaches one, how the thirteen can be corrected between
application releases, and how a file somebody downloads lands **trusted** instead of untrusted for
good. Replacing one of the thirteen needs `mix blueprint import <file> --overwrite`, and it costs that
slug its builtin refresh — seeding leaves a row whose source is not `builtin` alone, so the imported
copy is that machine's `laravel` from then on, even when the bytes were identical.

**A gallery change is not finished in this repository.** The release above is cut by hand, so adding
or editing a manifest leaves it holding the previous set — nothing here can notice, because the
release lives on the other side. Re-run `publish-blueprints` in `mixengine-packages` with `ref` set
to the **full** commit SHA of this repository (`actions/checkout` refuses an abbreviated one) and
`publish` on; the tag is moved rather than added to, and the run removes what the gallery no longer
holds.

**What is automatic is the reminder, not the release.** Pushing a gallery change to `master` fires
[.github/workflows/gallery.yml](../../.github/workflows/gallery.yml), which sends one
`repository_dispatch` to `mixengine-packages`; its `check-blueprints` compares the published set
against `mixengine@master` and goes red within a minute of the push that caused it, naming the
commit that asked. It also watches `blueprints/trust.rs`, because rotating that key invalidates
every signature already published without touching a manifest. Nothing there publishes on its own —
cutting the release names a ref and prunes what the gallery dropped, and `master` being ahead of a
published set is a state somebody is allowed to choose. Two things a person owns once:
`PACKAGES_DISPATCH_TOKEN` in this repository's secrets, a token with write access to
`mixnz/mixengine-packages` — a repository's own `GITHUB_TOKEN` cannot reach another repository at
all — and the weekly cron on the other side, which stays because a dispatch that was never sent
looks exactly like a gallery nobody touched.

**A count written in prose is a count that goes stale.** Three places in this file said "six" and one
in the packaging repository's `tools/blueprints.py` did too, and all four were wrong the moment the
gallery grew. Where a number is not the point of the sentence, leave it out; where it is, `ENTRIES`
is what a test reads and prose is not.

**A file is filed under its own name.** `blueprint.import` with no `--name` takes the file's stem,
not `[blueprint] name`: the manifest's name is display text — the gallery says `Static site` and
`Next.js` — and every rendering this product writes is `<slug>.toml`, so the stem is what carries a
blueprint's name from one machine to another. Before T79a it was the manifest's name, which meant a
hand import of *any* gallery file was refused by `validated_slug` before the signature was reached.
The stem still goes through that same check, so `My Stack.toml` is refused by name exactly as before.

## Acceptance criteria

- Capture a working project, apply it under a new name, and both sites serve correctly at the same
  time with no manual steps.
- Applying a blueprint whose PHP version is missing installs it and continues without user
  babysitting beyond the initial confirmation.
- A blueprint exported on Windows applies on macOS (no absolute paths, no OS-specific service names).
- `--dry-run` output matches exactly the actions the real run performs.
