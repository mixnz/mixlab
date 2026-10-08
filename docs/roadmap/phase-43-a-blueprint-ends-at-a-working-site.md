# Phase 43 — A blueprint ends at a working site

*Goal: after applying a blueprint in MixLab, the next thing a person sees is their website, or the
exact commands that bring it up, one click from running in MixLab's Terminal.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md](../specs/2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md).
T205a–T205c: [2026-10-09-t205a-t205c-what-hand-testing-the-gallery-left-open-design.md](../specs/2026-10-09-t205a-t205c-what-hand-testing-the-gallery-left-open-design.md).

---

- [x] **T205** **(P)** `[scaffold] archive` (schema 2, ADR 0061: a blueprint is written at the
      lowest schema that holds it) and WordPress built on it; `[[next_steps]]` with `once`, `serve`
      and `open`, read through `projects.blueprint_id` and shown by `AfterApply`, the Sites and
      Projects screens, the welcome page and `mix`; every MixLab apply asks for a front end, ticks a
      trusted scaffold and opens the browser when no step is needed; the Terminal gains `env` and
      `pathPrepend` for local targets, `onRestore`, and one-shot and draft tabs from MixEngine; five
      PHP blueprints (`cakephp`, `codeigniter`, `craft`, `statamic`, `yii`) and steps for the whole
      gallery; `mixengine-packages` re-publishes.
- [x] **T205a** A framework blueprint on PostgreSQL started on a database of its own (found by
      hand on 2026-10-08): `rails new --database=postgresql` pointed at `<name>_development` with no
      account, and Django's template stayed on SQLite. `[[services]] dotenv` now plans a
      `write_dotenv` step after the scaffold, and with the person's agreement (a box in MixLab,
      ticked for a signed blueprint; `--write-dotenv` or a question in `mix`) the apply appends the
      URL of the database it made to `.env`, private to this account. Rails reads
      `DEVELOPMENT_DATABASE_URL` through an application template, so `bin/rails test` keeps its own
      database; Django's template reads `DATABASE_URL`.
- [x] **T205b** `check-blueprints` compared the signed gallery with `master` and nothing else, so
      the starters `publish-blueprints` puts beside it could drift from `src/blueprints/starters/`
      with no reminder. `tools/blueprints.py --starters` now compares each `<name>-starter.zip`
      entry by entry (names and bytes; a zip's own bytes carry checkout times), each file published
      as itself byte for byte, and names a starter the release holds that the tree dropped;
      `gallery.yml` dispatches on a `starters/**` push.
- [ ] **T205c** **(P)** Ctrl+C in a local Terminal tab did not stop a running command in
      PowerShell, while Git Bash in the same window did: found by hand on 2026-10-08 with the
      window started from a background process with no console of its own. Started from a
      terminal it works, so a shell can inherit "ignore Ctrl+C" from however MixLab itself was
      launched. Make the PTY's child reset that (`SetConsoleCtrlHandler(NULL, FALSE)` or a
      process-creation flag) so the key works however MixLab started, and measure it from the
      Start menu, a shortcut and the tray.
- [ ] **T205d** Django on Windows ARM64 has no PostgreSQL driver: `psycopg-binary` 3.3.6 ships no
      CPython 3.13 wheel for it (measured 2026-10-09), so the blueprint's first step fails there.
      Measure whether the pure `psycopg` with the `libpq.dll` MixEngine's PostgreSQL package
      carries is enough.

**M43** On all three systems, `laravel` and `wordpress` applied from MixLab's Blueprints screen on a
home with no web server open in the browser on their own, and `nextjs` serves its page after one
click on *Run the required steps*.
