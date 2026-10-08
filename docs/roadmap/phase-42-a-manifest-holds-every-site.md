# Phase 42 — A manifest holds every site

*Goal: `mixengine.toml` describes every site of a project, so exporting one loses nothing and
adopting a colleague's checkout gets all of it.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-08-t204-a-manifest-holds-every-site-design.md](../specs/2026-10-08-t204-a-manifest-holds-every-site-design.md).

---

- [x] **T204** `[[sites]]` beside `[site]`, each entry with its own `services`; `project.export`
      writes every site and reports the entries it kept; `site.create` gains `from`; `project.show`
      lists what the file declares and which of it is here; MixLab's project panel adopts a
      missing site with one button.
- [ ] **T204a** Blueprints describe several sites: `schema = 3`, one `create_site` step per entry
      with its own resume identity, capture stops refusing, and `mixengine-packages` re-publishes.
      Built after T204 and T205. On T205's rule (its ADR) a blueprint is rendered at the lowest
      schema that holds it, so only a blueprint with more than one site is written at 3. T204a
      decides what `[[next_steps]] site` names (the `[[sites]]` entry, probably), requires it when
      there are several sites, and ties each `serve` step to one site. A migration it needs is
      numbered after T205's `0003`.

**M42** A project with three sites of three kinds is exported, deleted and re-created from the same
directory, and each site comes back with its domains, kind, routes and services, from `mix` and from
MixLab.
