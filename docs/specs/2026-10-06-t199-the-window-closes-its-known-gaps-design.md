---
status: implemented
date: 2026-10-06
task: T199
---

# T199 — The window closes its known gaps

Roadmap task T199, in a new phase 37. 2026-10-06.

## The problem

`apps/desktop/client-surface-exceptions.json` lists nine daemon methods under `knownGaps`: things
`mix` can do and a person in MixLab cannot. The list is only allowed to shrink, and these nine
have sat in it since T182i. Each is either a button the window should have, or a method the window
has no use for that was never said so:

| Method | What happens to it |
| --- | --- |
| `site.start`, `site.stop` | the Sites screen gets Start/Stop (D1) |
| `site.delete` | the Sites screen gets Delete (D1) |
| `job.cancel` | a running job gets Cancel (D2) |
| `cert.status` | the Domains screen shows what the front end presents (D3) |
| `project.export` | the Projects screen writes `mixengine.toml` (D4) |
| `job.list` | moves to `cliOnly` (D5) |
| `cert.ca_rotate` | moves to `cliOnly` (D5) |
| `cert.ca_uninstall` | moves to `cliOnly` (D5) |

All nine are already in the contract (`bindings/`) and already answered by the daemon. This task
adds no daemon method, changes no wire type and touches nothing under `crates/`. Every change is in
`apps/desktop/`: one Tauri command per method the window now calls, in
`src-tauri/src/modules/mixengine/commands.rs`, its `api.ts` wrapper, the screen that uses it, and
the exceptions file.

When it is done, `knownGaps` is empty and `node scripts/check-client-surface.mjs` prints
`0 known gap(s)`.

## D1 — Sites: Start/Stop and Delete on each row

**What a site's state is.** `site.start` and `site.stop` flip a flag and re-render the front end's
configuration: whether Nginx or Caddy has a server block for the site's domains. They start and
stop **no process** — not the front end, not the PHP pool, not the database. A stopped site's
services keep running, and a started site's services are not started by it (`rpc.rs`,
`SITE_START`). Starting what a site needs is already the **Open** button's job
(`service.start_project`), and stays so.

The Sites table's action cell keeps **Open** and **Edit** as buttons and gains a **⋮** button
after them, the same `ActionBar` + `MoreIcon` + `ContextMenu` the Dashboard's service rows use.
The row's other actions live in that menu rather than as more buttons: a cell of five buttons is
wider than the table can spare, and Start/Stop and Delete are not what a person does to a site
every visit. The menu holds, in order:

- **Stop** on an enabled site, **Start** on a disabled one — one entry whose label follows
  `row.state`. It calls `site.start` / `site.stop` and replaces the row with the `site` field of
  the `SiteDetail` that comes back (`SiteRow` is `SiteSummary`, and `SiteDetail.site` is one), so
  the State pill changes without a reread. Both work for an extension's site as well as a
  project's: `Sites::serving` does not call `editable`, and stopping a site an extension added is
  a fair thing to want. Both are disabled while a site is being opened, as Open already is.
- a separator, then **Delete** in the menu's danger style (`.context-menu-delete`), disabled for
  an extension's site with the same hint as Edit, because the
  daemon refuses it (`Sites::editable`). It opens a `ConfirmDialog` that says the site's domains are
  released and **the doc root is kept on disk**. After `site.delete` answers, the table rereads and
  a `NoticeBanner` names `doc_root_kept` — the same rule as Projects' delete, said after the fact
  as well as before because the path is the thing a person may go looking for.
- **Delete is also disabled while the site is shared**, with the hint "Stop sharing it first".
  A disabled entry carries its reason as its `title`.
  `Sites::delete` re-renders the front end and the hosts file but does not call
  `wants_the_firewall` or `advertises_what_it_declares`, so deleting a shared site would leave its
  firewall rule and its mDNS name in place until something else reconciles them. The window does
  not compose `site.unshare` + `site.delete` itself: an order of operations is the daemon's to
  decide. The daemon's half is a separate fix (see *Left for later*).

Start/Stop asks for no confirmation: both are undone by the other button, and neither touches a
file the user wrote.

## D2 — Jobs: Cancel on a running job

The Dashboard's running-jobs list (`jobs`, fed by `job_progress`) gets a small **Cancel** button
per row, calling `job.cancel`. Cancellation is cooperative, so the button turns into a
"Cancelling…" busy state and stays there until `job_finished` removes the row; a second click is
not offered. A job that never looks at its token runs to its end, and the button stays busy until
then, which is the truth: the work is still going (`JobState` has no `cancelling`, on purpose). The
Cancelling mark is a set of job ids local to the Dashboard, dropped for an id when its
`job_finished` arrives. No confirmation: a cancelled install is rerun with one click, and
`job.cancel` on a job that has already ended is not an error.

The same Cancel appears in the Blueprints `ApplyDialog` while its job runs, because a person
applying a blueprint is in that modal on the Blueprints screen, not on the Dashboard. There it
needs a sentence: cancelling an apply **stops without rolling back** (phase 8, T77), and applying
again continues from what is there.

**A cancelled apply ends in one of two ways, and the dialog handles both.** The daemon's apply
loop checks the token between steps and, when it is set, stops and returns the steps it ran — so
the job ends as `succeeded`, with a short list. The dialog remembers that Cancel was pressed and
says so above that list. A step that gives up *because* of the cancellation ends the job as
`cancelled` instead (`jobs.rs`: an error while cancelled), which `ApplyDialog` today matches
neither as applied nor as failed, so it would sit in `running` for good; that ending moves it to
the `failed` phase with the cancelled sentence, and Close calls `onDone(null)` as a failure does.
The note that the apply stops after the current step appears once Cancel is pressed, not
throughout every apply.

## D3 — Domains: what the front end presents

The certificate table is drawn from `cert.issue`, which says what is **on disk**. `cert.status`
adds what the running front end **presents**, through a live TLS handshake.

The `CertTable` card gains a **Check served** button in its header. It calls `cert.status` (every
site) and merges the answer into the rows by domain, adding a **Served** column:

- `presented` with `trust: trusted` and no `problem` → a success pill, "Served".
- any `problem` → a pill naming it (`names_differ`, `not_served`, `served_certificate_differs`,
  `not_trusted`, `expiring`, `no_certificate`), its tooltip the handshake's `because` when there is
  one.
- `not_asked`, not yet checked, or a domain `cert.status` did not answer for (a site with HTTPS
  off is still a row in the disk table) → "—".

The merge is a pure function beside `buildCertRows` in `certTable.ts`, tested without a daemon.

It is a button and not part of every reload because it opens a socket per site and fails slowly
when the front end is down; the disk table stays instant. A row's **Reissue** clears that row's
Served cell, since what was checked is no longer what is on disk.

`served_certificate_differs` is repaired by reloading the front end, not by reissuing — the pill's
tooltip says so rather than offering a button the window does not have.

## D4 — Projects: write `mixengine.toml`

Each Projects row gains a **Write manifest** button, calling `project.export`. No confirmation:
the daemon merges rather than rewrites, and comments, key order and a hand-written `[site]`
survive. The answer is shown in a `NoticeBanner`: "Created" or "Updated" with `path`, and, when
`sites_omitted` is not empty, a second sentence naming the sites the file could not hold. The table
rereads, so the Manifest column turns to "present".

## D5 — Three methods the window has no use for

These leave `knownGaps` for `cliOnly`, each with the reason below as its entry. No Tauri command is
added for them.

- **`job.list`** — *"A history for a person in a terminal; the window follows jobs live through the
  event stream, and a job that fails says why on the screen that started it."* A history dialog
  would be opened after something went wrong, and in the window the thing that went wrong has
  already said so where it happened (Packages, Blueprints → Apply, the Dashboard's list).
- **`cert.ca_rotate`** — *"Replacing the authority is for a key that has leaked, and it breaks every
  cached chain; `mix cert ca-rotate` is where that is done, deliberately."* It is destructive, rare,
  and the moment it is needed is one where a person is following written instructions anyway.
- **`cert.ca_uninstall`** — *"Removing trust while keeping MixEngine is uninstall's business (T87,
  `mix uninstall`); the window's CA block only repairs trust, and a button undoing that repair
  beside it would be one click from breaking every site."*

`scripts/check-client-surface.mjs` already allows this: a method leaves `knownGaps` either into the
window or into `cliOnly` with a reason somebody can review. This is the second way, three times.

## What does not change

- No daemon method, no wire type, no `bindings/` file.
- The handbook (`docs/guide/`) describes MixLab through `mix` only; its pages already cover these
  commands.
- `docs/features/client-surface.md` says what a client must be able to ask for, not which screen
  does; it does not change.
- `check-client-surface.mjs` is untouched; it already fails on an entry the window now calls,
  which is what forces each `knownGaps` line out in the commit that adds its button.

## MixLab

This task is MixLab's alone. The screens are: **Sites** (Start/Stop and Delete in the row's ⋮ menu), **Dashboard**
(Cancel) and **Blueprints → Apply** (Cancel), **Domains** (Check served), and **Projects** (Write
manifest). `job.list`, `cert.ca_rotate` and `cert.ca_uninstall` have no part in the window, for
the reasons in D5. Every new string goes into `src/modules/mixengine/i18n/en.ts` and `vi.ts`, and
the user-visible additions get lines under `## [Unreleased] → ### Added` in `CHANGELOG.md`.

## Left for later

- **Deleting a shared site should withdraw the share.** `Sites::delete` (daemon `sites.rs`)
  reconciles the front end and the hosts file but not the firewall or mDNS. A daemon fix with its
  own test, in its own task; when it lands, D1's shared-site guard can go.

## Testing

- `node scripts/check-client-surface.mjs` → ok, 0 known gaps.
- `npm run build`, `npm test`, `npm run lint` in `apps/desktop`; `cargo fmt --check` and
  `cargo clippy --locked --all-targets -- -D warnings` in `apps/desktop/src-tauri`.
- Pure logic gets vitest tests where there is logic to test: merging `cert.status` into cert rows
  (`certTable.ts`), the served-pill mapping, the Cancelling set against `job_finished`, and which
  site rows may be deleted.
- By hand in `npm run dev:app` against a sandbox home: stop a site and see its domain stop
  answering while its services keep running, then start it again; delete one, and see Delete
  disabled on a shared one; cancel a runtime install and a blueprint apply; check served with the
  front end up and down; export a project with and without an existing `mixengine.toml` and read
  the file.
