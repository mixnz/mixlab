---
status: implemented
date: 2026-09-17
---

# Marketing screenshots — one command, sample data, survives a redesign

2026-09-17.

**The case this comes from**: every large change to MixLab's look leaves the product's promotional
images out of date, and retaking them by hand means a running daemon, installed runtimes, a
database full of believable rows and an afternoon of arranging windows. What is wanted is a script
that, run after any such change, produces the same set of images from the same sample data — and
keeps working when the interface under it is redrawn.

## What is already true

- **Every call the frontend makes crosses Tauri IPC.** `invoke`, `Channel`, `listen`, and the
  `store`, `log`, `dialog`, `opener`, `clipboard-manager` and `app` plugins all go through
  `window.__TAURI_INTERNALS__`. `@tauri-apps/api/mocks` (2.11.1, already installed) replaces that
  object: `mockIPC(handler, { shouldMockEvents: true })` answers commands, emits events, and keeps
  the callback registry a `Channel` delivers through.
- **Every module restores its own screen from the session.** `shell/session.ts` reads
  `<old>-session` from `localStorage` — a list of tabs, each with an opaque `state` slot — and each
  module parses its slot:

  | Module      | Slot                                                   | Parsed by                  |
  | ----------- | ------------------------------------------------------ | -------------------------- |
  | `mixengine` | `{ screen: "dashboard" \| "sites" \| … }`              | `parseMixEngineTabState`   |
  | `db`        | `{ savedId, connected: true }`                         | `parseDbTabState`          |
  | `rest`      | `{ openIds, activeId }`                                | `parseRestTabState`        |
  | `terminal`  | `{ kind: "local", shellName, cwd }`                    | `parseTerminalTabState`    |
  | `tools`     | `{ toolId }`                                           | `parseToolsTabState`       |

- **The rest of the window is `localStorage` too.** `<old>-modules` (the visible set; present means
  no first-run screen), `<old>-theme`, `<old>-accent`, `<old>-lang`.
- **The title bar is the OS's, not the webview's.** Tauri keeps native decorations; nothing in
  `src/` draws window controls. What the user agent does decide is `IS_MAC` in `core/platform.ts` —
  the `⌘` or `Ctrl` in every shortcut label, and which modifier the shortcuts listen for — and the
  `data-platform` attribute `shell/theme.ts` writes for platform-specific CSS.
- **Every error the app can see ends in one call.** `core/log.ts`'s `logError` sends
  `plugin:log|log` at level `5` (Error), and it is what `main.tsx` calls for a window `error` and an
  unhandled rejection, and what `ErrorBoundary.componentDidCatch` calls.
- **The MixEngine contract is typed** in `bindings/`, reached as `@mixengine/api`; the other modules
  type their commands in their own `types.ts`.
- **`npm run build` is `tsc -b && vite build`** over `tsconfig.json`, whose `include` is `src` only.
- **Nothing in the repository drives a browser today.** `playwright` appears only transitively in
  `package-lock.json`.

## Decisions

### D1 — The real frontend, in a browser, over mocked IPC

The images are of the actual `src/` tree rendered by Vite in Chromium, with `mockIPC` answering
every command from sample data. Not the Tauri window over a real daemon: that needs runtimes
installed, database servers seeded and `tauri-driver`, which does not exist on macOS, and it fails
for reasons that have nothing to do with the interface. Not clicking through the interface by
visible text: that is exactly what a redesign breaks.

Nothing under `src/` changes for this. The demo is a second entry point that installs the mock and
then imports `src/main.tsx`.

### D2 — Screens are reached through the session, not through clicks

Each scene is one fresh page load with `localStorage` cleared and then seeded with:

- `<old>-modules` = all five modules,
- `<old>-theme` = `dark` or `light`,
- `<old>-lang` = `en`,
- `<old>-session` = a single active tab carrying the scene's slot from the table above.

The app restores that tab the way it restores one after a relaunch. A scene therefore depends on a
module's session slot — a documented, parsed, unit-tested shape — rather than on anything drawn.

### D3 — At most one short `act` step, by shortcut or by role

Two scenes cannot be reached by state alone: restoring a REST tab does not send its request, and a
tool's input is never stored. A scene may declare an `act` function that runs after the scene is
ready; the scene then waits to be ready again before the image is taken. It may use, in order of preference:

1. a keyboard shortcut the module registers (sending a REST request),
2. text the **sample data** owns — a table named `orders` is the fixture's word, not the interface's,
3. an element by what it is (`textarea`, a role) when it is the only one of its kind on the screen.

Never interface copy, never a CSS class or a CSS Module's generated name. An `act` that does not
find its element fails the scene by name (D6).

### D4 — Sample data is typed against the contracts

Fixtures for the MixEngine module are typed with `@mixengine/api`; fixtures for `db`, `rest`,
`terminal` and `tools` with those modules' `types.ts`. `tsconfig.json` includes `demo/`, so
`npm run build` — locally and in CI's `desktop` job — fails the moment a contract changes shape
under the sample data.

The data describes one believable machine:

- **Project** `acme-shop`; **sites** `acme-shop.test`, `api.acme.test`, `blog.test`, all HTTPS.
- **Runtimes** PHP 8.3 and 8.4, Node 22, Python 3.13.
- **Services** MariaDB, PostgreSQL 17 and Redis running.
- **Metrics** a fixed, smooth curve — no randomness anywhere in the fixtures.
- **Database** a saved PostgreSQL connection `acme_shop` with `orders`, `customers`, `products`,
  `order_items`; the scene shows its connection form, the fixtures keep the tables for other captures.
- **REST** a saved `GET https://api.acme.test/v1/orders?status=paid` whose response is a `200` with
  a JSON body.
- **Terminal** `terminal_open` pushes a canned ANSI byte stream through its `Channel`: a prompt,
  `php artisan migrate`, `git log --oneline --graph`, in colour.
- **Tools** the JWT decoder, given a sample token by the `act` step.
- **Plugins** answer neutrally — empty stores except the files a scene needs, dialogs cancelled,
  opener and log no-ops, app version from `package.json`.

### D5 — The six scenes

| Id          | Module      | Slot / act                                        | Headline (English, in `scenes.mjs`)|
| ----------- | ----------- | ------------------------------------------------- | ---------------------------------- |
| `hero`      | `mixengine` | `{ screen: "dashboard" }`                         | Your whole local stack, one window |
| `sites`     | `mixengine` | `{ screen: "sites" }`                             | Sites and runtimes                 |
| `database`  | `db`        | `{ savedId, connected: false }`: the form         | Database                           |
| `rest`      | `rest`      | `{ openIds, activeId }`, act: the send shortcut   | REST                               |
| `terminal`  | `terminal`  | `{ kind: "local", shellName, cwd }`               | Terminal                           |
| `tools`     | `tools`     | `{ toolId: "jwt" }`, act: fill its `textarea`     | Tools                              |

Each rendered in `dark` and `light`, English only: twelve raw images and twelve framed ones.

### D6 — Ready means quiet, and anything unexpected fails the run

A scene is ready when all of these hold at once:

- the app has mounted into `demo.html`'s own `#root` — a cold Vite server can take seconds before
  the bundle runs, and a page doing nothing yet is perfectly quiet,
- the mock reports no IPC call in flight and none started for 800 ms, counting the page's network
  requests as activity too — a cold server transforms a lazily imported screen while the page makes
  no IPC call and no DOM change,
- `document.fonts.ready` has resolved,
- a `MutationObserver` has seen no change for 500 ms,
- two animation frames have passed.

Not by waiting for a selector — a selector is a claim about the interface. A scene not ready within
20 s fails, naming the commands still in flight.

A scene also fails on:

- a command the mock has no answer for (logged with its name and arguments — this is the message
  that says which fixture to add),
- a `plugin:log|log` call at level `5` — which covers a crashed `ErrorBoundary`, a window `error`
  and an unhandled rejection in one place (the mock records them in `window.__demo.errors`),
- a Playwright `pageerror`,
- an `act` step that throws.

`console.error` is printed with the scene's report but does not fail it: React's development build
writes warnings there that say nothing about whether the image is right, and a run that fails on
them is a run nobody trusts.

Every scene is attempted before the run exits `1`, so one run lists every broken scene.

### D7 — Images are deterministic

- **Time** is fixed with Playwright's `page.clock` at 2026-01-15 10:24 local time, so relative
  times, uptimes and chart axes do not move between runs.
- **Motion** is off: `reducedMotion: "reduce"`, plus an injected stylesheet setting
  `animation: none`, `transition: none` and `caret-color: transparent` on everything.
- **Size** is a 1440×900 viewport at `deviceScaleFactor: 2`: a raw image is 2880×1800.
- **Platform** defaults to macOS: the context's user agent is a macOS Chrome one, so shortcut
  labels read `⌘` and platform CSS applies. `--platform windows|linux` changes it. The raw image is
  the webview only — the title bar is drawn by the frame (D8).

### D8 — Framing is a page, not an image library

`demo/frame/frame.html` takes the raw image, the theme, the platform, the headline and the
description in its query string, and draws a gradient ground, a window with rounded corners and a
shadow — its title bar drawn here, with the three macOS lights for `mac` and
minimise/maximise/close glyphs on the right otherwise — the raw image as the window's content, and
the headline and description above it. Playwright screenshots it at 1600×1000 CSS pixels, scale 2
— a 3200×2000 image, in which the window's content is drawn near the raw image's own resolution. The copy
lives in `scenes.mjs`; changing it is an edit there. No dependency is added for compositing.

### D9 — Output is local, not committed

`apps/desktop/screenshots/out/raw/<scene>-<theme>.png` and
`apps/desktop/screenshots/out/framed/<scene>-<theme>.png`. `screenshots/out/` is gitignored; the
script, the fixtures and the frame are committed. The directory is emptied at the start of a full
run so a removed scene leaves no stale image behind.

### D10 — CI renders every scene without writing images

`npm run screenshots -- --check` does everything of D6 and writes nothing. CI's `desktop` job runs
it after `npm run lint`, installing Chromium with `npx playwright install --with-deps chromium`.
This is what makes "rerun it after a redesign and it works" a property held on every change rather
than discovered on the day images are needed.

## Layout

```
apps/desktop/
  demo/
    demo.html          the demo entry — served by Vite in dev, never an input of `vite build`
    main.ts            installs the mock, seeds nothing, imports ../src/main.tsx
    constants.json     the fixed moment and the ids a scene's slot points at — read by both sides
    ipc/dispatch.ts    the dispatcher: fixtures by command, in-flight count, unmocked and error record
    ipc/store.ts       an in-memory `plugin:store`, holding every JSON file the modules read
    ipc/install.ts     wires the dispatcher into `mockIPC`
    fixtures/          time.ts, plugins.ts, mixengine.ts, db.ts, rest.ts, terminal.ts, index.ts
    scenes.mjs         the six scenes: id, slot, act, headline, description — plain JS, capture reads it
    args.mjs           the command line and the per-scene `localStorage`, pure and unit-tested
    readiness.mjs      the quiet-window detector, pure and unit-tested
    capture.mjs        the Playwright script: raw, framed, --check, --scene, --theme, --platform
    frame/frame.html   the frame page
  screenshots/out/     generated, gitignored
```

`package.json` gains `playwright` as a devDependency and a `screenshots` script. `capture.mjs`
starts Vite itself through its JavaScript API on a free port, and closes it on the way out. When Chromium is not installed, the
script says to run `npx playwright install chromium` rather than failing with Playwright's trace.

The demo's own files import from `src/modules/*` for types, which the lint boundary allows: it
guards `src/components`, `src/core`, `src/icons`, `src/shell` and `src/i18n` only.

## Documentation

- `apps/desktop/CLAUDE.md` gains the `npm run screenshots` row in its command table.
- `docs/standards/desktop/demo-screenshots.md` explains running it, reading a failure, and the
  two things a new backend command reached by a scene needs: a fixture, typed.

## Out of scope

- Vietnamese images, and any accent other than the default.
- Scenes beyond the six; adding one is an entry in `scenes.mjs` and whatever fixtures it reaches.
- Comparing images against a committed baseline. `--check` asserts that every scene renders
  without error, not what it looks like.
- Publishing the images anywhere.
