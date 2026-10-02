# CLAUDE.md

Short orientation for agents working on MixLab, the product this repository ships. Details live in [docs/](../../docs/README.md) — read the
relevant file there before changing anything in that area.

## What this is

MixLab is a desktop app built with **Tauri 2 + React 19 + TypeScript** (frontend) and **Rust**
(backend). It is a **shell** — a tab bar, keyboard shortcuts and a Settings dialog — plus one
**module** per kind of thing a tab can hold:

- **`db`** — MySQL, PostgreSQL, SQLite, MongoDB, Redis and ClickHouse (read-only) connections,
  optionally through an SSH tunnel, with saved connections remembered.
- **`rest`** — an HTTP client: saved requests, environments, history, and a response pane.
- **`terminal`** — a shell on this machine or on a server over SSH, with saved hosts.
- **`tools`** — small utilities that run in this process.
- **`mixengine`** — the one module that talks to the MixEngine daemon, and an **optional** one.

**MixLab comes first; MixEngine is an add-on.** Many users never start the daemon, so the shell and
every module but `mixengine` must work with `mixengined` absent, stopped or never run — and nothing
outside that module may start it. That includes updates: MixLab has its own updater, in the shell's
Settings → Updates, not in the MixEngine tab
([ADR 0056](../../docs/decisions/0056-mixlab-stands-without-mixengine.md)).

The shell knows nothing about any of them. Adding a fourth is a folder under `src/modules/` and
a line in `src/shell/registry.ts` — see
[docs/standards/desktop/adding-a-module.md](../../docs/standards/desktop/adding-a-module.md).

## Commands

| Command | What it does |
| --- | --- |
| `npm install` | Install frontend dependencies |
| `npm run dev:app` | Stage the four headless binaries beside the window, built from the root workspace, then run the full desktop app (Vite + Rust, hot reload) — the normal dev loop. A dev daemon still running from the last window is stopped first (`mix daemon stop` against this checkout's home — `.mixengine-home`, or `MixEngine-dev` for a checkout on an external disk on macOS, ADR 0040). `node scripts/stage-daemon.mjs --stage-only` is the first half alone |
| `npm run dev` | Frontend only in a browser; every `invoke` fails, UI-only work |
| `npm run build` | Typecheck + build frontend (`tsc && vite build`) — the fastest check |
| `npm test` | Run the vitest suite (`vitest run`) |
| `npm run lint` | eslint: hook dependencies, and the rule that nothing outside `src/modules/` imports a module |
| `npm run build:app` | Full production bundle into `src-tauri/target/release/bundle/` |
| `npm run icons` | Rebuild `src-tauri/icons/` from the SVGs in `public/`; macOS gets the padded one, and its menu bar the tray template (T168) |
| `npm run screenshots` | The six promotional images — Dashboard, Sites, Database, REST, Terminal, Tools — from sample data, dark and light, raw and framed, into `screenshots/out/`. `-- --check` renders every scene and writes nothing (CI runs it). See [demo-screenshots.md](../../docs/standards/desktop/demo-screenshots.md) |

Releasing is the repository's — [build-and-release.md](../../docs/operations/build-and-release.md):
one release ships MixLab and the MixEngine binaries together, from one feed. The app icon, and why there are two logo files, is
[icons.md](../../docs/standards/desktop/app-icon.md).

There is no linter config. `npm run build` is the fastest verification step; TypeScript runs
`strict`, `noUnusedLocals` and `noUnusedParameters`, so it catches most mistakes. `npm test` runs
vitest over the pure-logic modules (virtual rows, SQL statement splitting and guards, column
parsing, request building, tab badges). None of them say anything about CSS or about whether a
Tauri command is registered — only `npm run dev:app` and a click do.

The root workflow's [`desktop` job](../../.github/workflows/_lint.yml) runs all of it on request and on
every tag, and its `build` legs are what prove the window links on all three platforms.

## Layout

```
src/                 React frontend
  main.tsx           Entry point
  tray.tsx           The tray panel's window (`tray.html`) — the shell's frame around modules' `TraySection`s (T168, T192)
  shell/             Tab bar, [+] menu, shortcuts, Settings — knows no module
    App.tsx          The gate: which modules this window draws, decided before anything is drawn
    Workspace.tsx    The tab bar itself, and everything that reads the visible module list
    module.ts        ModuleDefinition, ModuleTabProps, TabBadge — what a module is
    registry.ts      MODULES, MODULE_PRESETS — the only file outside modules/ that names one
    profiles.ts      enabledModules and visibleModules() — the setting behind the gate
    launch.ts        Tabs the backend asks for — the only other way a tab opens
    App.css          Tokens + chrome + the classes any module may use
  core/              Helpers no module owns and any module may use
  components/        Shared primitives only, one folder each
  icons/  i18n/      Shared icons; the shared dictionaries and dicts.ts, which merges them
  modules/db/        The database module
    DbTab.tsx        Connection form -> connects -> renders one workspace
    sql/             The workspace every SQL engine shares, and the SqlApi/SqlDialect behind it
    mysql/ postgres/ What each SQL engine says for itself: api.ts, dialect.ts, columns.ts
    sqlite/          The third, and the only one with no server behind it
    mongo/ redis/    The two with a workspace of their own: <Kind>Workspace.tsx, api.ts
    components/      This module's own components
    i18n/            This module's own strings
    db.css  types.ts Its global styles; the types mirroring the Rust models
  modules/rest/      The REST client
    RestTab.tsx      Sidebar of saved requests + the request being edited
    api.ts           The only invoke calls here: rest_send, rest_cancel, secrets_*
    *.ts  *.test.ts  The pure halves — building a request, interpolating, parsing a paste
    components/  i18n/  rest.css
  modules/terminal/  The terminal
    TerminalTab.tsx  Target form -> a session -> xterm
    api.ts           terminal_open/write/resize/close, the shell list, and the events Channel
    components/  i18n/  terminal.css
src-tauri/src/       Rust backend
  lib.rs             Tauri builder; reads the opening URL first; each module registers its own state
  error.rs  secrets.rs  ssh/    Shared by every module (the tunnel and open_shell both live in ssh/)
  launch.rs  instance.rs  A connection handed over on the command line, and the channel to a running copy
  tray.rs  login_item.rs  The tray icon, its menu and panel window, and MixLab's login entry (T168, T192)
  modules/
    mod.rs           handler() — every command of every module, one block each
    db/              commands/, drivers/, handoff.rs, models.rs, state.rs
    rest/            commands.rs, models.rs, state.rs
    terminal/        commands.rs, local.rs, remote.rs, stream.rs, models.rs, state.rs
```

## Rules that matter most

- **Frontend never touches a network or a disk.** No driver, no HTTP request, no shell: it calls
  `invoke(...)` through its module's `api.ts` and renders what comes back.
- **Nothing outside `src/modules/<id>/` knows that module's concepts.** No file in `shell/`,
  `core/`, `components/`, `icons/` or `i18n/` may import from `modules/`, with exactly two
  exceptions — `shell/registry.ts` and `i18n/dicts.ts`, the two places a module is joined to the
  app. `tsc` compiles a broken boundary happily, so `npm run lint` is what says no,
  in CI and locally — see [adding-a-module](../../docs/standards/desktop/adding-a-module.md).
- **Every user-visible string goes through `t("...")`**, added to both `en.ts` and `vi.ts`.
- **Components use CSS Modules** and live in their own folder — see
  [docs/standards/desktop/component-structure.md](../../docs/standards/desktop/component-structure.md).
- **The root element of a workspace needs `width: 100%`**, not just `height` — `.tab-panel` is a
  row-direction flex container, so a root without it is only as wide as its content and hugs the
  left edge. It comes as a block of five properties, `box-sizing` and the `min-*` pair included:
  [docs/standards/desktop/workspace-root.md](../../docs/standards/desktop/workspace-root.md).
- **A new backend command touches five places.** Follow
  [docs/standards/desktop/adding-a-command.md](../../docs/standards/desktop/adding-a-command.md).
- **Every `std::process::Command` goes through `crate::platform::hide_console`.** Without it
  Windows opens a black console window for the child, which flashes over the app and reads as
  malware to the user. See
  [docs/standards/desktop/spawning-processes.md](../../docs/standards/desktop/spawning-processes.md).
- **On macOS a launch never asks for the Keychain password twice.** The Keychain asks once per
  *item* for a build it does not recognise by signature — every dev rebuild, every update. So
  everything MixLab keeps under service `MixLab` goes into the one `vault` item in
  `src-tauri/src/secrets.rs`, never into an item beside it. Twice this has regressed (per-connection
  entries, then sync's `sync-master-key`); `sync_and_the_connections_are_one_visit_to_the_store` is
  the test that says so. Nothing on the launch path reads MixEngine's `mixengine` item either: a
  saved connection's `keyringRef` is resolved only in `connect` (`withResolvedPassword`), and the
  vault is written only when it changes.
- **This app is MixLab, and its old name appears nowhere but in the values that fix up a user's
  machine**: `import.rs`'s identifier and marker, `secrets.rs`'s `LEGACY_SERVICE`, the storage
  keys in `public/storage-keys.js`, `shell/themeModel.ts` and `modules/terminal/settings.ts`, and
  the installer's scheme cleanup. Not in an identifier, a comment, a log line or a doc.
- Commit messages need a `type(scope): message` prefix (see the global rules).
- **Never link to a file under `docs/plans/`.** Those are local-only implementation
  plans, gitignored and absent on every other machine. Link to `docs/specs/` instead —
  see [docs/standards/plans-and-specs.md](../../docs/standards/plans-and-specs.md).
- **`PG_VERSION` in `src-tauri/src/modules/db/drivers/tools.rs` expires every September.** `pg_dump` will not dump a
  server newer than itself, and nothing in the build says so. Bumping it, or any other pinned
  download, follows
  [docs/standards/desktop/bumping-tool-downloads.md](../../docs/standards/desktop/bumping-tool-downloads.md).
- **A change a user would notice gets a line in `## [Unreleased]`** in the repository's root
  [CHANGELOG.md](../../CHANGELOG.md), under `### Added`, `### Changed` or `### Fixed`, written as
  part of the work rather than at release time. Follow the root's
  [standards/changelog.md](../../docs/standards/changelog.md) and this application's own
  [conventions/changelog.md](../../docs/standards/desktop/changelog.md) — one short line each,
  and a fix to something still unreleased is not a `Fixed` entry. This directory's `CHANGELOG.md`
  is the standalone client's history, frozen at 0.0.33.

## Where to read more

- [docs/architecture/desktop/overview.md](../../docs/architecture/desktop/overview.md) — process model, connection lifecycle
- [docs/architecture/desktop/frontend.md](../../docs/architecture/desktop/frontend.md) — React structure and patterns
- [docs/architecture/desktop/backend.md](../../docs/architecture/desktop/backend.md) — Rust structure and patterns
- [docs/standards/desktop/](../../docs/standards/desktop/) — code conventions, and how changelog entries are written
