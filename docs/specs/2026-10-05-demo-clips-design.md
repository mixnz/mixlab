---
status: implemented
date: 2026-10-05
---

# Demo clips — the screenshot rig, recording a short flow as video

2026-10-05.

**The case this comes from**: the website's hero is getting a live demo of going from nothing to a
running Laravel site, shown two ways — clicked through in MixLab, or typed with `mix` — and its
Sites feature gets a short clip of declaring a site. The MixLab halves have to be recordings of the
real window, and they have to be as cheap to redo after a redesign as the screenshots already are:
one command, sample data, the same result every run. A hand-made screen recording would be out of
date the first time a screen changes.

Two clips: **`new-site`** (the Sites screen's New site flow, for the website's Sites tab) and
**`quick-start`** (the Dashboard's Quick start building a Laravel project and opening it in a
browser, for the hero). `new-site` is described first because it came first; `quick-start` reuses
everything it needs and adds what is in [The quick-start clip](#the-quick-start-clip).

## What is already true

- **`npm run screenshots` renders the real frontend from fixtures.** `demo/capture.mjs` starts Vite,
  opens `demo/demo.html` in Chromium, answers every IPC call from `demo/fixtures/`, waits for the
  scene to go quiet (`demo/readiness.mjs`), runs the scene's `act`, waits again, and screenshots.
  The design is [the screenshot spec](2026-09-17-marketing-screenshots-design.md); the rules are in
  [demo-screenshots.md](../standards/desktop/demo-screenshots.md).
- **Captures freeze motion.** `FREEZE_CSS` turns off every animation and transition so a still is
  stable. A clip wants the opposite.
- **Fixtures are stateless.** `mixengine_sites` always returns the same `SITES`, which already
  contain `blog.test` (owner `blog`, root `/Users/ada/Sites/blog`). Nothing answers
  `mixengine_site_create`.
- **The Sites screen creates a site through `SiteForm`.** `Sites.tsx` opens it from its primary
  button; the fields are `SiteFields` (project, domain, document root, kind, HTTPS, redirect,
  enabled). It calls `siteCreate` → `mixengine_site_create`.
- **Playwright's bundled ffmpeg only writes VP8 WebM**, at a bitrate that blurs interface text.
  There is no ffmpeg on the machines that run this today.

## What changes

### One command, beside `screenshots`

```bash
npm run clips                              # every clip, dark and light
npm run clips -- --clip new-site --theme dark
npm run clips -- --check                   # play every clip, write nothing
```

`demo/clip.mjs` is the entry point. What `capture.mjs` and it share — the free port, the Vite
server, `initScene`, launching Chromium, the warm-up, waiting for quiet, the report — moves into
`demo/rig.mjs`, and `capture.mjs` keeps only what is about stills (freezing, the frame page). The
screenshot output does not change; `npm run screenshots -- --check` passing before and after is
the test of the move.

### A clip is a scene with steps

`demo/clips.mjs`, plain JavaScript like `scenes.mjs`:

```js
export const CLIPS = [
  {
    id: "new-site",
    moduleId: "mixengine",
    tabTitle: "MixEngine",
    fixtures: { sitesWithout: ["blog.test"] },
    setup: async ({ page }) => { await page.locator('nav [data-screen="sites"]').click(); },
    steps: [
      { click: '[data-demo="new-site"]' },
      { select: '[data-demo="site-project"]', option: "blog" },
      { type: '[data-demo="site-domains"]', text: "blog.test" },
      { click: '[data-demo="site-https"]' },
      { click: '[data-demo="site-save"]' },
      { pause: 2500 },
    ],
  },
];
```

- `setup` runs before recording starts, then the rig waits for quiet. Only `steps` are filmed.
- Steps are `click`, `type` (one character every 70 ms, so the typing is visible), `select`
  (open, pause, choose — the way a person does it) and `pause`. Between steps the rig waits 500 ms
  and no more: Playwright already waits for what each step acts on, and waiting for the scene to
  go quiet put one to three seconds of frozen window on film per step.
- **Selectors follow the screenshot rule** — never interface copy, never a CSS class. The five
  elements above get a `data-demo` attribute in the app, inert in production. `Button` and
  `Textarea` already pass extra attributes through; `Select` (on its trigger), `SwitchTile` (on
  its label) and `ModalAction` (on its button) each gain an optional `demo?: string` prop that
  writes `data-demo`. That and the attributes themselves are the only change to `src/`. A
  `select` step opens the trigger, then clicks the `[role="option"]` whose text is exactly
  `option` — fixture text (`blog`), which the rule allows. The kind is not touched: a new site's
  draft is already `php-fpm`.
- `fixtures.sitesWithout` drops those domains from `SITES` for this clip, so the site the clip
  creates is not already in the list. The docs walk through `blog.test`, and the website's
  terminal demo types `blog.test`; the clip creating the same site is the point.

### Fixtures that remember one creation

`demo/fixtures/siteRegistry.ts` (pure, tested in node) holds the sites one page sees. It starts
from `SITES` minus `sitesWithout`, answers `mixengine_sites` (honouring its `project` filter) and
`mixengine_site`, and builds a `SiteCreation` for `mixengine_site_create` — `enabled`, the HTTPS
and redirect it was asked for, owner from the project, root from `PROJECTS`, doc root `""` when
none is given, and for a php-fpm site with no pool named the pool the daemon would resolve,
`php-fpm@8.3` (the one `SERVICES` runs for `blog`). A domain already declared is refused, as the
daemon does. `mixengine_project_show`, which the form calls when a project is chosen, gets a
fixture from `PROJECTS`. The registry lives in the page, so every clip and every screenshot
starts from the same state; existing scenes see exactly the sites and details they saw before.

### Recording

- Viewport 1440×900 at device scale 2, like the stills. **Animations stay on**: no `FREEZE_CSS`.
- The rig injects a cursor — an SVG arrow in a fixed-position element above everything, with
  `pointer-events: none` — after the scene is quiet, so it does not count as a mutation the
  readiness check waits on. Before each `click`/`select`/`type` it glides to the centre of the
  target over 450 ms (ease-in-out), then the action runs; a click shows a 300 ms press ring.
- Frames come from CDP `Page.startScreencast` (PNG, `maxWidth` 2880), each saved with its
  timestamp. The screencast only sends a frame when something repaints, so the timestamps are
  uneven. `resample()` picks, for each tick of a constant 24 fps, the last frame that had arrived,
  holding the last until recording stopped, and those PNGs are piped to ffmpeg (`image2pipe`).
  ffmpeg's concat demuxer was the first try: version 9 mis-times still images in it (a list of
  1 s + 2 s came out 2 s long), so timing stays in node, where it is tested.
- **Size is a budget, not a hope.** The website shows the clip at most 1040 CSS pixels wide, so the
  video is scaled to 2080×1300 — sharp on a 2× screen, and nothing larger is ever displayed.
  Encoding: `ffmpeg -vf scale=2080:1300:flags=lanczos -c:v libx264 -preset veryslow -crf 26
  -tune stillimage -pix_fmt yuv420p -movflags +faststart -an`. The run prints each file's size;
  **over 1 MB is a warning, over 1.5 MB fails the clip.** A ~12 s clip of a mostly still window is
  expected to land at 0.6–1 MB. CRF 26 rather than 24: the quick-start clip's browser scene moves
  every pixel, and at 24 it came out at 1.26 MB; interface text stays sharp at 26. The posters are
  PNG at the same 2080×1300.
- The rig looks for `ffmpeg` on `PATH` (or `FFMPEG` in the environment). If it is missing the run
  stops before launching anything and prints how to install it (`winget install Gyan.FFmpeg`,
  `brew install ffmpeg`, `apt install ffmpeg`) — the same shape as the missing-Chromium message.
  `--check` does not need ffmpeg.

### Output

`apps/desktop/screenshots/out/clips/`, gitignored with the rest of `out/`:

- `new-site-light.mp4`, `new-site-dark.mp4`
- `new-site-light-start.png`, `-end.png` and the dark pair — the first frame (the video's poster,
  so nothing jumps when it starts) and the last (what a reader with reduced motion sees).

Publishing is by hand, as with the stills: upload to the CDN's next folder.

### Failures

The same report as screenshots: `unmocked`, `app error`, `pageerror`, `not ready`, and a step that
does not find its element names the step's index and selector. A clip whose last frame is the same
as its first fails — the flow did nothing. A clip over the 1.5 MB budget fails with its size.

## The quick-start clip

**What is on film** (about 24 s):

1. The Dashboard of a machine with no site yet, so the **Build your first site** card
   (`QuickStart.tsx`, T117) is drawn.
2. The card opens on **Laravel**, the gallery's first stack, so nothing picks it; `blog` is typed as
   the project name, **Choose…** gives the folder
   `/Users/ada/Sites/blog`.
3. **Create it** opens `ApplyDialog` with the project and folder filled in; **Preview** runs the dry
   run and the plan is listed.
4. The scaffold consent is ticked and **Apply** runs the job: its progress moves and the log follows
   `composer create-project`, until the job succeeds and **Close** hands over to `AfterApply`.
5. `AfterApply` starts what the project needs and shows the site ready; **Open https://blog.test**
   is pressed.
6. A browser window slides up over MixLab, its address bar reading 🔒 `https://blog.test`, showing
   Laravel's welcome page.

**The plan is the real Laravel blueprint's.** The fixture plan and the steps it reports are built
from `crates/mixengine-core/src/blueprints/gallery/laravel.toml` as it stands — PHP 8.4, Node 24,
Composer 2, MariaDB `main` with a `blog` database and user, Redis `main`, the `redis` PHP extension,
a php-fpm site on `public/` with HTTPS at `blog.test`, and the `composer create-project
laravel/laravel . --no-interaction` scaffold — written out by hand in the fixture, with a test that
reads the manifest and fails when the two drift apart. The demo's `BLUEPRINTS` description of
Laravel, which says PostgreSQL, is corrected to the manifest's own.

**A fresh machine.** `fixtures.fresh: true` starts the site registry and the project list empty, so
the Dashboard offers Quick start; services, runtimes and the rest of the machine stay as they are.
After the apply, the registry holds `blog` and `blog.test` like any created site.

**The apply, answered in time.** New fixtures, all derived from `NOW` and timers inside the page,
never `Date.now()`:

- `mixengine_blueprint_apply`: a dry run answers `planned` with the plan above; the real run answers
  `started` with a job, then emits `job_progress` events on the watch channel (`mixengine_watch`'s
  `onEvent`) at a fixed cadence — one per plan step, 170 ms apart, as `apply.rs` words them (`position * 100 / total` with `steps::describe`, then 100
  when the scaffold ends) — and a `job_finished` whose
  result is the `BlueprintApplied` (every step `done`). The registry gets the project and site at
  that moment.
- `mixengine_job_status` answers the finished job; `mixengine_job_logs_watch` sends a short,
  fixed excerpt of `composer create-project` output, a line every 55 ms during the scaffold step — word for word what composer printed on a real run
  (PHP 8.4, laravel/laravel v13.10.1), only the path changed, stopping where the key is set. The
  plan has no front-end steps: the daemon plans them only when no front end is held, and the demo
  machine runs Caddy.
- What `AfterApply` asks — elevation status with nothing pending, `service.start` for the project,
  `site.list` — answers as a machine that needs no password.
- `plugin:dialog|open` answers the folder when, and only when, a clip has said which
  (`fixtures.folder`); everywhere else it still answers `null`.

**The browser scene.** `plugin:opener|open_url` keeps answering `null`, and also dispatches a
`demo:open-url` event in the page. `demo/browser/overlay.ts`, imported by `demo/main.ts`, listens
only when the clip asked for it (`fixtures.browser: true`) and draws a browser window — title bar,
address bar with a lock and the URL, a thin loading bar — that slides up over MixLab in 450 ms and
shows `demo/browser/laravel-welcome.html` in an iframe. The screenshots never set the flag, so they
never see it. The clip's last step is a pause on that window.

**Laravel's welcome page is Laravel's.** `demo/browser/laravel-welcome.html` is the welcome view of
the `laravel/laravel` skeleton as `composer create-project` installs it today (v13.10.1, framework
  v13.34.0; MIT, said in a comment at the top), rendered to static
HTML: Blade directives resolved, the Vite branch dropped for the inline-style fallback the view
already carries, no network request. It follows `prefers-color-scheme` as the original does, so the
dark clip shows its dark face. When Laravel redraws its welcome page, the file is refreshed by hand,
like any vendored asset.

**Hooks.** The same rule as `new-site`; the new `data-demo` hooks are on the Quick start card
(`qs-stack`, `qs-project`, `qs-folder`, `qs-create`), in `ApplyDialog` (`apply-preview`,
`apply-scaffold`, `apply-run`, `apply-close`) and on `AfterApply`'s open button (`open-site`).
`Checkbox` gains the same optional `demo` prop if it does not pass attributes through already.

**One more output.** Besides the MP4 and its posters, the clip writes
`quick-start-<theme>-browser.png`: the browser window alone, at 2×, for the website's Terminal tab,
whose story ends in the same browser.

**Budget.** The same 1 MB / 1.5 MB as every clip. If `quick-start` lands over 1 MB, the log pauses
and the job cadence are what shorten first.

## MixLab

This is tooling for MixLab's own promotional material: it films the Sites screen's **New site**
flow and the Dashboard's **Quick start**, and adds no screen. The only change to the window is
inert `data-demo` attributes on those screens, `SiteForm`, `SiteFields`, `ApplyDialog` and
`AfterApply`, and the optional `demo` prop on `Select`, `SwitchTile`, `ModalAction` (and
`Checkbox` if needed) that writes them.

## Testing

- `demo/clips.test.mjs`: every clip's module id exists, its steps are well-formed, its selectors
  are `[data-demo=…]` or `data-screen`, and `fixtures` names only domains in `SITES`.
- `demo/args.test.mjs` grows the `--clip` cases.
- A fixture test: after `mixengine_site_create`, `mixengine_sites` includes the new site once.
- Quick start fixtures: the dry run's plan matches `laravel.toml` (runtimes, services, site,
  scaffold); a fresh registry is empty and holds `blog.test` after the apply; the job's progress
  events end in a `job_finished` carrying every step `done`.
- `clips.test.mjs` also checks `quick-start`'s hooks exist and that `fixtures.fresh` is a boolean.
- `npm run screenshots -- --check` and `npm run clips -- --check` both pass.
- `npm run build` type-checks the new fixture against `SiteCreation`.
