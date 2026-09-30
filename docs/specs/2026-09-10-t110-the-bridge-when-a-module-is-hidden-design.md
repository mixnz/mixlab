---
status: implemented
date: 2026-09-10
task: T110
---

# T110 — The bridge when a module is hidden

Roadmap task [T110](../roadmap/phase-13-profiles.md), on
[the desktop client design](2026-09-08-the-desktop-client-in-this-repository-design.md)'s D11 and on
[T108](2026-09-09-t108-a-module-visibility-setting-design.md) and
[T109](2026-09-10-t109-the-default-tab-follows-the-profile-design.md). 2026-09-10.

T108 gave the window a profile and T109 gave it a first tab. Both of them left the same hole, and
T108's "What this is not" names it twice: a person who turned the database client off can still
reach two things that want to open a database tab — the Services screen's *open* button, and a
`<old>://` URL — and neither of them knows the module is gone.

Today the second one is worse than doing nothing. `takeTabRequests` is handed all five ids on
purpose, so the request is drained and a tab is opened; the visibility effect then runs on the very
same commit's `tabs` and drops it again. **A handoff to a hidden module opens a tab that disappears
before it is drawn**, and nothing anywhere says why.

This task is the bridge. **No API method is added, nothing the daemon answers changes**, no module
is renamed and no stored value changes shape.

## What is already true

Written down so nothing below is built twice:

- **The Services screen's *open* button already goes through the tab-request queue.**
  `mixengine_database_open_in_<old>` builds a `Handoff`, keeps it, and calls `launch::request` with
  `TabRequest { module_id: "db" }` — the same queue `<old>://connect` pushes into from
  `handoff::accept`. There is one door into a database tab, not two, and the enabling therefore
  belongs at that door rather than at either caller.
- **`database.client` already distinguishes this window from a client that is not it.** T107:
  `Installed { extension: None }` is MixEngine's own window, and the daemon answers it only when
  there is no `desktop-app` extension or when the installed one's scheme is the window's own. So
  `Installed { extension: Some(id) }` is, by construction, an application that is not this process.
- **`database.open` exists, is dispatched, and puts the password in the launched client's
  environment** — T83. The desktop application has no command for it, because until this task it
  had no reason to want one: inside the window, *open* is in-process (the desktop client design's
  D10).
- **The confirmation and the closing are already separated.** T108's D6: the Settings pane asks
  before it changes the setting, and the closing is one effect on the visible id list in
  `Workspace`, deliberately so that a second way to change that list cannot skip it. This task is
  that second way, and it needs no third rule.
- **A module may import from `src/shell/`; the shared layer may not import from a module.** The
  eslint boundary in `apps/desktop/eslint.config.js` is one-directional, and the mixengine module's
  own backend already names `"db"` in `open_in_<old>.rs`.
- **`apps/desktop` has no DOM test environment.** 141 test files, all of them over pure functions.
  Anything this task wants proved has to be a function that takes values and returns one.

## D1 — The enabling happens at the queue, once, for any module

`Workspace`'s drain gains one step. Before a tab is opened for a request whose module is hidden,
the module is turned on:

```ts
async function drain() {
  const requests = await takeTabRequests(ids).catch(() => []);
  if (requests.length === 0) return;

  // One union across the whole batch, then one call: two requests for two hidden modules in the
  // same drain would otherwise each compute their next set from the same stale `enabled`, and the
  // second would undo the first.
  const before = enabledRef.current;
  let next = before;
  for (const request of requests) next = withModule(next, request.moduleId, ids);
  if (next !== before) onEnabledChange(next);

  for (const request of requests) {
    const tab = openTab(request.moduleId, request.state);
    // Hidden when the request arrived — which is the whole of what the notice says.
    if (!before.includes(request.moduleId)) noteEnabledFor(tab.id, request.moduleId);
  }
}
```

**Generic over module ids, not written about `db`.** Only the database module ever pushes a request
today — the two callers above are the whole of it — but a rule enforced for one id is a rule the
next id breaks, and `Workspace` is a file whose whole discipline is that it reads the registry
rather than naming its entries.

**The setting is changed before the tab is opened, in the same synchronous block.** That is not
stylistic. `enabled` is `App`'s state and `tabs` is `Workspace`'s, so these are two components'
updaters; React 18 and later batch every update made in one tick — including after an `await` —
into one commit, so `visibleIds` and `tabs` reach the visibility effect together and the new tab is
never a tab of a hidden module. In the reverse order the two would still land in one commit, but
the code would read as if the drop were being raced, and the next person to add an `await` between
them would be right to be confused.

**`enabledRef`, because the drain listens once.** The effect that registers `onTabRequest` has an
empty dependency list on purpose — listening once is the point — so the `enabled` array in its
closure is the one from mount. A ref written on every render is the only thing in that closure that
can be current. `onEnabledChange` needs no such care: it is `useStartupProfile`'s `setEnabled`, a
`useState` setter, stable for the life of the app.

### `withModule`

```ts
/** `enabled` with `moduleId` in it, or `enabled` itself when it is already there or is not a
 *  module this build has. Returning the same array is how the caller knows nothing changed. */
export function withModule(enabled: string[], moduleId: string, knownIds: string[]): string[];
```

In `shell/profiles.ts`, beside `normalizeModules`, and pure like everything else in that file. It
appends rather than inserting in registry order, because **order never comes out of the stored
value** — T108's D1 — and `visibleModules` is a filter over `MODULES` regardless of what the set
happens to carry.

## D2 — Enabled is enabled, and the tab is what says so

T110's sentence is "enables it for that tab and says so in the tab", and the tempting reading is a
visibility that lasts as long as the tab does. It is the wrong one, and the reason is not effort:

A per-tab visibility would be a second notion of *enabled* living beside the stored one, and the
two would immediately disagree in front of the user. The tab strip would draw a Database tab while
the `[+]` menu had no Database entry; `Ctrl/Cmd+1 … N` would count a module the Settings pane's
checkbox said was off; `readSession` would drop on the next launch a tab that was open when the
window closed. Three parts of one window saying three things about the same question is not a
scoped setting, it is a bug with a rationale.

**So the module is turned on in the setting, written down like every other change to it**, and what
belongs to the tab is the *reason*: a line saying that this happened. That line earns its place
twice over, because turning a module on is not only a tab —

- the `[+]` menu grows an entry and may stop being a button that opens a tab outright,
- **`Ctrl/Cmd+1 … N` are renumbered**, since the chords index into the visible list,
- the Settings dialog grows that module's pane.

A person who deliberately turned the database client off and then clicked a link is owed the
sentence. Turning it off again is the Settings pane, which already asks before it closes anything.

**The notice does not survive a relaunch.** It is `Workspace` state keyed by tab id, not something
written into the session: the session's per-tab slot belongs to the module (`shell/module.ts`), and
by the next launch the sentence would be false anyway — the module is simply enabled, and was when
the session was written.

## D3 — The notice

A new shell component, `shell/components/TabNotice/`, drawn at the top of the panel of the tab it
belongs to:

> **Database** was turned on so this tab could open. Turn it off again in Settings → Modules.

`role="status"` and not `role="alert"`: nothing failed, and nothing is interrupted. Dismissible,
with no timer — `ErrorBanner`'s sixty seconds are there because an error stops being news; this is
a fact about the window that stays true until the person acts on it.

**It is drawn by the shell**, from the module's own `labelKey`, and no module learns anything. The
one place the shell already renders per tab is the panel, so that is where it goes.

**The wrappers are always in the tree and are boxes only when there is a notice.** Two constraints
pull against each other, and one declaration satisfies both.

They have to be *always there*, because React tells one element from another by its position: a
pane that gained a wrapper when the notice appeared, or lost one when it was dismissed, would be
unmounted and mounted again — and for a database tab that means dropping the connection it was
holding. Dismissing a line of text must not close a connection.

They have to be *nothing* when there is no notice, because `.tab-panel` is a flex container in row
direction whose single child is a module root sized `width: 100%; height: 100%`; two real boxes
between them would change every module's layout for a line that is almost never there.

```tsx
<div
  className={notice ? "tab-panel tab-panel-noticed" : "tab-panel"}
  style={{ display: tab.id === activeId ? "flex" : "none" }}
>
  <div className="tab-panel-stack">
    {notice && <TabNotice … />}
    <div className="tab-panel-body">{pane}</div>
  </div>
</div>
```

`display: contents` on `.tab-panel-stack` and `.tab-panel-body` is both things at once — the
element keeps its place in the React tree and produces no box at all, so the module root is the
flex item of `.tab-panel` exactly as it has always been. `.tab-panel-noticed` then turns the pair
into a column and a body that fills what the notice leaves.

## D4 — The Services screen offers rather than acts

`DatabasePanel` draws its *open* affordance from `database.client` exactly as it does today. What
changes is that it also knows whether the built-in client is one of this window's modules, and
draws a different set of controls when it is not.

### What the shell tells a module

`ModuleTabProps` gains one member:

```ts
/** Whether this window is drawing the module with this id.
 *
 *  For a screen that offers a way *into* another module and must not offer it as though it were
 *  already there — T110. It is a question about what is drawn, not about what exists: every
 *  module's backend commands are registered in every build (T108's D8). */
isModuleVisible: (moduleId: string) => boolean;
```

A predicate rather than the visible id list, because a module has no business enumerating the
others, and because a predicate is what the one caller wants. `Workspace` passes
`(id) => visibleIds.includes(id)`; `MixEngineTab` hands it to `ServicesDetail`, which hands it to
`DatabasePanel` — two props through two components, which is what the codebase does everywhere
else. No context: `shell/module.ts` says out loud that there is no event bus between modules, and
ambient shell state read from anywhere inside a module is the first half of one.

The id itself is a named constant in the mixengine module, beside the one its own backend already
hardcodes in `open_in_<old>.rs`:

```ts
/** The module a database service opens into. The one id this module names, and the frontend half
 *  of `open_in_<old>.rs`'s `module_id: "db"`. */
const DATABASE_MODULE_ID = "db";
```

### What is drawn

One pure function, in the mixengine module beside the panel:

```ts
export type OpenChoice = "builtIn" | "builtInAfterEnabling" | "external";

/** The *open* controls for a service, given what the daemon says opens it and whether this window
 *  is drawing the built-in database client. Empty is a hint, not a button — as today. */
export function openChoices(client: DesktopClient, builtInVisible: boolean): OpenChoice[];
```

| `client.state` | built-in visible | choices |
| --- | --- | --- |
| `installed` | yes | `["builtIn"]` — today's single *Open in MixLab* |
| `installed`, `extension` absent | no | `["builtInAfterEnabling"]` |
| `installed`, `extension` present | no | `["builtInAfterEnabling", "external"]` |
| `not_installed` | either | `[]` — the sentence about where it looked, as today |
| `no_client` | either | `[]` — the sentence about no client, as today |

**`external` only when the daemon named an application that is not this window.** `extension:
Some(id)` is exactly that (see *What is already true*), and offering *use an external client* while
the only client the machine has is the window the person is looking at would be a button that opens
a second copy of MixLab to forward a URL back to this one.

**Nothing else moves.** With the built-in client visible the panel is what it was, down to the
label. With the report in either of its two non-installed states there is still no button — the
gap that leaves is D6's, and it is not this task's.

### What the two controls do

- **`builtInAfterEnabling`** calls `databaseOpenInthe standalone client`, unchanged. The module is turned on by
  D1, at the queue, because that is where the request lands. The button's label says what it is
  about to do; it does not do it itself.
- **`external`** calls a new command over `database.open`, and the password never enters this
  process: the daemon reads it and puts it in the launched client's environment (T83).

```rust
/// `database.open` — hand this service to the desktop client MixEngine found, which is not this
/// window (see the design's D4). A passthrough: `params` is `DatabaseOpen { service, database? }`.
#[tauri::command]
pub async fn mixengine_database_open(params: Value) -> Result<Value, AppError>;
```

Beside `mixengine_database_client` in `modules/mixengine/commands.rs`, registered in
`modules::handler()`, wrapped in `api.ts` like its neighbours. `user` is left out, so the daemon
signs in as the server's administrator — the same account the in-process path resolves through
`database.client`'s `secret`.

## D5 — Both halves consented to, and both halves said out loud

The two entrances end up behaving differently on purpose:

| | asks first | says so afterwards |
| --- | --- | --- |
| the *open* button, `db` hidden | yes — two controls instead of one | yes |
| a `<old>://` URL, `db` hidden | no — there is nobody to ask | yes |

The button asks because there is a person standing in front of it who turned the module off. The URL
does not, because a URL arrives at a window that may not even be in front, and the alternative — a
modal asking permission for a connection MixEngine handed over on the command line — would break
`mix database open` for the person who ran it.

The notice appears on both, and the redundancy on the button path is deliberate: the person agreed
to open a tab, not necessarily to have `Ctrl/Cmd+1` mean something else afterwards.

**A `<old>://` URL is registered with the OS, so a web page can send one.** That was already true
before this task, and such a URL already opened a database tab; what is new is that it can turn a
module's visibility on. This is visibility and not capability — T108's D8 — every module's commands
are registered in every build and a hidden module's files are on disk either way. The notice is what
keeps the change from being silent, and the Settings pane is one click from undoing it.

## D6 — What this is not

**It does not fix `no_client` while the client is in the same process.** `database.client` answers
`no_client` when there is no `desktop-app` extension and the daemon could not locate the window —
a development build, or an install layout `locate_window` does not know — and the Services screen
then draws no *open* button at all, even though the built-in client is the process drawing the
screen. That is T83's and T107's, it predates this task, and T110 says the affordance is drawn from
`database.client` "as today". Fixing it here would mean the button appearing in states where it
does not today, which is a change to a different task's rule.

**It does not make the external path the one inside the window.** The desktop client design's D10
stands: with the built-in client visible, *open* is in-process and never through the OS, whatever
`database.client` says is installed.

**It adds no per-module capability gate.** D2 above and T108's D8: a hidden module is not drawn, and
that is the whole claim.

**It redesigns no window.** Two controls where there was one, and one line at the top of a tab.

## Files

```
apps/desktop/src/shell/profiles.ts                     withModule
apps/desktop/src/shell/profiles.test.ts                its cases
apps/desktop/src/shell/module.ts                       isModuleVisible on ModuleTabProps
apps/desktop/src/shell/Workspace.tsx                   the drain, the notice state, the wrapper
apps/desktop/src/shell/App.css                         .tab-panel-stack, .tab-panel-body
apps/desktop/src/shell/components/TabNotice/           the line, its styles, its index
apps/desktop/src/i18n/en.ts, vi.ts                     profiles.turnedOn, .turnedOnDismiss
apps/desktop/src/modules/mixengine/MixEngineTab.tsx    passes it to ServicesDetail
apps/desktop/src/modules/mixengine/screens/ServicesDetail/ServicesDetail.tsx
apps/desktop/src/modules/mixengine/screens/ServicesDetail/openChoices.ts
apps/desktop/src/modules/mixengine/screens/ServicesDetail/openChoices.test.ts
apps/desktop/src/modules/mixengine/screens/ServicesDetail/DatabasePanel.tsx
apps/desktop/src/modules/mixengine/api.ts              databaseOpen
apps/desktop/src/modules/mixengine/i18n/en.ts, vi.ts   the three strings
apps/desktop/src-tauri/src/modules/mixengine/commands.rs   mixengine_database_open
apps/desktop/src-tauri/src/modules/mod.rs                  its registration
```

The four other modules' `Tab` components are not in the list and need no edit: they destructure the
members of `ModuleTabProps` they use, so a new one costs them nothing.

Nothing in `crates/`, nothing in `bindings/`: no type on the wire changes, so
`packaging/bindings.sh` is not run.

## Verification

Pure logic, in vitest:

- `withModule` — an id already in the set returns the very same array; an id this build does not
  have returns the very same array; a new id is appended; the input is never mutated.
- `openChoices` — every row of D4's table, including that `installed` with an extension and a
  visible built-in client is still one choice and not two, and that both non-installed states are
  empty whether the built-in client is visible or not.

In Rust: nothing. `mixengine_database_open` is a passthrough to `rpc::call` with no decision in it,
exactly like `mixengine_database_client` beside it, which has no test either.

Neither says anything about CSS, about whether the command is registered, or about React's
batching. `npm run dev:app` and a click are what prove those, on the four paths worth walking by
hand:

1. *Everything*, a database service, *open* — one button, a tab, no notice. Unchanged.
2. *MixEngine*, a database service, *open* — one button that says it will turn the client on;
   a Database tab, the notice at the top of it, `Ctrl/Cmd+2` now a Database tab, the module
   ticked in Settings.
3. *MixEngine*, `mix database open mariadb@main` from a terminal — the window comes forward with
   the same tab and the same notice, and nothing was asked.
4. The notice dismissed, the tab closed, Settings → Modules → the database client off — one
   confirmation, and the window is back where it started with every saved connection intact.
