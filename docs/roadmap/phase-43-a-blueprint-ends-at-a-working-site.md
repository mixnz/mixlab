# Phase 43 — A blueprint ends at a working site

*Goal: after applying a blueprint in MixLab, the next thing a person sees is their website, or the
exact commands that bring it up, one click from running in MixLab's Terminal.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md](../specs/2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md).

---

- [ ] **T205** **(P)** `[scaffold] archive` (schema 2, ADR 0061: a blueprint is written at the
      lowest schema that holds it) and WordPress built on it; `[[next_steps]]` with `once`, `serve`
      and `open`, read through `projects.blueprint_id` and shown by `AfterApply`, the Sites and
      Projects screens, the welcome page and `mix`; every MixLab apply asks for a front end, ticks a
      trusted scaffold and opens the browser when no step is needed; the Terminal gains `env` and
      `pathPrepend` for local targets, `onRestore`, and one-shot and draft tabs from MixEngine; five
      PHP blueprints (`cakephp`, `codeigniter`, `craft`, `statamic`, `yii`) and steps for the whole
      gallery; `mixengine-packages` re-publishes.

**M43** On all three systems, `laravel` and `wordpress` applied from MixLab's Blueprints screen on a
home with no web server open in the browser on their own, and `nextjs` serves its page after one
click on *Run the required steps*.
