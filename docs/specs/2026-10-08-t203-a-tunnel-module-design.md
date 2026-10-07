---
status: draft
date: 2026-10-08
task:
  - T203
  - T203a
  - T203b
---

# T203 — A `tunnel` module: share a local address on the internet with cloudflared

Roadmap tasks T203 to T203b, in a new phase 41. 2026-10-08. They follow phase 39, where cloudflared
was considered as a MixEngine extension and set aside: what a person wants from it is something
they start, use and stop while watching, which is a MixLab tool rather than a service the daemon
supervises (the T201 spec's *Out of scope*).

## The problem

A developer wants to show what is running on their machine to someone who is not on it: a client
looking at a dev server, a webhook from Stripe or GitHub reaching a local endpoint, a phone on mobile
data. MixEngine's LAN sharing (T74) reaches the same network and no further.

Cloudflare's **quick tunnel** does this with no account: `cloudflared tunnel --url <local address>`
prints a random `https://<words>.trycloudflare.com` and forwards it until the process ends. What it
lacks is everything around that one command: getting the binary, finding the URL in its output,
and making sure it stops.

**Where it belongs.** "Remote tunnels (Cloudflare/ngrok) as an extension" sits in
[parked.md](../roadmap/parked.md). As a supervised extension it would outlive the person who started
it, publishing a site to the internet after they had stopped thinking about it. As a MixLab module it
lives as long as MixLab does, shows what it is sharing while it does, and works with or without
MixEngine ([ADR 0056](../decisions/0056-mixlab-stands-without-mixengine.md)): what it shares is
any address on this machine, a Vite dev server on `localhost:5173` or an API on `localhost:3000`.

**And it is the first module added after presets existed.** A person's choice of modules is stored
as the list of ids the preset held when they chose it (T108). Somebody who picked *Everything* holds
`["mixengine","db","rest","terminal","tools"]`, so a sixth module stays hidden for them, and
`presetOf()` stops calling their set *Everything*. Every module added from now on meets this.

## Decisions

### D0 — A preset is stored as a preset, not as the list it held (T203)

**What is kept is what the person chose.** A new key, `mixlab-modules-preset`, holds the preset id
(`mixengine`, `everything`, `databaseTools`) when the person's set is a preset. When it is there, the modules shown are `MODULE_PRESETS[<id>]` of **the build
that is running**, so a module added to a preset appears for everybody who chose that preset, this
one and every later one. When it says `custom`, `mixlab-modules` is the person's own list, as today, and
nothing is added to it.

**The key has three states.** A preset id when the person's set is a preset; `custom` when it is a
set of their own; absent only on a home no build of T203 has written yet. Absent and `custom` are
different on purpose: a person who unticks Tunnel from *Everything* holds exactly the five modules
*Everything* held before T203, and without `custom` that list would be read as the old preset again
and Tunnel would come back.

**`mixlab-modules` stays, as a list, and is written every time.** Both keys travel in the
`preferences` sync collection (phase 30), and a MixLab from before this task on another machine reads
only the list: a value it cannot read there is a first-run question asked again. So the list remains
the complete answer for an older build, and the preset key is what a newer build reads first.

- **Reading:** a valid `mixlab-modules-preset` wins while the list beside it agrees with it (see *From
  another machine*), and the list is then rewritten to this build's composition if it differs.
  Otherwise the list is read as before (`resolveStoredModules`).
- **Writing:** whatever changes the set writes the list and, beside it, `presetOf(list)` or
  `custom`. Choosing a preset, ticking a box and a tab request turning a module on all go through
  that one write, so no caller has to know about the key.
- **The first read after the update** finds a list and no key at all. A list equal to what a preset
  held **before this task** is taken as that preset, and the key is written. That needs the old
  compositions as a table in `shell/profiles.ts` (`mixengine`: one module; `everything`: the five;
  `databaseTools`: `db`, `rest`, `terminal`, `tools`), because `MODULE_PRESETS` now says something
  else. A list matching none of them is somebody's own and is left alone.
- **From another machine:** the two keys arrive as two sync records, maybe not together, and the
  other machine may run a build that writes only the list. So a preset key wins only while the list
  beside it still says the same thing: a list equal to the preset's composition in this build or
  before this task agrees with it, and any other list is somebody's change made where the preset key
  is unknown. Then the list wins, and the next write marks it `custom`. Without that rule a change made on an
  older build would be undone on every newer one.
- **The standalone client's upgrade** (`LEGACY_SHELL_KEYS` in `profiles.ts`), which today answers
  *Everything* with no stored set, writes the preset key as well.
- **Settings** shows the stored preset, not one `presetOf()` infers from the list.

The new key joins `shell/storageKeys.ts` and `preferencesSync.ts`'s allow-list.
`public/storage-keys.js` only moves keys the standalone client wrote, and this one it never had. All of it is pure functions in `shell/profiles.ts` with
vitest beside them.

**Rejected: adding `tunnel` once to a list that equals an old preset, with a flag.** It works for this
module and has to be written again, with another flag, for the next one.

### D1 — A module of its own, `tunnel` (T203a)

A folder under `src/modules/tunnel/` and `src-tauri/src/modules/tunnel/`, one line in
`shell/registry.ts`, one in `lib.rs` and a block in `modules::handler()`
([adding-a-module](../standards/desktop/adding-a-module.md)).

- **Its own tab, not a tool in `tools`.** `tools` holds things that run in this process and end when
  their tab is closed. A tunnel is a child process that has to be found again from anywhere, which
  is what a tab of its own and a tray section give it (D5).
- **One tab lists every tunnel** (`singleTab`): a tunnel is a row, not a tab. Two tabs would be two
  lists of the same processes.
- **Runs with no MixEngine at all.** Nothing in it dials `mixengined`, and nothing in `mixengine`
  reaches it: it needs a local address and a `cloudflared`, and nothing else.
- **Presets:** in `everything` and in `databaseTools`, the developer's toolbox without MixEngine. Not
  in `mixengine`, which is MixEngine's window and nothing more; a person there turns it on in
  Settings. With D0, a person who chose either of the other two sees it after the update.
- **Name:** *Tunnel* / *Tunnel*. Developers say the word in English (writing-user-facing-text).

### D2 — The binary: the machine's first, then one MixLab downloads (T203a)

The `db` module's rule for `pg_dump` (`modules/db/drivers/tools.rs`), for the same reasons:

1. A `cloudflared` on `PATH`, or at a path set in the module's Settings pane, is used as it is.
   Its version is not checked: quick tunnels are years older than any build a person is likely to
   have. One that fails to start shows its own error on the row, beside the offer to download.
2. Otherwise the tab offers **Download cloudflared** and says the size. MixLab fetches the pinned
   version for this system into its data directory, checks the SHA-256 compiled into this build
   before anything is unpacked or run, and makes it executable on Unix. It is never put on `PATH`.

**Pinned in MixLab's source**, a version constant and one SHA-256 per target, bumped by the
procedure in [bumping-tool-downloads](../standards/desktop/bumping-tool-downloads.md). Nothing
updates unasked: a new cloudflared arrives with a new MixLab.

A file fetched with `curl` carries no quarantine attribute on macOS and no mark of the web on
Windows, so neither Gatekeeper nor SmartScreen stands between the download and its first run; that
was measured for SeaweedFS's `weed` in T201 and is measured again here.

| Target | Upstream asset | Shape |
| --- | --- | --- |
| macOS x86-64 / arm64 | `cloudflared-darwin-amd64.tgz` / `-arm64.tgz` | archive with `cloudflared` |
| Linux x86-64 / arm64 | `cloudflared-linux-amd64` / `-arm64` | bare binary |
| Windows x86-64 | `cloudflared-windows-amd64.exe` | bare binary |
| Windows arm64 | the x86-64 `.exe` | upstream publishes no arm64 build; Windows 11 runs it emulated |

**The download code is shared, not copied.** `tools.rs` already fetches with `curl`, checks a
SHA-256 and unpacks with `tar`. That half moves to `src-tauri/src/downloads.rs`, beside `ssh/` and
`secrets.rs` as code every module may use, and learns the one shape it lacks: a file that is the
program itself, which is copied rather than unpacked. `db` keeps its suites and its pins; only the
fetching moves.

### D3 — A tunnel is a child process of MixLab, and dies with it (T203a)

`cloudflared tunnel --no-autoupdate --url <target>`, spawned through `platform::hide_console`
([spawning-processes](../standards/desktop/spawning-processes.md)).

- **Its address is read from its own output.** cloudflared prints the `trycloudflare.com` URL to
  stderr once Cloudflare has assigned it. The row shows *Connecting…* until a line matches
  `https://[a-z0-9-]+\.trycloudflare\.com`, then the URL with **Copy** and **Open**. A process that
  exits before printing one shows its last lines as the error.
- **It ends with MixLab, never later.** Every tunnel is killed when MixLab quits, including by the
  tray's Quit and by an update's restart: `RunEvent::Exit` in `lib.rs`, beside `launch::stop`, stops
  them all. MixLab has nothing yet that ends a child process with it, so this is new, and it has to
  hold when MixLab does not exit cleanly too:
  The machinery exists: `mixengine_platform::process::spawn_supervised`, which MixLab may use
  (ADR 0027), puts each child in a job object with kill-on-close on Windows and starts it with
  `PR_SET_PDEATHSIG` on Linux. macOS has neither, so each tunnel's pid and `started_at` go into
  `tunnel/running` in the data directory, and the next start ends a recorded process group only if
  `started_at(pid)` still matches and the program is `cloudflared`. A pid alone is reused by the
  system within minutes, and signalling a stranger's process is the one mistake this cannot make
  (ADR 0007, and the rule `services.pid_start_time` follows).
- **One list, in the backend.** Tunnels are held by the Rust side, not by the tab, so the tab and the
  tray panel (a window of its own) read the same list, and closing the tab stops nothing.
- **Closing the window is not quitting.** MixLab keeps running in the tray (phase 32), and the tunnels
  with it, which the tray shows (D5).
- **`--no-autoupdate`.** cloudflared otherwise replaces its own binary, which would break the SHA-256
  MixLab checked and is an update nobody asked for.
- **No state is kept.** A tunnel's URL is new every time, so nothing is saved across a restart and
  nothing is synced. The tab's slot stays empty: it is `localStorage`, which holds ids and never a
  host or a URL ([adding-a-module](../standards/desktop/adding-a-module.md)), and a target is both.

### D4 — What a tunnel points at (T203a)

The tab takes **a local address**: `http://localhost:5173`, `localhost:3000`, `127.0.0.1:8080`. It is
refused unless the host is `localhost`, `127.0.0.1` or `::1`: a tunnel to another machine on the
LAN publishes something that is not this person's to publish.

- **No scheme means `http://`.** An `https://` target is passed as it is, and cloudflared checks its
  certificate: a local server with a self-signed one fails, and the row shows cloudflared's error.
  Turning verification off is not offered.
- **The host is passed as typed.** cloudflared tries both addresses of `localhost` itself, so a
  server listening on `127.0.0.1` only is reached through `localhost` (measured on 2026.10.0).
- **Nothing listening is named.** A request through a tunnel whose target has nothing on it makes
  cloudflared log *Unable to reach the origin service*, and the row then says nothing answered at
  that address, so a 502 in the visitor's browser has a reason on this machine.

What a person is shown before the first start, every time, in one line under the field: *Anyone
with the link can open this while the tunnel runs.* Quick tunnels have no access control, and that
sentence is the whole of the consent.

**A dev server that refuses the tunnel's host is named, not left as a blank page.** Vite 5 and later
answer a request for a host they do not know with *Blocked request. This host is not allowed*, and a
tunnel's host is one they do not know. The request reaches the dev server, so the tunnel is working.
Once the URL appears, the backend sends one `GET /` through it; a `403` whose body says *Blocked
request* makes the row name the setting that lets the host in (`server.allowedHosts`), rather than
leaving the person to read a page that looks like a failure of the tunnel. One request, once: the
module does not watch the traffic.

### D5 — The tray shows what is shared (T203b)

The module lends the tray a section ([ADR 0058](../decisions/0058-the-tray-is-mixlabs-and-a-module-lends-it-a-section.md)):
one line per running tunnel, its target and its URL, with **Copy** and **Stop**. It is drawn only
while a tunnel runs, so a person who closed the window can see from the menu bar that something is
still published, and end it there.

**The section is always lent, and says so when nothing runs.** `TraySection` is part of a module's
definition, not of its state, and the panel exists while any visible module lends one. So for the
*Database tools* preset, which lent none until now, a click on the icon opens the panel rather than
the window; the panel's header still has *Open MixLab*. With no tunnel running the section is one
line, *No tunnel running*.

## Out of scope

- **Named tunnels and a Cloudflare account.** A stable hostname needs `cloudflared tunnel login`, a
  credentials file and DNS records. That is a second product, with secrets on disk to look after.
- **ngrok and other providers.** One provider until a second has a user; the module is named for
  what it does so a second can join it.
- **Sharing a MixEngine site on the internet.** Considered here as a *Share on the internet* item on
  the Sites screen, and taken out. The front end picks a site by its name, so the tunnel would have to
  send `Host: <domain>.test`, and an application then builds every absolute URL from that host:
  Laravel's `url()` and `asset()`, Symfony, Rails and Django from the request, WordPress from its
  stored site URL. The visitor gets the HTML and nothing else: stylesheets, scripts, images and links
  all point at a name only this machine resolves. Webhooks would work; showing a site to someone, the
  main reason to want it, would not. Doing it properly means the front end answering the tunnel's own
  host for that site, a temporary alias the daemon adds and removes, which is a MixEngine feature with
  a method `mix` must reach as well, beside `site.share` (T74). It replaces the tunnel line in
  [parked.md](../roadmap/parked.md) as the thing to come back to.
- **A daemon method.** Nothing here asks `mixengined` for anything. "MixLab reaches what `mix`
  reaches" is about daemon methods, and this module adds none; a headless user runs `cloudflared`
  themselves.
- **Access control on a quick tunnel.** Cloudflare offers none; D4's sentence is the answer.
- **What a quick tunnel cannot carry.** Cloudflare documents quick tunnels as for testing: no
  uptime promise, a cap on concurrent requests, and no Server-Sent Events. The module says *for
  testing* where it offers one, and works around none of it.

## Open questions

None left. **A `~/.cloudflared/config.yml` on the machine** was the last: measured on 2026.10.0 with a
named tunnel's config in `~/.cloudflared`, a quick tunnel still opens and serves, and passing an
empty `--config` only adds an error line. The module passes no `--config`.

## MixLab

- **Shell:** the stored preset (D0), in `shell/profiles.ts`, `storageKeys.ts`, `preferencesSync.ts`
  and `public/storage-keys.js`; Settings shows the chosen preset rather than one inferred.
- **A new `tunnel` module** (`src/modules/tunnel/`, `src-tauri/src/modules/tunnel/`): the tab with
  the address field and one row per tunnel (D3, D4), the download offer and a Settings pane for a
  `cloudflared` path (D2).
- **Tray:** a section listing running tunnels (D5).
- **Shared code:** `src-tauri/src/downloads.rs`, taken out of `modules/db/drivers/tools.rs` (D2).

Nothing in the `mixengine` module changes, and no daemon method is added, so `check-client-surface`
is unchanged. Strings go into the module's `en.ts` and `vi.ts` together. The changelog gets `Added`:
share an address on this machine on the internet from MixLab's Tunnel tab.

## Acceptance

- **T203.**
  - Somebody who chose *Everything* before the update sees the Tunnel tab after it, with no question
    asked; somebody with a set of their own sees exactly that set.
  - Turning one module off in Settings makes the set the person's own, and a later module is not
    added to it.
  - A MixLab from before this task, syncing with one after it, keeps reading a list it understands
    and asks no first-run question; a module turned off there stays off on the newer machine.
- **T203a.**
  - On a machine with no cloudflared, the tab offers the download with its size; the file is checked
    against its pinned SHA-256 before it runs, on Windows, macOS and Linux.
  - A tunnel to a local dev server shows a `trycloudflare.com` URL that opens it from another network.
  - Quitting MixLab ends every tunnel. Killing MixLab ends them on Windows; on macOS and Linux the next
    start ends what was left.
  - A target that is not loopback is refused before anything starts.
  - A tunnel to a Vite dev server that blocks unknown hosts names `server.allowedHosts` on its row.
  - Closing the tab or the window stops no tunnel; quitting from the tray stops them all.
  - On macOS, a recorded pid that now belongs to another program is left alone at the next start.
- **T203b.** With the window closed, the tray lists a running tunnel, copies its URL, and stops it.
