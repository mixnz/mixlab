# Demo screenshots

`npm run screenshots` renders MixLab's promotional images from sample data: the real frontend in
Chromium, every IPC call answered by `apps/desktop/demo/fixtures/`. The design is
[the spec](../../specs/2026-09-17-marketing-screenshots-design.md).

```bash
npm run screenshots                                   # every scene, dark and light, macOS frame
npm run screenshots -- --scene database --theme dark  # one image
npm run screenshots -- --platform windows             # Windows title bar and Ctrl shortcuts
npm run screenshots -- --check                        # verify every scene, write nothing
```

The first run on a machine needs `npx playwright install chromium`. Images land in
`apps/desktop/screenshots/out/{raw,framed}/`, which is gitignored; a full run empties it first.

## Reading a failure

Each failed scene lists why:

| Line | Meaning | What to do |
| --- | --- | --- |
| `unmocked   <cmd> <args>` | The screen called a command no fixture answers | Add a handler for `<cmd>` — below |
| `app error  …` | The app logged an error: usually it crashed on a fixture's shape | Fix the fixture, not the app |
| `pageerror  …` | An exception escaped the page — an unanswered command rejects this way too | Read it; it is almost always a fixture |
| `failed     not ready after 20000 ms (…)` | An IPC call or a request never settled, or something redraws on a timer | Answer the listed command; stop the timer through its setting |
| `failed     locator…: Timeout` | A scene's `act` did not find its element | Update the `act` in `demo/scenes.mjs` |

`console` lines are printed for context and never fail a scene.

A scene is ready when the app has mounted into `demo.html`'s `#root` and IPC calls, network
requests and DOM mutations have all gone quiet. Network requests count because a cold Vite server
can spend seconds transforming a lazily imported screen while the page does nothing else.

## Adding a fixture for a new command

1. Find the call: `rg -n '"<cmd>"' apps/desktop/src`. The `invoke<T>` there names the type.
2. Add `"<cmd>": returns<T>(value)` to the module's file in `demo/fixtures/` — MixEngine types
   come from `@mixengine/api`, the others from `src/modules/<id>/types.ts`. A handler that needs
   its arguments is `(args): T => …`. A command that streams through a `Channel` gets it as an
   argument and calls its `onmessage` after returning.
3. A file a module reads through `@tauri-apps/plugin-store` is seeded in that fixture's
   `…Files` export, keyed by file name and then by store key.
4. Derive every time from `NOW` in `fixtures/time.ts`, never `Date.now()`, and never use
   `Math.random` — the same run must give the same image.

`npm run build` type-checks the fixtures against the contracts, so a reshaped type fails there.

## Adding or changing a scene

A scene in `demo/scenes.mjs` is a module id, the session slot that module restores, an optional
`act`, and the copy the frame prints. `demo/scenes.test.mjs` runs each slot through its module's
parser, so a change to how a tab restores fails `npm test` first. The MixEngine tab restores no
screen — it always opens on Dashboard — so its scenes carry no slot and reach another screen in
`act`, through the sidebar button's `data-screen`, which is the screen's id.

An `act` may press a shortcut the module registers, find text the fixtures own, or take the only
element of its kind. Never interface copy and never a CSS class: those are what a redesign changes.

## Clips

`npm run clips` films short flows the same way: the real frontend, the same fixtures, and a cursor
that glides to what it presses. Three clips today: `new-site` (the Sites screen's New site),
`quick-start` (the Dashboard's Quick start building the Laravel blueprint, then a browser opening
the site) and `pin-runtime` (the Projects screen pinning `legacy` to PHP 8.1). The design is
[the clips spec](../../specs/2026-10-05-demo-clips-design.md).

```bash
npm run clips                                  # every clip, dark and light
npm run clips -- --clip new-site --theme dark  # one clip
npm run clips -- --check                       # film every clip, encode nothing
```

A full run needs ffmpeg (`winget install Gyan.FFmpeg`, `brew install ffmpeg`,
`apt install ffmpeg`, or `FFMPEG=<path to the binary>`). Output lands in
`apps/desktop/screenshots/out/clips/`: `<clip>-<theme>.mp4` (H.264, 2080×1300, 24 fps) and the
`-start` / `-end` posters, each as PNG and as WebP (quality 80; a WebP over 150 KB is noted).

A clip is declared in `demo/clips.mjs`: a module, an optional `setup` that runs before filming,
and `steps` — `click`, `type`, `select` (opens, then picks the option whose text matches),
`waitFor` (until an element appears, for timing the app decides, such as a job finishing) and
`pause`. Steps aim only at `data-demo` hooks in `src/`; `clips.test.mjs` fails when one is
missing. A hook carried by every row alike is narrowed by the `data-demo-key` of the row around it,
a name the fixtures own: `[data-demo-key="legacy"] [data-demo="project-menu"]`. What a clip needs the fixtures to do differently goes in its `fixtures`, which the
fixtures read through `demo/fixtures/options.ts`: `sitesWithout` (domains to start without),
`fresh` (no project and no site, so Quick start is offered), `folder` (what the folder picker
answers) and `browser` (draw a browser window when MixLab opens a URL, `demo/browser/overlay.ts`,
showing the vendored `demo/browser/laravel-welcome.html`) and `runtimePins` (two projects, `blog`
and `legacy`, with PHP 8.4.26 and 8.1.34 installed and pins that `project.update` changes). A clip's `still` names an element
photographed once the film stops, written as `<clip>-<theme>-<suffix>.png`.

The Laravel apply (`demo/fixtures/laravelApply.ts`) mirrors the gallery's `laravel.toml`, which
its test reads; change the blueprint and the test says so. When Laravel redraws its welcome page,
refresh `laravel-welcome.html` by hand from a fresh `composer create-project laravel/laravel`.

| Line | Meaning | What to do |
| --- | --- | --- |
| `failed     step N, <step>: …` | That step did not find or could not use its element | Put the `data-demo` hook back on the element, or update the step |
| `failed     the last frame is the first` | The flow changed nothing on screen | A step is acting on the wrong element |
| `failed     X MB is over the 1.50 MB budget` | The encoded clip is too heavy for the website | Shorten the pauses, or the flow |
| `note       X MB, over 1.00 MB` | Within budget, but heavier than wanted | The same, when convenient |
