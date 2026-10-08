# Phase 43 — A blueprint ends at a working site

*Goal: after applying a blueprint in MixLab, the next thing a person sees is their website, or the
exact commands that bring it up, one click from running in MixLab's Terminal.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md](../specs/2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md).

---

- [x] **T205** **(P)** `[scaffold] archive` (schema 2, ADR 0061: a blueprint is written at the
      lowest schema that holds it) and WordPress built on it; `[[next_steps]]` with `once`, `serve`
      and `open`, read through `projects.blueprint_id` and shown by `AfterApply`, the Sites and
      Projects screens, the welcome page and `mix`; every MixLab apply asks for a front end, ticks a
      trusted scaffold and opens the browser when no step is needed; the Terminal gains `env` and
      `pathPrepend` for local targets, `onRestore`, and one-shot and draft tabs from MixEngine; five
      PHP blueprints (`cakephp`, `codeigniter`, `craft`, `statamic`, `yii`) and steps for the whole
      gallery; `mixengine-packages` re-publishes.
- [ ] **T205a** A framework blueprint on PostgreSQL still starts on a database of its own: found by
      hand on 2026-10-08. `rails new --database=postgresql` points at `<name>_development` with no
      account, and Django's template stays on SQLite, so the database MixEngine made under the
      project's name sits unused until the person sets `DATABASE_URL` (Rails) or `DATABASES`
      (Django) by hand, with the password from MixLab's panel. Write it for them without putting
      the password in a Terminal tab, a target or a gallery file (T205's D8): an `.env` the
      framework already reads, or the server step's environment.
- [ ] **T205b** `check-blueprints` compares the signed gallery with `master` and nothing else: the
      starters `publish-blueprints` zips beside it (`express-mongodb`, `django`, `php-mysql`,
      `static`) can drift from `src/blueprints/starters/` with no reminder. Compare each published
      `<name>-starter.zip` with a zip of the tree, as the publish run's own verify step already
      does.
- [ ] **T205c** **(P)** Ctrl+C in a local Terminal tab did not stop a running command in
      PowerShell, while Git Bash in the same window did: found by hand on 2026-10-08 with the
      window started from a background process with no console of its own. Started from a
      terminal it works, so a shell can inherit "ignore Ctrl+C" from however MixLab itself was
      launched. Make the PTY's child reset that (`SetConsoleCtrlHandler(NULL, FALSE)` or a
      process-creation flag) so the key works however MixLab started, and measure it from the
      Start menu, a shortcut and the tray.

**M43** On all three systems, `laravel` and `wordpress` applied from MixLab's Blueprints screen on a
home with no web server open in the browser on their own, and `nextjs` serves its page after one
click on *Run the required steps*.
