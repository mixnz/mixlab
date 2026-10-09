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
- [x] **T204a** Blueprints describe several sites: `[[sites]]` at `schema = 3`, each entry with
      its own `services`; one create/names/certificate group per site, resumed by any of its
      names; capture writes every site and refuses only two PHPs; `[[next_steps]] site` names an
      entry's `domain_pattern` and is required with several sites; `mix` and MixLab group the plan
      and the steps by site. A site with no list links every service the plan made sure of, which
      fixes a shared instance left unlinked. No gallery file changes, so `mixengine-packages` has
      nothing to re-publish (decided 2026-10-09).
      Design: [2026-10-09-t204a-a-blueprint-holds-many-sites-design.md](../specs/2026-10-09-t204a-a-blueprint-holds-many-sites-design.md).

**M42** A project with three sites of three kinds is exported, deleted and re-created from the same
directory, and each site comes back with its domains, kind, routes and services, from `mix` and from
MixLab.
