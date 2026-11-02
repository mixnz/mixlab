---
status: approved
date: 2026-11-02
task:
  - T212
---

# T212 — ngrok in the Tunnel tab, beside cloudflared

Roadmap task T212, added to phase 41. 2026-11-02. It follows T203, whose spec
([T203](2026-10-08-t203-a-tunnel-module-design.md)) kept one provider *until a second has a user*
and named the module for what it does *so a second can join it*.

## The problem

Many developers already share a local server with ngrok. They have an account, they ran
`ngrok config add-authtoken` once, and `ngrok http 3000` is in their fingers. MixLab's Tunnel tab
offers them Cloudflare's quick tunnel only, so they either learn a second tool or keep a terminal
open beside MixLab for the one they know.

**What this task is for is familiarity, not a feature Cloudflare lacks.** A person who uses ngrok
finds ngrok in the Tunnel tab, and it behaves the way the cloudflared row already does: type a
local address, press Start, get a public URL, Copy, Open, Stop, ended with MixLab.

## Goals

- The Tunnel tab starts a tunnel with **cloudflared or ngrok**, the person's choice per tunnel.
- Both kinds sit in **one list** and in the tray's one section, and are stopped, copied and ended
  with MixLab exactly alike.
- ngrok is found and fetched the way cloudflared is (T203 D2): the machine's own first, then a
  pinned one MixLab downloads on a click, checked against its SHA-256.
- A person with no authtoken is told what ngrok needs, and can give it from MixLab.

## Non-goals

- **Choosing a domain.** ngrok's free plan assigns each account one dev domain, and a paid plan
  can reserve more; MixLab passes no `--url`, and the URL is whatever ngrok gives. Same as the
  random `trycloudflare.com` today.
- **ngrok's features beyond `ngrok http`:** TCP and TLS endpoints, Traffic Policy, OAuth, basic
  auth, the inspector at `localhost:4040`, `ngrok start` with named tunnels from the config.
- **An account in MixLab.** MixLab never signs anybody up, never reads the ngrok dashboard, and
  keeps no token of its own (D3).
- **Any provider besides these two.** SSH services (localhost.run, Pinggy), Tailscale Funnel and
  Dev Tunnels were weighed and left for later.
- **MixEngine.** Nothing here dials `mixengined`, as in T203.

## Anti-goals

- **MixLab holding an ngrok secret.** The authtoken is ngrok's, kept where ngrok keeps it. MixLab
  does not copy it into localStorage, its keyring, sync, logs or an error message.
- **ngrok changing the person's config behind their back.** The only write to ngrok's config is the
  one a person asks for by pressing *Save authtoken* (D3).
- **A cloudflared user noticing anything but a choice.** Somebody who never picks ngrok gets the tab
  they had, with one more control.

## Decisions

### D1 — A provider per tunnel, chosen beside the address

The form gains a **provider choice**, *Cloudflare* or *ngrok*, before the address field (a
segmented control from `src/components/` if one exists, otherwise its dropdown). The choice is
remembered on this machine in `localStorage` (`mixlab-tunnel-provider`), not synced: a provider is
a fact about what this machine has. Default `cloudflare`, so the tab opens as it did.

- **Every row says its provider**: a new *Provider* column in the tab, and a short label beside each
  line in the tray.
- `TunnelInfo` gains `provider: "cloudflare" | "ngrok"`; `tunnel_start` takes it; `tunnel_binary`
  and `tunnel_download` take it too and answer for that provider.
- **The texts that name Cloudflare become per provider.** The tab's description becomes neutral
  (*Share an address on this machine on the internet*). The *for testing* note under the form is
  the selected provider's: Cloudflare's as today; ngrok's says the free plan shows visitors a
  warning page before the site and allows a limited number of endpoints at once (wording settled
  against ngrok's limits page when written). *Anyone with the link can open this while the tunnel
  runs* stays, for both.
- **What stays per provider in the backend** is a small table in `src-tauri/.../tunnel/`: program
  name, pinned downloads, arguments, how the URL is read, which lines are hints. The running list,
  the stop and the exit (`state.rs`) stay one.

### D2 — The ngrok binary: the machine's first, then one MixLab downloads

The rule of T203 D2, for a second program.

1. An `ngrok` at a path set in the module's Settings pane, or on `PATH`, is used as it is. A v2
   agent (`ngrok version` 2.x) is not refused up front; it fails on v3's flags, and the row shows its
   own error beside the offer to download.
2. Otherwise the card offers **Download ngrok** with its version and size. MixLab fetches the pinned
   archive into `<data>/tunnel/`, checks the SHA-256 compiled into the build before unpacking, unpacks
   `ngrok` with `tar`, and makes it executable on Unix. Never put on `PATH`.

**Pinned by version, never by channel.** ngrok's own links (`.../ngrok-v3-stable-...`) move with
every release, so a pinned digest would break on the next one. The
[archive](https://dl.equinox.io/ngrok/ngrok-v3/stable/archive) gives each version its own URL under
`bin.equinox.io/a/<id>/ngrok-v3-<version>-<os>-<arch>.tar.gz`. MixLab pins one version, with one URL,
SHA-256 and size per target, bumped by
[bumping-tool-downloads](../standards/desktop/bumping-tool-downloads.md), which gains an ngrok
section. Nothing updates unasked: ngrok's `update_check` only prints a notice, and its binary is
replaced by `ngrok update`, which MixLab never runs.

| Target | Asset | Note |
| --- | --- | --- |
| macOS x86-64 / arm64 | `ngrok-v3-<v>-darwin-amd64.tar.gz` / `-arm64` | |
| Linux x86-64 / arm64 | `ngrok-v3-<v>-linux-amd64.tar.gz` / `-arm64` | |
| Windows x86-64 / arm64 | `ngrok-v3-<v>-windows-amd64.tar.gz` / `-arm64` | native ARM64, unlike cloudflared |

`.tar.gz` everywhere, so the existing `Shape::Tgz` path and the `tar` that ships with Windows 10 and
up do it all. The version at the time of writing is 3.39.13; the builder pins whatever is current
when the task is built, and records the digests from the downloaded files.

The binary lookup in `binary.rs` becomes per provider (name, settings key, pins); `find_in` and
`install` take the provider's table rather than growing a second copy.

### D3 — The authtoken stays ngrok's; MixLab can hand one to ngrok

**MixLab passes no `--config` and no `--authtoken`.** ngrok reads its own config file
(`%LocalAppData%\ngrok\ngrok.yml`, `~/Library/Application Support/ngrok/ngrok.yml`,
`~/.config/ngrok/ngrok.yml`) and `NGROK_AUTHTOKEN`, exactly as it does in a terminal, so the person
who already ran `ngrok config add-authtoken` has nothing to do.

**A start without an authtoken is named.** ngrok refuses with its `ERR_NGROK_4018`; the row turns
`failed` with a `needsAuthtoken` hint: *ngrok needs an authtoken from your ngrok account*, a link to
the dashboard's authtoken page, and a pointer to the Settings field below.

**Settings has an *ngrok authtoken* field with a Save button.** Save runs
`<the ngrok MixLab would use> config add-authtoken <token>`, ngrok's own command, which writes ngrok's
own file. MixLab stores nothing: the field is empty again after Save, says *Saved to ngrok's config*,
and the value is never logged or put into an error (a failure shows ngrok's output with the token
replaced by `***`). This is what makes a MixLab-downloaded ngrok usable at all, since that binary is
not on `PATH` and the person cannot run the command by name. With no ngrok found yet, Save is
disabled and the field says to download ngrok first.

The token is a command-line argument for the life of that one short process, visible to other
processes of the same user while it runs. That is how ngrok's own documentation tells people to
set it, and it is accepted here; the tunnel process itself never carries it.

### D4 — Running ngrok as a child of MixLab

`ngrok http <target> --log stdout --log-format json`, spawned with `spawn_supervised` like
cloudflared, so the job object (Windows), `PR_SET_PDEATHSIG` (Linux) and the macOS records file all
apply unchanged.

- **The URL is read from its JSON log on stdout**: the line whose `msg` is `started tunnel` carries
  `url`. Parsed as JSON (`serde_json`), not by a pattern on the host, because the assigned domain's
  suffix differs by plan and has changed over time (`ngrok-free.app`, `ngrok-free.dev`, custom
  domains). The URL must be `https://`.
- **The reader becomes per provider**: cloudflared's stderr with `public_url`, ngrok's stdout with a
  JSON reader. The tail kept for a failure is the last lines of that same stream, and for ngrok
  shows each line's `err` or `msg` field rather than raw JSON.
- **No console UI.** With `--log stdout` and no TTY ngrok draws none (`console_ui: iftty`); a config
  that sets `console_ui: true` is the person's, and is measured rather than overridden (Open
  questions).
- **The macOS sweep** checks the recorded program's name against both `cloudflared` and `ngrok`
  (`is_cloudflared` becomes `is_ours`). The records file stays one file.
- **The target rule (T203 D4) is unchanged**: loopback only, no scheme means `http://`, the host as
  typed. ngrok takes `http://localhost:5173` as its upstream address.
- **Hints.** `NothingListening` is set from ngrok's own line for a refused upstream connection
  (measured, see Open questions). `AllowedHosts` works as it does: the one `GET /` after the URL
  appears, sent with `ngrok-skip-browser-warning: 1` so the free plan's warning page does not hide
  Vite's `403`. `needsAuthtoken` is new (D3). Any other failure shows ngrok's `ERR_NGROK_*` line as
  the row's detail, which already names the problem and links ngrok's page for it — including a
  second tunnel refused because the free plan's dev domain is already online.

### D5 — What stays as it is

The running list held in Rust, one tab (`singleTab`), the tray section always lent, closing the tab
or the window stopping nothing, `stop_all` at exit, nothing saved across a restart, nothing synced
(the provider preference included), the tab's session slot empty. Presets are untouched: this
adds no module.

## Out of scope

- A stable URL as a feature of its own: a person whose ngrok always gives the same domain gets it,
  and nothing is built around it.
- Running ngrok's inspector or reading its local API on `:4040`.
- Detecting that a person's ngrok config is v2 and migrating it.
- A headless story: as in T203, a headless user runs `ngrok` themselves.

## Open questions

To be measured on the pinned version, on Windows and macOS at least, before the code depends on
them. None of them changes the design; each fixes a string or a pattern.

1. The exact JSON of the `started tunnel` line, and of the line for an unreachable upstream.
2. The exact text of `ERR_NGROK_4018` with no authtoken, and of the error for a dev domain already
   online.
3. Whether two ngrok agents at once both start (the second's web interface moves to `:4041`, or
   fails) on a free account, and what the second row then says.
4. Whether `console_ui: true` in a person's config breaks reading stdout; if it does, pass
   `--config` with their file plus a small one setting `console_ui: false`.
5. Whether an `https://localhost` upstream with a self-signed certificate is verified by ngrok;
   the row shows whatever ngrok does, as T203 D4 does for cloudflared.

## MixLab

- **Tunnel tab:** the provider choice before the address (D1), a Provider column, the per-provider
  download card and *for testing* note, the `needsAuthtoken` hint with its dashboard link (D3).
- **Settings → Tunnel:** an *ngrok path* field beside the cloudflared one, and the *ngrok authtoken*
  field with Save (D3).
- **Tray:** each line names its provider.
- **Backend:** `src-tauri/src/modules/tunnel/` gains a provider table (D1), ngrok pins in
  `binary.rs` (D2), a JSON reader in `process.rs` and a per-provider reader in `state.rs` (D4), and
  a `tunnel_ngrok_authtoken` command (D3). `TunnelInfo` and the commands carry `provider`.

Nothing in the `mixengine` module changes and no daemon method is added, so `check-client-surface` is
unchanged. Strings go into the module's `en.ts` and `vi.ts` together. The changelog gets `Added`:
start an ngrok tunnel from MixLab's Tunnel tab, beside Cloudflare's.

## Acceptance

- A person with ngrok on `PATH` and an authtoken already set picks *ngrok*, types `localhost:5173`,
  and gets an `https://` URL that opens the dev server from another network, with no other step.
- On a machine with no ngrok, the tab offers the download with version and size; the archive is
  checked against its pinned SHA-256 before it is unpacked, on Windows, macOS and Linux.
- With no authtoken, the row names it and links the dashboard; pasting the token in Settings and
  pressing Save makes the next start succeed, and the token appears in no MixLab file, log or error.
- A cloudflared tunnel and an ngrok tunnel run at once, each row naming its provider, in the tab and
  in the tray.
- Quitting MixLab ends both kinds. Killing MixLab ends them on Windows; on macOS the next start ends
  a leftover `ngrok`, and leaves alone a recorded pid that now belongs to another program.
- A Vite dev server that blocks unknown hosts names `server.allowedHosts` on an ngrok row too.
- Somebody who never picks ngrok sees the tab as before, plus the provider choice set to Cloudflare.
