# MixEngine documentation

Everything written for people lives in this folder, for two audiences, and the two parts never
restate each other:

- **[guide/](guide/)** is for whoever *uses* MixEngine: the handbook, sixteen pages in English and
  Vietnamese, published at `https://mixnz.github.io/mixlab/` and compiled into `mix docs` (T90,
  [ADR 0021](decisions/0021-the-handbook-is-one-corpus-published-three-ways.md)).
- **Everything else** is for whoever *builds* it: why MixEngine is shaped the way it is, and how
  to change it. [CLAUDE.md](../CLAUDE.md) is the short entry point; this is the detail it keeps out.

`.claude/` holds only the agent tooling's configuration
([ADR 0043](decisions/0043-documentation-lives-under-docs.md)). `node scripts/check-docs.mjs`
checks every link into this tree and every spec's status header, and CI's `lint` job runs it.

## Folders

| Folder | Answers | Read it when |
| --- | --- | --- |
| [architecture/](architecture/) | *How the system is put together* | Adding a subsystem, changing a boundary |
| [features/](features/) | *What each user-facing feature must do* — authoritative | Implementing or changing a feature |
| [specs/](specs/README.md) | *How one piece of work was designed, and whether it was built* | Picking up or questioning a task |
| [decisions/](decisions/README.md) | *Why it is this way* | Questioning an existing choice |
| [standards/](standards/) | *How we write code here* | Writing anything |
| [operations/](operations/) | *How it gets built, packaged, shipped* | Touching CI, installers, runtime bundles |
| [roadmap/](roadmap/todo.md) | *What to build next, in order* | Picking up work |
| [reviews/](reviews/README.md) | *How good what is built actually is, at a date* | Reviewing the codebase, or checking whether a past finding was fixed |
| `plans/` | *Local implementation plans* — gitignored, never linked | — |

The desktop application under `apps/desktop/` (MixLab) keeps its part of a folder in a `desktop/`
subfolder: `architecture/desktop/`, `standards/desktop/`, `decisions/desktop/`,
`roadmap/desktop/` and `reviews/desktop/`. [apps/desktop/CLAUDE.md](../apps/desktop/CLAUDE.md) is
its short entry point.

A new top-level folder is created once three documents fit nowhere above.

## Reading order for a newcomer

1. [architecture/overview.md](architecture/overview.md) — the whole system on one page
2. [architecture/daemon-and-ipc.md](architecture/daemon-and-ipc.md) — how clients talk to the core
3. [architecture/data-model.md](architecture/data-model.md) — the nouns and their relationships
4. [roadmap/todo.md](roadmap/todo.md) — where we are (index over the per-phase files)
5. [specs/README.md](specs/README.md) — every design, oldest first, with its status

## Index

### architecture
- [overview.md](architecture/overview.md) — layers, processes, on-disk layout, request lifecycle
- [daemon-and-ipc.md](architecture/daemon-and-ipc.md) — transport, JSON-RPC surface, event stream
- [process-supervision.md](architecture/process-supervision.md) — service specs, restarts, health, logs
- [platform-abstraction.md](architecture/platform-abstraction.md) — the OS traits and their impls
- [data-model.md](architecture/data-model.md) — SQLite schema, config files, state ownership
- [security-model.md](architecture/security-model.md) — privilege split, authn, secrets, threat notes
- desktop: [overview.md](architecture/desktop/overview.md) — process model, connection lifecycle ·
  [frontend.md](architecture/desktop/frontend.md) · [backend.md](architecture/desktop/backend.md)

### features
- [runtime-versions.md](features/runtime-versions.md) — multi-version PHP/Node/Python/Ruby
- [services.md](features/services.md) — web servers, databases, caches
- [domains-and-dns.md](features/domains-and-dns.md) — `.test` domains, hosts file, internal DNS
- [tls.md](features/tls.md) — internal CA, per-site certs, trust store, renewal
- [client-surface.md](features/client-surface.md) — what a graphical client must be able to ask for
- [lan-sharing.md](features/lan-sharing.md) — access from phones/tablets on the same Wi‑Fi
- [blueprints.md](features/blueprints.md) — capture and clone an environment
- [extensions.md](features/extensions.md) — plugin model, registry, opening a database in MixLab
- [resource-isolation.md](features/resource-isolation.md) — lightweight limits, on-demand start
- [updates.md](features/updates.md) — auto-update from GitHub Releases without OS code signing

### specs
- [specs/README.md](specs/README.md) — generated from each spec's header: date, task, status

### standards
- [rust.md](standards/rust.md) · [testing.md](standards/testing.md) ·
  [git-and-reviews.md](standards/git-and-reviews.md) ·
  [plans-and-specs.md](standards/plans-and-specs.md) — where specs and plans go, and what a spec's
  status means · [changelog.md](standards/changelog.md) — one tag per section, in order, three
  subheadings
- desktop: [adding-a-command.md](standards/desktop/adding-a-command.md) ·
  [adding-a-module.md](standards/desktop/adding-a-module.md) ·
  [app-icon.md](standards/desktop/app-icon.md) ·
  [bumping-tool-downloads.md](standards/desktop/bumping-tool-downloads.md) ·
  [changelog.md](standards/desktop/changelog.md) ·
  [component-structure.md](standards/desktop/component-structure.md) ·
  [css-modules.md](standards/desktop/css-modules.md) ·
  [demo-screenshots.md](standards/desktop/demo-screenshots.md) ·
  [filter-bar.md](standards/desktop/filter-bar.md) · [i18n.md](standards/desktop/i18n.md) ·
  [icons.md](standards/desktop/icons.md) ·
  [spawning-processes.md](standards/desktop/spawning-processes.md) ·
  [workspace-root.md](standards/desktop/workspace-root.md)

### operations
- [build-and-release.md](operations/build-and-release.md) ·
  [runtime-packaging.md](operations/runtime-packaging.md) ·
  [releasing.md](operations/releasing.md) — cutting a release, step by step

### decisions
- [decisions/README.md](decisions/README.md) — ADR index and template; MixLab's four dated
  decisions from the standalone client are indexed there too

### roadmap
- [todo.md](roadmap/todo.md) — the index: phases, task ranges, milestones, where we are
- one file per phase: [0](roadmap/phase-0-foundations.md) ·
  [1](roadmap/phase-1-process-supervision.md) · [2](roadmap/phase-2-runtimes.md) ·
  [3](roadmap/phase-3-services.md) · [4](roadmap/phase-4-sites-and-elevation.md) ·
  [5](roadmap/phase-5-https.md) · [7](roadmap/phase-7-efficiency.md) ·
  [8](roadmap/phase-8-differentiators.md) · [9](roadmap/phase-9-ship.md) ·
  [10](roadmap/phase-10-client-surface.md) · [11](roadmap/phase-11-the-desktop-app-comes-home.md) ·
  [12](roadmap/phase-12-one-product.md) · [13](roadmap/phase-13-profiles.md) ·
  [14](roadmap/phase-14-a-window-a-new-user-can-start-from.md) ·
  [15](roadmap/phase-15-what-a-terminal-inherits.md) ·
  [16](roadmap/phase-16-one-site-many-backends.md) ·
  [17](roadmap/phase-17-a-disk-somebody-chose.md) ·
  [18](roadmap/phase-18-what-a-machine-lacks.md) · [19](roadmap/phase-19-mongodb.md) ·
  [20](roadmap/phase-20-mixlab-redesigned.md) · [21](roadmap/phase-21-a-site-that-stays-up.md) ·
  [22](roadmap/phase-22-mixengine-in-the-tray.md) ·
  [23](roadmap/phase-23-one-home-for-the-documentation.md) ·
  [24](roadmap/phase-24-a-test-job-that-scales.md) · [parked.md](roadmap/parked.md)
  (phase 6 is a gap, not a missing file — see todo.md)
- desktop: [mixengine-module.md](roadmap/desktop/mixengine-module.md) ·
  [query-editor.md](roadmap/desktop/query-editor.md)

### reviews
- [reviews/README.md](reviews/README.md) — conventions (`R<n>` ids, status legend) and the index
- [2026-08-27.md](reviews/2026-08-27.md) · [2026-08-29.md](reviews/2026-08-29.md)
- desktop: [README.md](reviews/desktop/README.md) · [2026-08-27.md](reviews/desktop/2026-08-27.md)
  — ids are their own namespace, cited as "desktop R3"
