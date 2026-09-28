---
status: approved
date: 2026-09-28
task: T191
---

# T191 — A path is spelled the way this system spells one

Phase 9, beside the other things a release is judged by on the first machine it lands on. **(P)**
2026-09-28.

## The case

On Windows, MixLab and `mix` show a person paths like these:

- `C:\Users\dev\MixEngine\runtimes\php\8.3.33\bin/php`
- `D:/bulk/runtimes\php\8.3.33`
- `C:\Users\dev\blog/public/assets`, in the site form, while the site's own page says
  `C:\Users\dev\blog\public\assets` for the same directory

and a database tab titled with a whole `C:\…\shop.sqlite` path where the file name was meant.

None of these is one bug. Several parts of the code write a path with `/` on purpose, and each has a
good reason: TOML reads `\` as an escape, nginx eats a backslash, a site's `doc_root` is rendered
into configuration a person may read on another machine, and a manifest is written once for every
system. What is missing is the second half of the rule: **where a value in that form is joined onto
a path of this machine, or put in front of a person, it has to be respelled.** T80 met this once
(`…\mailpit/mailpit`) and fixed it for extension placeholders only. This task fixes it everywhere
else.

### Where it happens

Found by reading the code on 2026-09-28. The implementation plan re-checks each one on a real
Windows run before and after the change.

**The daemon, and so `mix` and MixLab alike.**

| # | Where | What goes wrong |
| --- | --- | --- |
| A1 | `mixengine-core/src/runtimes.rs:487`, `generate/recipe.rs:379`, `services/client.rs:84`, `install.rs:681` | `install_path.join(relative)`, where `relative` is a manifest's `provides` value such as `bin/php`, gives `…\8.3.33\bin/php`. No response field carries it. It reaches people through errors (`SmokeTestFailed`, the shim's "cannot run" when `php` is typed, a service that will not start) and through the daemon log's `program = …`, which is also what MixLab shows in its error messages and log viewer. |
| A2 | `mixengine-core/src/paths.rs:186-190` | A `[paths]` override is used as the file wrote it. The template recommends `"D:/bulk/data"`, so everything under a relocated directory comes out as `D:/bulk/data\mariadb\…`, and `StorageDirectory.path` (`mixengined --storage`) shows `D:/bulk/data` beside three siblings spelled with `\`. |
| A3 | `SiteSummary.doc_root`, a static route's `root` (`RouteTarget::Static`), and `PlanAction::CreateSite.doc_root` as `mix` prints it | These are stored with `/` (`sites.rs:237`) and reach both clients in that form, so a person on Windows reads `public/assets`. |

**MixLab's own code.**

| # | Where | What goes wrong |
| --- | --- | --- |
| B1 | `apps/desktop/src/modules/mixengine/siteState.ts:69` `joinDocRoot` | Joins with `` `${base}/${relative}` ``. The site form shows `C:\Users\dev\blog/public`, and the folder dialog is opened at that spelling. |
| B2 | `apps/desktop/src/modules/mixengine/storagePicker.ts:54` `oneFolderFor` | Joins with `/` "because the value goes into `config.toml`", and the same value is what the storage screen shows (`MixEngineTab.tsx:241`): `D:\Bulk/runtimes`. The reason no longer holds: `config::set_paths` writes through `toml_edit`, which escapes a backslash. Picking one folder per row (`pick`) already sends `\`, so the screen sends two spellings depending on which button was pressed. |
| B3 | `siteState.ts` `relativeToRoot`, fed by the folder dialog | Answers `public\assets` after Browse, while the same field loaded from the daemon holds `public/assets`. |
| B4 | `apps/desktop/src/modules/db/DbTab.tsx:71` `fileName` | Its comment says it splits on either separator; the regex `/[\/]/` splits on `/` only. A SQLite tab on Windows is titled with the whole path. |

### What is right today, and stays

- `doc_root_full` (`mixengine-daemon/src/sites.rs:1159`) splits the stored `/` form and joins it one
  part at a time. This is the pattern D1 makes shared.
- `projects.root_path` goes through `mixengine_platform::paths::in_full`, whose Windows side
  (`GetFullPathNameW`) already answers with `\`.
- Everything written *for another program*: nginx and PHP configuration (`/`), the `doc_root`
  column, TOML written by hand, the portable `~/…` form of an SSH key path sync carries
  (`core/ssh.ts:73`), and extension placeholders (T80). Those are stored forms, and this task does
  not touch them.

## Principle

**A path has two spellings, and each has one place.** The *stored* spelling is whatever the thing
it is written into needs: `/` for nginx, PHP, a manifest, the `doc_root` column, a `~/…` that
travels. The *native* spelling is the one this system uses, and it is the only one a person reads:
in `mix`'s output, in an API response, in MixLab. A stored value becomes native at the moment it
becomes a path of this machine, which means when it is joined onto one or put on a screen. It never
waits until a client happens to render it.

On macOS and Linux both spellings are the same string, so every change here is a no-op there. That
is why it has to be tested on Windows (D6).

## D1. One join for a relative path written with `/`

`mixengine-core` gets one function (working name `paths::join_stored`) that joins a relative path,
written with `/`, onto a base one component at a time: it splits on `/` (and on
`std::path::MAIN_SEPARATOR`, so a value that already arrived native is not treated as one name) and
pushes each part. It has no `#[cfg]`: on Unix the two separators are the same character.

It replaces the four `join(relative)` calls of A1, and `doc_root_full` is rewritten on top of it,
so the one correct copy becomes the only copy. `install::archive::safe` still runs first where it
runs today; this changes how a path is spelled, not which ones are accepted.

## D2. A `[paths]` override is respelled when the file is read

When `config.toml` is read, each `[paths]` value is rebuilt from its components
(`path.components().collect::<PathBuf>()`) after the existing checks (`is_drive_less_root`,
`is_drive_relative`, `names_a_directory`) have passed. On Windows that turns `D:/bulk/data` into
`D:\bulk\data`, and everything `Paths::new` builds from it follows. The file itself is not
rewritten. The template's advice to use `/` stays, because it is about TOML escaping, which is
still true for a person editing the file by hand.

`RequestedPaths` (what `mix config paths --data …` and MixLab send) gets the same treatment before
it is written. The value is then written to the file in native form and escaped by `toml_edit`.
This is what B2 relies on.

The implementation plan starts with a test that pins what `components().collect()` gives for
`D:/bulk/data`, `\\server\share/bulk` and `D:\bulk/data` on Windows, so D2 does not depend on
anyone's memory of how `PathBuf::push` behaves.

## D3. `doc_root` leaves the daemon in native spelling

`SiteSummary.doc_root`, the `root` of every `RouteTarget::Static` in `SiteSummary.routes`, and
`PlanAction::CreateSite.doc_root` are spelled natively by the daemon, through one
`mixengine_core::paths::native_relative(&str) -> String`. The columns keep `/` (`sites.rs:237` is
unchanged), and the input side already accepts either spelling on Windows, because
`relative_doc_root` normalises through `components()`. A round trip therefore stays exact: MixLab
reads `public\assets`, sends `public\assets` back (as `site.update` does with the whole route list),
and the column still holds `public/assets`.

`PlanAction::CreateSite.doc_root` is respelled where the plan is built (`blueprints/plan.rs`). The
apply that follows passes it to `site.create`, which accepts either spelling for the reason above.

This is done in the daemon rather than in each client, for the rule that neither client carries
logic: `mix` and MixLab would each need a copy, and they would drift apart. No type changes.

**The fields' doc comments in `mixengine-proto` are not changed in this task**, although "as
stored" no longer describes what Windows receives. Every byte of `mixengine-proto` is part of the
privileged helper's fingerprint (`packaging/helper-lock.mjs`, T182b). A comment edit there would
move `HELPER_VERSION`, and every install would then replace its elevated helper on the next update,
elevation prompt included, for no change in behaviour. The comment is corrected the next time a
release moves the helper for a reason of its own.

A blueprint is the exception, and it stays one. A manifest's `doc_root` is portable text written for
every system, and `mix blueprint export` keeps writing `/`.

## D4. MixLab joins and splits with the system's separator

A small `apps/desktop/src/core/paths.ts`, built on `IS_WINDOWS` from `core/platform.ts`:

- `SEPARATOR`: `\` on Windows, `/` elsewhere;
- `joinPath(base, relative)`: trims separators from the end of `base`, respells `relative`'s
  separators, and joins with `SEPARATOR`;
- `fileName(path)`: the last segment, splitting on either separator.

Then:

- **B1:** `joinDocRoot` uses `joinPath`.
- **B2:** `oneFolderFor` uses `joinPath`, and its comment about TOML goes, because D2 makes it
  untrue. Both buttons on the storage screen send and show one spelling.
- **B3:** `relativeToRoot` answers in native spelling whichever separator the dialog used. With D3,
  the field then holds the same string whether it came from Browse or from the daemon.
- **B4:** `DbTab.tsx` uses the shared `fileName`, and its private copy goes.

`core/ssh.ts`'s `~/…` form is left alone: it is the stored form of a key path that travels, and the
Rust side (`ssh/mod.rs`) is what reads it.

## D5. `mix` prints what the daemon sends

With D1–D3, `mix` has nothing of its own to change. Its only path joins are protocol paths
(`logs/job/{id}`), which are not filesystem paths. A `mix` that respelled paths itself would be the
client-side logic D3 avoids.

## D6. Tests

- **Unit, `mixengine-core`:** `join_stored` with `bin/php`, with `a/b/c`, and with a value already
  in native spelling. On Windows each result is asserted to contain no `/`; on Unix, to equal
  today's `join`. `native_relative` alike. One test goes through `Chosen::program`, so a join site
  that is later changed back to `join` fails a test.
- **Unit, `mixengine-core` config:** D2's cases, including a UNC share and an override that
  mixes both separators, through `load` and through `set_paths`.
- **Integration, `mixengine-cli`, run on Windows and ignored elsewhere with the reason said:** one
  sandbox home whose `config.toml` relocates `packages` with a `/`-spelled absolute path, one
  `fakeservice` package installed from the test registry into it, and one project with a site whose
  doc root is nested and which has a static route. The test runs `mix … --json` for `status`,
  `package list`, `site list`, `site show`, and `mixengined --storage`, walks every string in the
  answers, and fails on any string that starts with a drive letter or `\\` and contains `/`, or on
  a relative doc root or route root that contains `/`. It names the command and the JSON path of
  what it found (`tests-that-say-why`). This is one test for the rule itself, so a field added
  later cannot bring the bug back without failing it. Like `tests/package.rs`, it is ignored in a
  release build, which has no `fakeservice` recipe.
- **Unit, `apps/desktop`:** `core/paths.test.ts`, with the path style passed in rather than read
  from the user agent, so the Windows cases run on every CI runner. The existing `siteState` and
  `storagePicker` tests get Windows cases.

## MixLab

No new screen. These screens change what they show:

- **MixEngine → Sites**, the site form: the full doc root line and the doc root field (B1, B3, D3).
- **MixEngine → first start / storage** (`MixEngineTab.tsx`): the four directories (B2, A2).
- **MixEngine → Sites**, the route list: a static route's directory (D3).
- **MixEngine → Services**, the log viewer and any error naming a program (A1).
- **MixEngine → Packages and Runtimes**, install paths under a relocated directory (A2).
- **Database**, tab titles for SQLite files (B4).

No daemon method is added or removed, so `scripts/check-client-surface.mjs` has nothing new to
check.

## Out of scope

- **Paths MixLab's toolbox gets from the OS** (a file dialog, a SQLite path, an upload in REST) are
  already native. Only B4, which mis-splits them, is in scope.
- **Generated configuration under `etc/`** keeps whatever each program reads. It is disposable and
  not something a person is meant to read.
- **Log lines already written** keep the spelling they were written with.

## Risks

- **A comparison between two spellings of one path** that happened to match because both were
  wrong the same way. D6's integration test is where this shows up. The plan also searches for
  string comparisons between response paths, in the daemon and in `apps/desktop/src`, before
  changing any spelling.
- **D3 changes what a field holds on Windows.** Every client of `bindings/` is in this repository,
  and no stored value changes, so nothing outside needs migrating.
- **Three proto doc comments say "as stored" until the helper next moves** (see D3). The
  follow-up is to correct `SiteSummary::doc_root`, `RouteTarget::Static::root` and
  `PlanAction::CreateSite::doc_root` in the same release as the next `HELPER_VERSION` bump.
