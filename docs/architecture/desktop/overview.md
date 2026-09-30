# Architecture overview

MixLab is a Tauri 2 desktop app: a React webview for the UI, a Rust process for everything that
touches a network or a disk.

```
React webview                              Rust process
─────────────                              ────────────
shell/App.tsx    profile gate              lib.rs           plugins, per-module state
shell/Workspace  tab bar, no module
  └─ registry ──► modules/db/DbTab.tsx     modules/mod.rs   handler(): every command
  │     └─ <db>/api.ts ──invoke─────────► modules/db/commands/<engine>.rs
  │                    ◄─JSON result────    └─► drivers/{mysql,postgres,mongo,redis}.rs ──► server
  ├─────────────► modules/rest/RestTab.tsx
  │     └─ rest/api.ts ──invoke─────────► modules/rest/commands.rs ──► reqwest ──► host
  └─────────────► modules/terminal/TerminalTab.tsx
        └─ terminal/api.ts ──invoke─────► modules/terminal/commands.rs
                     ◄─Channel bytes────    └─► local.rs (pty) / remote.rs ──► ssh/
                                          ssh/ — tunnel and shell, shared by db and terminal
```

The split is strict: **no driver, credential handling, HTTP request or shell exists in the
frontend**. The webview only ever sends a command name plus JSON arguments and renders what comes
back — bar the terminal, whose output arrives on a Tauri `Channel` because a shell talks first.

## The shell and its modules

The shell owns the tab bar, the keyboard shortcuts and the Settings dialog, and knows only what
[`shell/module.ts`](../../../apps/desktop/src/shell/module.ts) declares:

- `ModuleDefinition` — an id, an icon, a label, and the component a tab renders.
- `ModuleTabProps` — what that component is handed: `active`, `onTitleChange`, `onBadgesChange`,
  and the pair that lets a tab come back to what it had open, `restored` / `onStateChange`.
- `TabBadge` — a mark the module wants drawn on its own tab. The shell draws it without knowing
  what it means. The engine logo and the read-only lock are two of these; before the split they
  were `tab.kind` and `tab.readOnly`, which put `DbKind` into the tab bar.

The contract deliberately has **no lifecycle hooks and no event bus**. A module cleans up in its
own `useEffect` and saves through its own store, and a shell that guesses at needs nobody has yet
is harder to add to than the thing it replaced.

The one exception is a single **opaque slot per tab**: the module writes a value through
`onStateChange`, the shell stores it with the session and hands it back as `restored` next launch,
and nothing in `src/shell/` ever looks inside it. Ids only — the shape and the reasons are in
[the spec](../../specs/2026-08-23-tab-session-context-design.md).

[`shell/registry.ts`](../../../apps/desktop/src/shell/registry.ts) lists the modules and is the only file outside
`src/modules/` that names one.

## Tabs and connections

`shell/Workspace.tsx` owns a list of tabs — `{ id, moduleId, title, badges, state }`. Each renders
the `Tab` component its module supplies, and a tab that has been on screen is kept mounted (hidden
with `display: none`) so switching tabs never loses a connection or scroll position. `Ctrl/Cmd+T`
opens a tab of the default module, `Ctrl/Cmd+W` closes one, `Ctrl+Tab` and `Ctrl+Shift+Tab` move
along the strip; closing the last tab spawns a fresh one. With more than one module **visible**,
`[+]` opens a menu instead of a tab.

Which modules are visible is a setting, and `shell/App.tsx` is the gate that settles it before the
workspace mounts: `shell/profiles.ts` holds `enabledModules`, and everything above that enumerates
modules — the `[+]` menu, the number chords, the Settings panes, session restore — reads
`visibleModules()`. See
[the T108 design](../../specs/2026-09-09-t108-a-module-visibility-setting-design.md).

The strip survives a restart, and so does what each tab had open: `shell/session.ts` keeps
`{ id, moduleId, title, state }` per tab and which one was active in `localStorage`. `state` is the
module's own — a saved connection's id, the ids of the requests on a REST strip, a saved host's id
or the name of a local shell — and the shell neither reads nor validates it. On launch only the
tab that was active is mounted; the others are names on the strip that mount the first time they
are picked, so nothing reconnects until a tab is actually looked at.

A `DbTab` starts as a form. On connect it calls `connect_db` with a `ConnectionConfig`, gets back a
**connection id** (a UUID string), and swaps itself for the workspace matching the database kind.
That id is the first argument of every subsequent command — the backend looks the live handle up by
it in `DbState.connections`.

## Connection lifecycle

1. `connect_db(config)` — if `config.ssh` is set, `ssh::open_tunnel` first opens a local
   listener that forwards to the real host, and the driver is pointed at `127.0.0.1:<local_port>`
   instead. Every kind is wrapped in a 10s timeout.
2. The resulting handle (`MySqlPool`, `PgPool`, `mongodb::Client` or a `redis::Connection`) is
   stored in `DbState.connections` under a new UUID, together with the `Tunnel`.
3. Every later command takes that `id` and calls one of `commands::{mysql_pool, postgres_pool,
   mongo_client, redis_connection}` in `modules/db/commands/mod.rs`, which lock the map, **clone the handle out, and release the lock** before
   anything is run on it, erroring with `"Connection is not a … connection"` on a kind mismatch.
   Never hold `connections` across an `await` on a query: the map is one lock for the whole app,
   and a query awaited under it stops every other tab.
4. `disconnect_db(id)` removes the entry, which drops the handle and with it the `Tunnel`, whose
   own `Drop` aborts the forward. `DbTab` calls it both from the Disconnect button and
   from an unmount cleanup — closing a tab is a disconnect, and the backend hears about it no
   other way.

The other two modules keep state of their own, each behind its own struct and never meeting
`DbState`. `RestState` holds a `reqwest::Client` per `(follow_redirects, accept_invalid_certs)` pair
— cloned out, so the second request to a host skips the TLS handshake — and a `CancellationToken`
per send in flight, which is what `rest_cancel` looks up. `TerminalState` holds a `Session` per
open tab: the two channels a shell is written to and resized through, plus a token whose `Drop`
kills the shell, so no path can leak one. Neither has a lifecycle the shell knows about; both are
undone by the tab unmounting and calling the module's own close command.

Redis is the one kind whose handle reconnects itself: it goes through a `ConnectionManager`, since
a desktop client idles long enough for a server's `timeout` to close the socket underneath it. The
selected database is therefore part of the connection info rather than a `SELECT` sent by hand —
`redis::select_db` reopens the connection so that a later reconnect still lands in the right one.

## A connection handed over by another program

MixEngine's `mix database open` starts MixLab as
`mixlab "mixlab://connect?kind=…&host=…&port=…&user=…&label=…&password_env=MIXENGINE_DB_PASSWORD"`
with the password in that one variable. Three files carry it, none of them a module's except the
last:

- `launch.rs` reads `argv[1]` and takes the variable out of the environment **on the first line of
  `run()`**, before the builder spawns a thread or the webview forks a helper. It only trusts a
  variable named `MIX…_…PASSWORD` — once `mixlab://` is registered with the OS, a web page can name
  any variable in a link. It then either forwards `{url, secret}` to a copy already running or
  queues a `TabRequest` the shell drains (`launch_take_requests`, `launch://request`).
- `instance.rs` is that channel: a named pipe on Windows, a Unix socket under
  `$XDG_RUNTIME_DIR`/`$TMPDIR` elsewhere, one JSON line each way. It replaces
  `tauri-plugin-single-instance`, which forwards `argv` only. A second start with no URL just
  brings the window up.
- `modules/db/handoff.rs` turns the URL into a `ConnectionConfig` kept in `HandoffState` until the
  tab opened for it calls `handoff_take` once; `DbTab` then fills the form and, when the password
  came along or the server has no accounts (`handoffArrival.ts`), goes through the ordinary
  `connect_db`. A `mixlab://` link from a browser brings no password, so that tab waits at the form
  with the caret in the password field. Nothing is saved unless the user presses Save.

`tauri-plugin-deep-link` registers the scheme through the installers. Only macOS listens to its
events (the Apple Event is the only way a URL reaches a macOS process); on Windows and Linux the
plugin re-emits `argv`, which `launch.rs` has already handled, and later URLs arrive over the
channel. The whole design, with the threat model for the variable name, is
[the spec](../../specs/2026-09-03-mixengine-connection-handoff-design.md).

## MongoDB is the odd one

Mongo is configured as a single connection string, not host/port/user/password — a seed list like
`mongodb://a:27017,b:27017/?replicaSet=rs0` cannot be expressed in separate fields. So:

- `ConnectionConfig.uri` carries everything, and `host`/`port`/`username`/`password` are ignored.
- Tunneling can't use `resolve_endpoint`; it parses the first endpoint back out of the URI
  (`mongo::first_endpoint`), tunnels that, and overrides the host afterwards.
- The frontend parses the URI with a regex (not `new URL`, which rejects seed lists) purely for
  cosmetics: the tab title and the initially selected database.

## Persistence

- **Saved connections** — split in two by [src/modules/db/savedConnections.ts](../../../apps/desktop/src/modules/db/savedConnections.ts),
  which is the only module that knows about the split:
  - What a connection *is* (host, port, user, database, sidebar width) — `tauri-plugin-store`,
    file `connections.json`, key `saved`. Plain text on purpose.
  - What lets you connect (`password`, the whole Mongo `uri`, the SSH password and key
    passphrase) — the OS credential store, through the `secrets_*` commands
    ([src-tauri/src/secrets.rs](../../../apps/desktop/src-tauri/src/secrets.rs)): Windows Credential Manager, the
    macOS Keychain, the Secret Service on Linux. One JSON entry per connection id, under the
    service name `MixLab`.

  A connection saved by an older build, with its password still in the file, is moved across the
  first time it is read and the file rewritten without it.
- **Query history, drafts and snippets** — `tauri-plugin-store`, one file each
  (`query-history.json`, `query-drafts.json`, `query-snippets.json`). The database module's, like
  the connections: a module picks its own store files, and nothing generalises persistence.
- **The REST module's own files** — `rest-workspace.json` (the sidebar's collections and the open
  requests), `rest-history.json`, `rest-environments.json`. Same split as the connections: an
  environment variable marked secret lives in the credential store, not in the file.
- **The terminal's own files** — `terminal-hosts.json` (a saved target without its secrets: a shell
  on this machine or an SSH server, each with the commands to run on connect) and
  `terminal-settings.json` (font, scrollback, cursor, default shell). A server's password and key
  passphrase go through the same `secrets_*` commands; a local shell has nothing to put there. The
  file keeps its name from when the list held only servers.
- **Theme** (`mixlab-theme`) and **language** (`mixlab-lang`) — `localStorage`, not the store.

Every one of those files is the module's own. There is no shared persistence layer to add a key to,
and adding one would be the first thing that made two modules care what the other keeps.

## Security posture

[`ssh/`](../../../apps/desktop/src-tauri/src/ssh/mod.rs) verifies host keys on a trust-on-first-use basis: a server never seen before is
accepted and its SHA-256 fingerprint written to `known_hosts.json` in the app data directory, and
a later connection offering a different key is refused with both fingerprints in the message. Its
own file, not OpenSSH's `~/.ssh/known_hosts`. Accepting a rebuilt server's new key means deleting
its entry there. Both ways in go through it: `open_tunnel` for a database behind a hop, and
`open_shell` for a terminal tab, which is why the file is the app's rather than the database
module's.

Two deliberate tradeoffs, marked in the code:

- `drivers/mysql.rs` runs user-authored SQL through `sqlx::AssertSqlSafe`, opting out of sqlx's injection
  guard. This is a database client — arbitrary SQL is the product, not a bug. Identifiers the app
  itself interpolates (database/table/column names) still go through `quote_ident`.
- `rest_send` will accept an invalid certificate, and will send to any host, when the request asks
  it to. This is an HTTP client — talking to a server with a self-signed certificate is a thing it
  is for. The setting is per request, off unless it was turned on, and the client built for it is
  kept apart from the ordinary one by `RestState`'s key.

The webview itself runs under a CSP (`app.security.csp`, with a looser `devCsp` for Vite's HMR):
everything loads from `'self'`, and `script-src` allows no inline script — which is why the theme
preload lives in [public/theme-preload.js](../../../apps/desktop/public/theme-preload.js) rather than in
`index.html`. Adding a `<script>` to the HTML will silently not run.

That policy reaches further than the app's own document. A frame with no response of its own —
`srcdoc`, `data:`, `blob:` — inherits it, and nothing inside such a frame can lift it: CSP policies
intersect, so a `<meta http-equiv>` there only ever tightens. The REST response Preview needs the
opposite, so it is **served**: `modules/rest/preview.rs` registers the `mixlab-preview` scheme and
answers with the response body under a policy of its own, which is what the pane's two switches
actually pick. `frame-src` in the config names that scheme in both its forms — `mixlab-preview:` and
the `http://mixlab-preview.localhost` that Windows and Android rewrite it to — and it is the only
frame source the app allows.
