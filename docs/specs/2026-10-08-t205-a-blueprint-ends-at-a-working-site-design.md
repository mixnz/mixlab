---
status: implemented
date: 2026-10-08
task: T205
---

# T205 — A blueprint ends at a working site (design)

A person who applies a blueprint in MixLab expects the next thing they see to be their website. Today
that holds for one path through one window, and for no blueprint that runs a program of its own.
This design closes the gap without making `mixengined` supervise anybody's application. MixEngine
does every step that can be done unattended, which now includes downloading a release archive. For
the rest, the blueprint declares what a person still has to run, and MixLab puts those commands one
click away in its own Terminal.

## What this closes

Read against `master` on 2026-10-08 (`blueprints/gallery/`, `api/apply.rs`, `AfterApply.tsx`,
`Blueprints.tsx`):

| # | Gap | Blueprints | Cause |
|---|---|---|---|
| 1 | Nothing ever answers on the site | `nextjs`, `strapi`, `express-mongodb`, `django`, `rails` | `SiteKind::NodeApp` is *"a declaration and no more"* and a `reverse-proxy` upstream is a program the person runs. The welcome page (T124) says *"Start your development server"* without naming a command |
| 2 | No web server on a fresh home | every one, applied from the Blueprints screen | `Blueprints.tsx` opens `ApplyDialog` without `withFrontEnd`. Only Quick Start asks for a front end (T115) |
| 3 | An empty directory behind a configured site | `laravel`, `laravel-mongodb`, `symfony`, `drupal`, `nextjs` | The scaffold box starts unticked, so the ordinary click leaves the step `NotRun` |
| 4 | The site is ready and nothing opens it | every one | `AfterApply` offers a button and deliberately does not navigate |
| 5 | WordPress arrives as an empty directory | `wordpress` | Its only official distribution is a zip archive, and `[scaffold]` can only run a command that is the same in `cmd.exe` and `sh` |
| 6 | What is left to do lives in a `description` | `laravel-mongodb`, `rails`, `vite`, `strapi`, `express-mongodb` | There is nowhere else to put it. T124 deferred `[blueprint] next_steps` to a separate task, and this is that task |

There is also a coverage gap. The gallery has no PHP framework outside Laravel, Symfony, Drupal and
WordPress, no PHP project paired with PostgreSQL, and no PHP project without a database.

## Goal

After `blueprint.apply` from MixLab:

- **Every PHP blueprint, and `static`, opens in the browser and shows the application's own page**
  (or its web installer) with no step left for the person. The two that bring no source on
  purpose, `static` and `php-mysql`, open on MixEngine's welcome page, which says where to put the
  files (T124). `vite` is static too, but needs a build first, so it belongs to the next point.
- **A blueprint whose site is a program shows what to run,** with each command one click from
  running in a MixLab Terminal tab that has the project's runtimes on its `PATH`. Those commands can
  be saved as a Terminal target before anything runs, so the person can start the site later.
- `mix blueprint apply` prints the same steps, and the welcome page in the browser repeats them.

## Not in scope

- **`mixengined` supervising a site's program.** This was weighed and rejected. D1 records why.
- **Ports.** A `node-app` site keeps the port its blueprint names. Two `nextjs` projects running at
  the same time still collide on 3000, and the step's `note` says so. Allocating ports is a
  property of something that supervises, and nothing here does.
- **Several sites in a blueprint.** That is T204a. This design only leaves room for it (D4's `site`).
- **A virtualenv or a Bundler path per project.** `django` and `rails` steps install into the
  runtime when the person runs them, and say so. A per-project environment is a later task.
- **`done_when` checks** (the daemon marking a step done by looking at the disk). The steps are a
  static list.

## Relation to T204 and T204a

- **T204** ([design](2026-10-08-t204-a-manifest-holds-every-site-design.md), merged as `63a2016f`)
  changed the **project** manifest, `core::manifest`, and explicitly left blueprints alone. The two
  designs touch different types. The only shared files are `api/apply.rs`,
  `docs/features/blueprints.md`, `CHANGELOG.md` and the roadmap, where they meet as text and not as
  design. This design is built on top of it.
- **T204a** (several sites in a blueprint) had planned `schema = 2`. This design takes `schema = 2`
  for the archive scaffold (D2, D3), so **T204a moves to `schema = 3`**, and phase 42 and T204's
  design already say so. D3's rule (*a file is written at the lowest schema that holds it*) means
  T204a's bump affects only blueprints with more than one site.
- `[[next_steps]]` entries carry an optional `site` from the start (D4). T204a makes it required
  only when a blueprint has more than one site, and needs no change to the step format.

## Is there an ADR here? Yes, one

**ADR 0061 — a blueprint is written at the lowest schema that holds it.** Bumping the schema is a
decision about the published gallery, and the gallery leaves this repository: `mixengine-packages`
signs and publishes every manifest, and installed builds of every age import them. T204's design
already named the schema bump as the one ADR-worthy decision. Recording the rule once means T204a
and every later bump inherit it instead of re-arguing it.

The rest needs no ADR:
- D1 confirms what `SiteKind::NodeApp`, T43 and ADR 0031 already say.
- The Terminal changes are features of one module.
- The cross-module bridge follows the precedent `explore_data.rs` set for `db`.

## Decisions

### D1 — MixEngine never runs a site's program; it says what to run

There were three candidates, judged against this repository's rules:

| | MixLab spawns it | A Terminal tab | `mixengined` supervises it |
|---|---|---|---|
| Survives closing the window or a reboot | no | no, unless saved (D10) | yes |
| A visible process the person can stop | no | **yes, it is the tab** | through the Dashboard |
| Rules | breaks *"the `mixengine` module is a client, not a second daemon"* | none broken, see D11 | needs an ADR reversing T39a D3 and T43 |
| Security | duplicates T78a's trust gate in a client | the person presses Enter | a blueprint's command runs again at every start and every activation |
| Headless installs | unaffected | unaffected | a new standing behaviour on servers |

The supervisor would answer more questions, at the price of turning a command someone else wrote
into a standing process. The Terminal answers the one that matters (*what do I run, and where*)
while keeping the person as the one who runs it. **So `mixengined` stays out of running a site's
program, and the gap is closed by information plus a tab.**

### D2 — `[scaffold] archive`: a release archive downloaded and unpacked by the apply

```toml
schema = 2

[scaffold]
archive = "https://wordpress.org/latest.zip"
strip = "wordpress"        # optional: the archive's single top-level folder
needs_empty_dir = true
```

- **`archive` and `command` are mutually exclusive.** A manifest with both, or with neither, is
  refused at read time. `needs_empty_dir` keeps its meaning. `needs_npm_safe_dir` is refused beside
  `archive`, because it describes a command.
- **`https://` only**, refused at read time otherwise. **No checksum** (decided 2026-10-08):
  `latest.zip` changes with every WordPress release, so a pinned hash would break the gallery entry
  within weeks. HTTPS plus the consent below is the trust.
- **Format from the URL suffix**, through `core::install::archive`. Its module documentation
  already argues why the suffix decides, and it already refuses an entry path that leaves the
  target directory (`safe`, `enclosed_name`). A suffix outside its closed set is refused at plan
  time, naming the suffix.
- **Unpacked into a staging directory** under the home's `cache/`, then the contents of `strip`
  (or of the root, when `strip` is absent) are moved into the project root. The move is a rename
  when both are on one volume and a copy otherwise, because a project on another disk is ordinary
  (ADR 0040). A `strip` the archive does not contain fails the step and names what the archive did
  contain. The staging directory is removed either way.
- **The core gains one public entry point** for this, beside the scaffold, so that the daemon's
  apply does not reach into `core::install`, whose `archive` module is `pub(crate)`. It reuses the
  installer's downloader (proxy, resume) and `archive::extract`.
- **A size ceiling of 1 GiB** on the download. Past it the step fails, naming the size.
- **The same consent as a command (T78a).** Unpacking is not running, but the PHP files it writes
  run the moment somebody opens the site. So an archive from a blueprint nobody vouches for is the
  same risk as a command from one. The plan shows the URL where it would show a command.
  `ScaffoldConsent` gains `archive: Option<String>`, and `command` becomes `#[serde(default)]`.
  Exactly one of the two must match what the plan showed. `--run-scaffold` and
  `--run-untrusted-scaffold` cover both kinds.
- **A new plan action, `PlanAction::FetchArchive { url, strip }`**, in the place `RunScaffold`
  holds. All other scaffold rules carry over: it is checked at plan time (`needs_empty_dir`), it
  has no timeout and can be cancelled through `job.cancel`, and a failure is a failed step rather
  than a failed apply. The download's progress goes to the job log.

### D3 — A blueprint is written at the lowest schema that holds it (ADR 0061)

- `SCHEMA` becomes 2, the highest this build **reads**.
- `render` writes `schema = 2` only when the manifest uses `archive`, and `schema = 1` otherwise.
  So every gallery file but `wordpress.toml` stays readable by every installed build, and the
  round-trip test (`every_gallery_blueprint_is_its_own_rendering`) holds file by file.
- **`[[next_steps]]` does not raise the schema.** A build that predates it ignores unknown keys
  (`BlueprintManifest` denies none) and applies the blueprint exactly as its author intended.
  The steps are information, and losing them loses no behaviour. That is the line ADR 0061 draws:
  *a key that changes what an apply does raises the schema, and a key that only informs does not.*
- An older build that imports `wordpress.toml` refuses it through `UnknownBlueprintSchema`, which
  names the blueprint. Before this rule, the same file would have failed on a missing `command`.
- An older build that imports a schema-1 file with steps stores its own rendering
  (`store::save`), and that rendering has no `[[next_steps]]`. That copy loses the information and
  nothing else, which is the same outcome as ignoring the key.

### D4 — `[[next_steps]]`: what is left, in three kinds

```toml
[[next_steps]]
kind = "once"
run = "npm install"

[[next_steps]]
kind = "serve"
run = "npm run dev"
note = "Next.js listens on 3000; a second project on the same port will not start."

[[next_steps]]
kind = "open"
path = "/wp-admin/install.php"
credentials = true
```

| `kind` | Means | Keys of its own |
|---|---|---|
| `once` | A command to run one time | `run` (required) |
| `serve` | A command that keeps running | `run` (required) |
| `open` | Finish something in the browser | `path` (optional, starts with `/`) |

Every kind also takes:
- **`note`**: one sentence shown under the step.
- **`optional`** (default `false`): the site answers without this step. Laravel's welcome page
  needs neither `php artisan migrate` nor `npm run dev`, so both are optional there, while
  Next.js answers nothing until `npm run dev` runs. D7 decides on this flag whether a browser can
  open straight away, so it is declared and never inferred from the kind.
- **`credentials`** (default `false`): this step needs the database account the apply created,
  whether in a web installer (`open`) or in a `.env` (`once`). See D8.
- **`site`** (optional): which site the step belongs to. In schema 1 and 2 a blueprint has one
  site and `site` must be absent. T204a defines its value.

**Rules on `run`, checked at read time.** They follow the scaffold's own rules, because the line
must mean the same thing in `cmd.exe`, PowerShell and `sh`:
- one line, at most 512 characters, with a program for its first word;
- no `&&`, `||`, `;`, `|`, `>`, `<`, backtick, `$` or `%`;
- no `\`, so that no path is spelled for one OS;
- `{project}` is the only token, expanded through `domains::slug` exactly as for `[scaffold]`
  (ADR 0030).

A step that breaks a rule makes the whole manifest unreadable and names the step by its position.
A half-read list of instructions is worse than none.

**Order is execution order.** In a person's run, the `once` steps go before the `serve` steps.

### D5 — A project points at its blueprint, and the steps are read through it. No migration

- **The apply writes `projects.blueprint_id`.** The column has been in `0001_initial.sql` and in
  `data-model.md` from the start, reserved for blueprints (T39 left it `NULL` as *"Phase 8's"*),
  and nothing has written it until now. The `RegisterProject` step sets it through a core function
  and not through `ProjectCreate`, so a project somebody creates by hand still names no blueprint
  and the wire type is unchanged. A resumed apply sets it on a row that still has it `NULL`.
- **Steps, trust and capture are read through it.** `[[next_steps]]` comes from the blueprint row's
  manifest, and `trusted` from that row's `trusted`. `{project}` is expanded at read time through
  `domains::slug` of the project's name. Nothing is ever stored expanded, so capture never has to
  turn a slug back into the token. That is a replacement that would rewrite any word containing
  the slug: a project called `app` would turn `php artisan app:install` into a token.
- **A reference, not a copy, deliberately.** When a blueprint is corrected (by a new build's
  gallery, or a signed file imported with `--overwrite`), the projects made from it see the
  correction. The cost is the other side of the same thing: overwriting a blueprint with something
  unrelated changes what its projects show, and their trust follows the new row. Overwriting takes
  an explicit `--overwrite` or a capture over that slug, and the steps belong to the file that is
  there now, so this is the honest reading. `ON DELETE SET NULL` already covers a blueprint that
  goes away, and this build has no `blueprint.delete`.
- **No new column, so no migration.** Avoiding one is a reason, not a convenience: a migration
  brings an upgrade fixture (`crates/mixengine-testkit/fixtures/upgrade/`), and T204a would have
  had to number its own migration after it.
- **The account the apply created is not stored.** There is no `databases` table, only the
  keyring, and the name T202 settled on is known to the apply alone. It travels in the apply's
  answer (D6), which is the moment an installer asks for it. After that, MixLab's Dashboard already
  reveals any account's password through `database.credentials`.
- **`blueprint.capture` writes the steps of the project's blueprint** into `[[next_steps]]` as that
  blueprint has them. A project with no `blueprint_id` (created by hand, or applied before this
  task) has none and captures none.

### D6 — Where the steps are read

- **`BlueprintApplied` gains `next_steps` (expanded), `trusted`, and
  `database: { service, database, user }`.** The last one is the account the `CreateDatabase` step
  really used, never a password. `AfterApply` and `mix blueprint apply` render from it.
- **`ProjectDetail` gains `next_steps` and `trusted`**, read through `blueprint_id` (D5). MixLab's
  site and project screens read it, and so does `mix project show`. All of these are optional
  members under ADR 0019. There is no new method, so `check-client-surface` has nothing to add.
- **The welcome page (T124) lists the `once` and `serve` commands** in place of the kind's generic
  sentence when the site's project has steps. It shows the commands and their `note`, and never
  anything about the database: the page is reachable from the LAN (T74), and T124's D5 already
  keeps account names off it. It stays English-only (T124's D7). A site owned by an
  extension, or a project with no steps, keeps today's sentence.

### D7 — MixLab: every apply aims at a working site

- **`front_end: true` on every apply from MixLab.** The `withFrontEnd` prop is removed from
  `ApplyDialog`. T115 kept it off because *an apply is about one project*, but the question a person
  asks MixLab is always *give me a site that works*. A home that already has a front end is left
  alone, as T115 promises. `mix` keeps `--with-front-end` opt-in, since its users include servers.
- **The scaffold box starts ticked when the blueprint row is `trusted`.** That covers `builtin`,
  `captured` on this machine, and `imported` with a verified signature. The command or URL is
  still printed in full above the box, and the consent still names it. The default changes, the
  gate does not. An untrusted blueprint's box starts unticked, as today.
- **The browser opens by itself when nothing else is needed.** That means the apply has no failed
  step and no `once` or `serve` step is without `optional`. In that case `AfterApply`, after its
  grant-and-start chain, calls `openUrl` once and stays on screen with the address and the steps
  panel (D8), so the optional steps are still in view. The URL is the site's, plus the `path` of
  the first `open` step when there is one, so WordPress, Drupal and Craft land on their installer.
  This reverses the *"offer the address, do not navigate by itself"* paragraph in
  `AfterApply.tsx`. The click on Apply is the request, and the request was for a website. When a
  step is still needed, the site would answer 502 or the welcome page at this moment, so the
  dialog shows the steps instead and the browser waits for the person.

### D8 — MixLab: the steps panel after an apply

`AfterApply` gains a panel drawn from `next_steps`, in order:

- **`once` and `serve` rows:** the command in monospace, its `note`, an *optional* mark where it
  applies, then **Copy** and **Run** (a Terminal tab with that one line, D11). A `serve` row adds
  **Save as Terminal target**, which saves the target there and then and turns into *Saved in
  Terminal* (D11). Saving without running is the ordinary case for someone who will start work
  tomorrow, and a button that says Save has to leave something saved.
- **Above the rows, Run the required steps**, shown when at least one step is not `optional`. It
  opens one tab that runs the required `once` steps in order, then the first required `serve`.
  Each further required `serve` gets a tab of its own, because a process that keeps running holds
  its tab. Optional steps are never run by this button: each one is the person's own choice, on
  its own row.
- **`open` rows:** an **Open** button for the site URL plus `path`.
- **The credentials block, in `AfterApply` only**, drawn once when any step has `credentials`:
  the database's host, port, name and user from `BlueprintApplied.database`, each with Copy, and a
  **Reveal password** control that calls `database.credentials`. Credentials never enter a
  Terminal target, a tab state or the welcome page.
- **With the Terminal module hidden** (`isModuleVisible("terminal")` is false), the Run buttons
  read *Turn on Terminal and run*, which is the `builtInAfterEnabling` precedent from T110: the
  shell turns a module on for a tab request that names it. Save opens no tab, so it keeps its
  label, and the target is there when Terminal is turned on. Copy always works.
- **The panel stays where it is.** A dialog belongs to the tab that opened it (*Dialogs belong to
  their tab*, below), so Run brings the Terminal tab to the front and `AfterApply` waits in the
  MixEngine tab with the password and the remaining steps; going back to that tab finds it as it
  was left.
- **The same panel**, without Run/Save's first-run framing, is drawn on the **Projects** screen
  from `ProjectDetail.next_steps`, so closing the dialog loses nothing. A project's row opens **in
  place**, under itself, through its *Details* button or a click anywhere on the row, and holds
  the project's runtimes, the sites its `mixengine.toml` declares and its steps; one row is open
  at a time. *Why in place:* the first version drew all three as cards under the whole table, and
  with ten projects a click on the first opened nothing anyone could see. The row keeps the Sites
  screen's layout (no leading chevron), and its less common actions (Edit, Write manifest, Delete)
  sit in a ⋮ menu, as Sites' do. There, a step with `credentials` carries a link to the site's
  database on the Dashboard, where the existing password control lives (D5 says why the account
  is not repeated here).
- **A Ruby that cannot build the gems is said above the steps** (T206a's devkit, where the steps
  are). Measured by hand: `rails new` on a Windows Ruby with no `msys2` ran `bundle install`, which
  stopped at the first gem with a C extension, and `rails server` then listed sixty missing gems; a
  note under `gem install rails` had said so, and a note is not read before Run. The panel reads
  the Ruby the project resolves to, whether its release `lacks` `native gems`, and whether a
  devkit is installed (`devkitNeed.ts`). When one is missing it says so above the rows with
  **Install devkit (size)** beside it, the Languages tab's button, and *Run the required steps* is
  disabled until the install's job ends.
- **And it is offered before the apply, where the decision is made.** The Apply dialog's plan reads
  the Ruby line the blueprint pins (`register_project`'s `pins.ruby`, `planDevkitNeed`) and, when
  that line cannot build native gems and no devkit is installed, shows *Install the devkit with it
  (size)*, ticked. The devkit installs as its own job beside the apply. One install job for the
  whole window (`devkitInstall.ts`), so the panel that follows shows it under way rather than
  offering it twice.
- **On the Languages tab** a Ruby row carries one mark, its reasons on hover, inside the row's
  *needs* cell: the row is a six-column grid, and the first version's two marks and a button were a
  seventh child that pushed *Install* onto the next line. The devkit is offered once, above the
  list, and only where a Ruby is installed: someone who does not use Ruby is not asked about it.
- **One place to read the steps.** *What to run* in a site's ⋮ menu on the Sites screen goes to
  the Projects screen with that project's row open (`projectsNavigation.ts`), and an apply that has
  just finished leaves its project to be opened there on the next visit; neither draws a copy of
  the panel of its own.

### D9 — Terminal: a local target carries `env` and `pathPrepend`

- **`SavedLocalTarget` gains `env?: Record<string, string>` and `pathPrepend?: string[]`.**
  `TerminalTarget`'s local branch gains the same, and `local.rs` sets them on the spawned shell:
  `env` with `Command::env`, and `pathPrepend` joined with the platform separator ahead of the
  inherited `PATH`.
- **Two keys, not one**, because `PATH=` in `env` would replace the inherited `PATH`, and
  `%PATH%` / `$PATH` is spelled differently in every shell.
- **The form gains both fields** under *Run on connect*, with the same caption about plain text,
  because `terminal-hosts.json` is readable.
- **Local targets do not sync** (`sync.ts` sends only `ssh`), so a home's absolute path never
  leaves the machine. Nothing changes there.
- **What MixEngine passes**: `pathPrepend = [PathReport.directory]` (`<home>/bin`, from
  `path.status`) and `env = { MIXENGINE_HOME: DaemonStatus.home }`. The shims then resolve the
  project's pinned versions from `cwd`, whether or not the person ever ran `mix path install`.
  The Terminal module learns none of this: to it these are two generic fields.

### D10 — Terminal: what a target's opening does with its commands

Today a restored tab always reopens its session and always types its target's *Run on connect*
with Enter (`TerminalTab.tsx`, the restore effect). For a dev-server target, every launch of
MixLab would start every such server at once. Every saved target, local and SSH, gains:

| `onRestore` | Opening the target, or a tab MixLab restores |
|---|---|
| `run` | opens and types the commands **with** Enter. This is the default, so a target written before this field keeps today's behaviour |
| `type` | opens and types the commands **without** Enter, leaving them on the prompt |
| `none` | opens the shell at `cwd`, or connects the SSH session, and types nothing |

- It applies to **every opening** of the target: the Open button, a double click in the list, and a
  tab MixLab restores (`openingFor` in `session.ts`). The form names it *When this target opens*.
  *Why not restoring only, as first written:* measured by hand, a target set to *Just open* or
  *Type them* still ran its commands when opened from the list, which is not what anyone choosing
  those means; the field name *When the tab comes back* did not say otherwise clearly enough.
- `openingKeystrokes` gains a `press: boolean` argument, which keeps it pure.
- The form shows it as a three-way choice beneath *Run on connect*, disabled while that field is
  empty.
- Targets saved from an apply (D11) start at `type`.

### D11 — Terminal: a one-shot run through the launch queue, and a save through a module action

The `mixengine` module never imports `terminal`. A run opens a tab, so it goes through
`launch::request` with `module_id: "terminal"`, which is the path `explore_data.rs` uses for `db`;
the Terminal module validates what arrives (`parseTerminalTabState`), as it already does for every
restored state. A save opens nothing, so it goes through a **module action** instead (below).

- **One-shot run.** `TerminalTabState`'s local branch gains `env` and `pathPrepend` (persisted) and
  `run` (a list of lines, **never persisted**). The tab types `run` once, then writes its state
  without it. A restored one-shot tab therefore reopens its shell at `cwd` with the right `PATH` and
  runs nothing. This widens the line `tabState.ts` draws (*ids only, `cwd` the one path*) by two
  fields that are facts about this machine, like `cwd`. Losing them on restore would hand the person
  the system's `npm` without a word, which is the worse failure.
- **Save, as a module action.** `ModuleDefinition` gains `actions`, named functions one module
  lends the others; the registry hands them to `core/moduleActions.ts`, and another module calls
  one by the lender's id, with a payload the lender validates. The Terminal lends `saveTarget`:
  name `<project> · <run>`, the shell a new tab would open (Settings' default, else the first
  detected), `cwd` = project root, D9's `env` and `pathPrepend`, *Run on connect* = the `serve`
  line, and `onRestore = "type"`. It saves through its own `addTarget`, so the store and sync stay
  the Terminal's, and it answers with the entry's id, which is what lets the button say *Saved*. A
  second press finds the entry already saved for the same name, folder and command and makes no
  second row.

  *Why not a draft tab, as first built.* The first version opened a Terminal tab holding the
  target filled in and **unsaved**, for the person to check and save. Measured by hand: the button
  said Save and saved nothing, the tab opened behind the dialog the button was in so nothing seemed
  to happen at all, and the form was taller than the pane with its Save button out of reach. A
  person who pressed Save and found no target on the next launch reads that as MixLab losing their
  work. The person can still check and edit the entry: it is in the Terminal's list like any other.
- A target outlives its project. When `cwd` no longer exists, the Terminal says so on open instead
  of starting the shell in the home directory.

### D12 — Steps from a blueprint nobody vouches for are typed, never pressed

`trusted` in `BlueprintApplied` and `ProjectDetail` is the blueprint row's own (D5), so it is
`false` for an `imported` blueprint whose signature is `missing` or `rejected`. The panel words it the way
T79b does (*unsigned*, *mismatched*). For such a project:
- **Run** and **Run the required steps** type the lines without Enter, and the panel says why.
- A saved target's `onRestore` is `type` (D11), for every blueprint, so no launch of MixLab starts
  a server from a saved line by itself; the person can change it on the entry.

The steps came from the same file as the scaffold, so they get the same gate.

### D13 — Five PHP blueprints, and steps for the whole gallery

New entries, each earning its place under *"a coverage surface, not a list of favourites"*:

| Slug | Site | Services | Scaffold | Its gap |
|---|---|---|---|---|
| `cakephp` | php-fpm, `webroot` | mariadb | `composer create-project cakephp/app . --no-interaction` | the only `webroot` |
| `codeigniter` | php-fpm, `public` | mysql | `composer create-project codeigniter4/appstarter . --no-interaction` | the only framework on MySQL (`php-mysql` has none) |
| `craft` | php-fpm, `web` | postgres | `composer create-project craftcms/craft . --no-interaction` | the only PHP project on PostgreSQL |
| `statamic` | php-fpm, `public` | none | `composer create-project statamic/statamic . --no-interaction` | the only PHP project with no service (flat files) |
| `yii` | php-fpm, `web` | mariadb | `composer create-project yiisoft/yii2-app-basic . --no-interaction` | the only `web` that is an application rather than a CMS |

All use PHP 8.4 and Composer 2, and all set `needs_empty_dir = true`. **Craft and Statamic run
post-create scripts that may prompt.** The first implementation step measures each one with stdin
closed, as the scaffold runs it. A command that waits gets `--no-scripts`, and the work it skipped
becomes a `once` step: a prompt is harmless in a Terminal tab and a hang in a job.

Steps for every entry (indicative; the implementation measures each):

*opt* marks `optional = true`, and *cred* marks `credentials = true`.

| Blueprint | Steps | Opens by itself (D7) |
|---|---|---|
| `laravel` | once `php artisan migrate` (*opt*, *cred*; note: set `DB_*` in `.env` first, since Laravel starts on SQLite); once `npm install` (*opt*); serve `npm run dev` (*opt*; note: Vite, for editing assets) | yes |
| `laravel-mongodb` | once `composer require mongodb/laravel-mongodb` (*opt*; moved out of `description`) | yes |
| `symfony`, `cakephp`, `codeigniter`, `yii` | none | yes |
| `php-mysql`, `static` | none (welcome page) | yes |
| `drupal` | open `/core/install.php` (*cred*) | yes, on the installer |
| `wordpress` | **archive** (D2); open `/wp-admin/install.php` (*cred*) | yes, on the installer |
| `craft` | open `/admin/install` (*cred*; note: choose PostgreSQL) | yes, on the installer |
| `statamic` | once `php please make:user` (*opt*); open `/cp` | yes, on `/cp` |
| `nextjs` | serve `npm run dev` (note: port 3000) | no |
| `strapi` | once `npx create-strapi@latest .`; serve `npm run develop` (note: port 1337) | no |
| `express-mongodb` | **archive** (a starter of this repository's); once `npm install`; serve `npm start` (note: `MONGODB_URI` when MongoDB is not on 27017) | no |
| `django` | once `python -m pip install django` (note: installs into the runtime this project pins, shared with other projects on it); once `python -m django startproject --template <django-starter.zip> config .` (note: the .test domain is allowed); serve `python manage.py runserver 127.0.0.1:8000` | no |
| `rails` | once `gem install rails` (same note); once `rails new . --database=postgresql` (*cred*); serve `ruby bin/rails server -p 3000` (note: set `DATABASE_URL` from the account) | no |
| `vite` | once `npm create vite@latest .`; once `npm install`; once `npm run build` | no |

- **`laravel` installs its npm packages as a step of its own.** `composer create-project` writes
  `package.json` and installs nothing from it, so `npm run dev` without `npm install` before it
  answered *Cannot find package 'vite'* (found by hand, 2026-10-08). Every `npm run` step in the
  gallery now comes after a step or a scaffold that installs its packages, asserted over the
  shipped set (`every_npm_run_step_comes_after_its_packages_are_installed`).
- **Run the required steps has to be able to finish, and the site has to answer after it.** Two
  more entries failed that by hand. `express-mongodb` required `node index.js`, a file nothing
  writes, so the button always ended at *Cannot find module* (`no_required_step_runs_a_file_nothing_creates`).
  Making the step optional left a blueprint with no site at all, so it now unpacks a **starter**:
  an Express 5 server reading `PORT` and `MONGODB_URI`, whose page says whether MongoDB answers.
  Its source sits beside the gallery (`src/blueprints/starters/express-mongodb/`), and
  `publish-blueprints` zips it onto the same release as the signed gallery, from the same commit
  (`every_starter_archive_is_in_the_tree`). Its steps are `npm install` and `npm start`. `rails new
  --database=postgresql` points at `<name>_development` with no account while MixEngine made the
  database `{project}` with one, so every page answered *ConnectionNotEstablished*: the step carries
  `credentials`, the panel's database block names the port beside the host, and the server's note
  says to set `DATABASE_URL` (`rails_new_carries_the_database_account`). The password is never put
  in a Terminal tab or target (D8).
- **`craft`'s installer is told PostgreSQL.** It offers MySQL first, and kept against PostgreSQL's
  port it waited and failed on *MySQL server has gone away* (found by hand, 2026-10-08). Until its
  `.env` names a database, every Craft page also takes about 50 s, retrying the default MySQL
  account; that is Craft's, not the server's. The `open` step's note names the driver
  (`an_installer_on_postgres_says_postgres`).
- **Django names its package `config` and not `{project}`.** `startproject` takes a Python
  identifier, and the slug of `My Blog` is `my-blog`. `python -m django` rather than
  `django-admin` needs nothing on `PATH` beyond the `python` shim.
- **Django starts from the gallery's template.** The stock `startproject` leaves `ALLOWED_HOSTS`
  empty, which under `DEBUG` admits only `localhost`, so every page through the site answered
  *DisallowedHost* (found by hand, 2026-10-08), and a form posted over HTTPS would fail Django's
  origin check next. `starters/django/` is Django's own template with `.test` allowed and
  `https://*.test` trusted, published as `django-starter.zip` like the Express starter; Django
  renders it and still makes each project's `SECRET_KEY`
  (`django_starts_from_a_template_that_admits_its_test_domain`). Its database stays SQLite: wiring
  the PostgreSQL MixEngine made is the same manual step as Rails' `DATABASE_URL`.
- **`rails` after `gem install`** reaches the person's `PATH` through T131's globals shim. Two
  things are measured before the entry is written: that this holds on Windows, and whether
  `--database=postgresql` builds the `pg` gem there. If it does not, the step drops the flag and
  its note says how to switch to PostgreSQL.
- **Interactive initialisers move here from the scaffold.** `create-strapi` and `create-vite` were
  kept out of `[scaffold]` because a prompt hangs a job. In a Terminal tab a prompt is just a
  question.
- `description`s lose the instructions that move into steps.

## MixLab

- **Blueprints screen and Quick Start, `ApplyDialog`:** always sends `front_end: true` (D7). The
  scaffold box defaults to the blueprint's trust. A `FetchArchive` step renders like `RunScaffold`,
  with the URL in place of the command.
- **`AfterApply`:** opens the browser when no step is needed (D7). It always draws the steps panel
  when there are steps: Copy and Run per row, Save as Terminal target on `serve` rows, Run the
  required steps, and the credentials block (D8). Its three-call chain (grant,
  `service.start { project }`, `site.list`) is unchanged.
- **Projects screen:** a project's row opens in place with its runtimes, declared sites and the
  same steps panel from `ProjectDetail.next_steps`; Sites' *What to run* and a finished apply both
  land there with that row open.
- **Terminal module:** `env` and `pathPrepend` for local targets, under the form's *Advanced*,
  closed unless the target sets one (D9); `onRestore` (D10); the one-shot `run` state arriving
  through the launch queue, and the `saveTarget` action (D11). All generic Terminal features that
  name nothing from MixEngine. `npm run lint` keeps proving that `mixengine` imports nothing from
  `terminal`. The form scrolls on its own beside the targets list, so a pane shorter than the form
  still reaches Save and Open.
- **Dialogs belong to their tab.** `Modal` was drawn into `document.body`, over the whole window
  and the tab strip, so a dialog in one tab held every tab and a tab opened from it came up behind
  it. Each tab's pane now carries a layer (`components/Modal/host.tsx`, provided by `Workspace`), and
  a `Modal` opened inside a tab is drawn there: it covers that tab only, hides with it, and is there
  again on the way back. Escape and the shortcut count (`enterModal`) take a dialog into account only
  while its tab is on screen. A dialog outside any tab, such as Settings, still covers the window.
- **No new daemon method.** `BlueprintApplied`, `ProjectDetail` and `ScaffoldConsent` gain optional
  members, and `PlanAction` gains `FetchArchive`. `bindings/` is regenerated.
- **With MixEngine off,** nothing here runs: the panel and both bridges are in the `mixengine`
  module, and the Terminal changes work with no daemon at all.
- **Strings** go through `t()` in `en` and `vi` for the panel, the buttons, the trust sentence and
  the Terminal fields. The welcome page stays English (T124's D7).

## Testing

- **Manifest** (`blueprints/manifest.rs`): `archive` with `command` refused, and neither refused;
  `http://` refused; `needs_npm_safe_dir` beside `archive` refused. Each `run` rule in D4 refused,
  naming the step's position. `site` refused in schema 1 and 2. `render` writes `schema = 1` without
  `archive` and `2` with it. `schema = 3` is refused by name.
- **Gallery** (`tests/blueprint_gallery.rs`): `ENTRIES` is eighteen. Each file is its own rendering.
  Only `wordpress` is schema 2. The set carrying a `command` and the set carrying an `archive` are
  each asserted by name. Every `run` in the gallery passes D4.
- **Plan and apply** (`plan.rs`, `api/apply.rs`): `FetchArchive` is planned where `RunScaffold`
  would be. It is blocked on a non-empty root, and on a suffix outside `archive::Format`. A consent
  naming a different URL is refused. A `strip` that is missing fails the step and names the
  archive's top level. An entry escaping the root is refused (already covered in `archive.rs`, so
  reached here through one fixture). The staging directory is gone after success and after failure.
- **Through `blueprint_id`** (`projects.rs`, `blueprints/capture.rs`): an apply sets it, and a
  resumed apply sets it on a row left `NULL`. `project.create` never sets it. `ProjectDetail`
  expands `{project}` through the slug. Capturing a project named `app` whose step says
  `php artisan app:install` writes that line back unchanged. Overwriting the blueprint shows its
  projects the new steps, and an unsigned overwrite turns their `trusted` false. A project whose
  blueprint is gone, or that never had one, shows no steps. `BlueprintApplied.database` names the
  account T202 settled on (`laravel-1-2` when `laravel-1` was foreign), not the planned one.
- **No migration:** the upgrade fixtures under `crates/mixengine-testkit/fixtures/upgrade/` are
  untouched, and `cargo sqlx prepare` reports no schema change.
- **The archive path end to end, on all three systems by CI:** a local HTTP fixture serves a zip
  shaped like WordPress's (one top-level `wordpress/`, an `index.php`), and the apply leaves
  `index.php` at the project root. CI does not call `wordpress.org`. A suite that fails when
  somebody else's server is slow tells nobody anything about this code.
- **The real `wordpress.org` archive, by hand**, once per system before the entry ships: the
  installer answers at `/wp-admin/install.php`.
- **Steps** (`manifest.rs`): `optional` and `credentials` are read on every kind. `path` is
  refused outside `open`, and `run` is refused on `open`.
- **Measured by hand, before the manifests are committed:** each of the five new scaffolds with
  stdin closed, on Windows and on Linux. A command that waits is recorded and goes `--no-scripts`
  (D13).
- **Desktop** (`vitest`): `openingKeystrokes(text, false)` has no `\r`. `parseTerminalTabState`
  reads `env` and `pathPrepend`, and never returns `run` after the first write. `saveTarget` saves
  the entry its payload describes and makes no second row for the same name, folder and command;
  a dialog out of sight is not counted by `modalDepth`. The
  `onRestore` default is `run` when absent. The panel's choice of which buttons to draw follows
  kinds, `optional`, trust and module visibility. The auto-open decision (D7) is a pure function,
  tested over the gallery's own step lists, and its result matches the last column of D13's table.
- **By hand in `npm run dev:app`:** `nextjs` from the Blueprints screen on a home with no front end
  ends at the steps panel; Run the required steps shows the Next.js page at `https://<slug>.test`.
  `laravel` opens the browser on its own. `wordpress` opens on its installer, with the credentials
  block in MixLab. Save as Terminal target says *Saved in Terminal* without leaving the dialog, and
  relaunching MixLab finds the target in the Terminal's list; opening it leaves `npm run dev` typed
  and waiting. Run brings the Terminal tab to the front, and the MixEngine tab still holds the dialog.
  Applying from a home whose `mix path` was never installed still gives the tab Node 24.

## Documentation, when it lands

- `docs/features/blueprints.md`: *Scaffold commands* gains the archive form. *Built-in gallery*
  becomes eighteen entries, with each new one's reason (D13), and counts written in prose are
  removed where they are not the point (that section's own advice). A new section, *What is left
  to do*, covers `[[next_steps]]`.
- ADR 0061. Phase 42's T204a line already names `schema = 3` and this ADR.
- `docs/features/client-surface.md`: the Sites and Blueprints screens name the steps panel.
- A new roadmap phase for T205. Root `CHANGELOG.md` under `[Unreleased]`.
- `mixengine-packages`: re-run `publish-blueprints` at the full SHA. Its help text's
  *"The six manifests"* goes, by the same rule about counts.
