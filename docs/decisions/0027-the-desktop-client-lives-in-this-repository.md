# 0027. The desktop client lives in this repository, behind the same API

**Status**: Accepted — supersedes [0011](0011-no-gui-in-this-repository.md); its naming paragraph is
superseded by [0044](0044-mixlab-is-the-product-and-mixengine-is-the-engine.md); its rule 3 is
superseded by [0056](0056-mixlab-stands-without-mixengine.md), which gives an install that carries
the window an updater of MixLab's own
**Date**: 2026-09-08

## Context

[ADR 0011](0011-no-gui-in-this-repository.md) put the graphical client in another repository and
gave three reasons. All three were true on 2026-08-22, and the seventeen days since have changed
what each one weighs.

**The client it anticipated was built, next door, and it is complete.** the standalone client grew a fifth module,
`mixengine`, between 2026-09-04 and 2026-09-07: every one of the nine screens
[client-surface.md](../features/client-surface.md) lists, the transport over the Unix socket and
the Windows named pipe with the owner check R1 asked for, the three SSE streams, autostart of
`mixengined`, elevation as a flow, jobs as per-row state, and `daemon.update_apply` behind its
Update pane. [Phase 10](../roadmap/phase-10-client-surface.md) was closed by that module reading
this repository's API against its own screens. So the first reason — *"it is the second of
something that already exists"* — has inverted: there is now exactly one desktop shell, and the
question is only which repository it is released from.

**The critical path is behind us.** The second reason was that screens were not what stood between
the project and `https://blog.test`. Phases 0 to 10 are done and M9 shipped as v0.0.1. Nothing a
desktop client needs is still waiting to be built underneath it.

**Two products are what users are refusing.** People who want MixEngine do not want to download a
second application to get a window; and people who already have a database client they like do not
want the standalone client's one imposed as the price of that window. Both complaints are about *packaging*, and
neither is answerable while the two are released from two repositories on two cadences with two
installers and two updaters.

The third reason — *"a client on the far side of a published API tests it harder"* — is the one that
is still true, and this ADR keeps it rather than arguing it away. What ADR 0011 conceded it lost is
worth quoting: *"the API loses its most demanding consumer from this repository's CI … an
insufficient API surfaces in another repository, later, across a boundary."* T96 and T97 were
exactly that bill.

## Decision

**The desktop application moves into this repository**, under `apps/desktop/`, with its git history,
and is released from here as part of MixEngine. This repository now ships `mixengined`, `mix`,
`mixengine-elevate`, `mixengine-shim` **and one desktop application, named MixLab**. The name is
the window's alone: the daemon, the CLI, the helper, the shim, the home directory, `MIXENGINE_HOME`,
the installers and the release feed keep MixEngine's name. MixLab is what a person opens; MixEngine
is what runs underneath it and what a terminal calls. The CLI-only distribution does not go away: a
headless archive with the four binaries is published beside the installers, and every capability
still reaches a person through `mix`.

Five rules replace ADR 0011's single one.

1. **The desktop application reaches the daemon the way any client does, and no other way.** Its
   Rust half may depend on `mixengine-proto` and `mixengine-platform` — the same two crates
   `mixengine-cli` is allowed — and on nothing else in `crates/`. Never `mixengine-core`, never
   `mixengine-daemon`. It speaks JSON-RPC over the local endpoint, reads the three streams, and
   its TypeScript is typed against [`bindings/`](../../bindings/) directly rather than a vendored
   copy. A test in the desktop crate reads its own manifest and fails on any other `path`
   dependency, the way `workspace_layering.rs` guards the crates above it.

2. **The application hosts two kinds of thing, and the line between them is drawn in code.** The
   `mixengine` module is a thin client of the daemon. The `db`, `rest`, `terminal` and `tools`
   modules are a **toolbox that runs in the application's own process**: a database client for any
   server, an HTTP client, a terminal. They keep their own Rust, their own saved state and their own
   credentials, and the daemon never learns they exist. "No business logic in clients" is a rule
   about MixEngine's capabilities, and a MySQL client for somebody's staging server is not one of
   MixEngine's capabilities. Three things are enforced by lint: a toolbox module never dials the
   daemon; the `mixengine` module never imports from a toolbox module; and the one bridge between
   them is the shell's own *open a tab* request, which already exists and already carries a
   connection in-process.

3. **One installer, one updater, and both are MixEngine's.** Tauri produces the desktop executable
   and, on macOS, the `.app`; it bundles nothing and updates nothing. The desktop executable joins
   the four binaries in `packaging/`'s installers and in the update payload, and the daemon's own
   updater ([updates.md](../features/updates.md)) replaces it. The Tauri updater and its NSIS
   installer leave with the merge. An updater applies **only the binaries the install already
   has**: a headless install never grows a window it did not ask for.

4. **The toolbox is optional to look at, never optional to ship.** The shell carries a module
   visibility setting with presets — *MixEngine*, *Everything*, *Database tools* — chosen at first
   run and changeable in Settings. A fresh install defaults to *MixEngine*: the person who only
   wanted a window over the daemon sees the Dashboard and nothing of the database client. One
   build, one artifact per platform; what differs is a setting, not a product line.

5. **The desktop crate is a Cargo workspace of its own, excluded from the root one.** Its
   dependency tree — Tauri, a webview, four database drivers, an SSH client, a pty — is several
   times the size of the daemon's, carries a second TLS stack, and would defeat the root
   `deny.toml`'s duplicate-version ban and the elevated helper's dependency budget in one move.
   Two lock files are the cost; the root workspace's guarantees are what they buy. Folding it in is
   allowed later, as its own decision, when there is a reason.

**The licence is MIT OR Apache-2.0 for everything here**, as both repositories already state. The standalone client's
note about GPL-3.0 for SignPath code signing recorded an option that was never taken; it comes
along as history and is not adopted. Code signing stays where [parked.md](../roadmap/parked.md)
put it.

**`mixnz/<old>` is archived once the merged product ships**, with its last release pointing here.
Until then it stays what users have installed. The `desktop-app` extension kind, the standalone client's registry
entry and `database.open` are unchanged: they exist for an external client, and an installed
standalone client is one for as long as anybody keeps it.

## Consequences

**Easy.** One download, one version number, one release pipeline, one changelog. The API gains
back what ADR 0011 gave up: a type reshaped in `mixengine-proto` fails the desktop typecheck in the
same CI run, not in another repository weeks later. The borrowed pieces the standalone client kept in step by hand
— the vendored bindings, the pipe-name fingerprint copied from `mixengine-platform` — become
imports. A person who wants only the terminal still has it; a person who wants only a window has it
without a database client in the way.

**Hard, and accepted.** The repository grows a Node toolchain, a hundred thousand lines of
TypeScript and a second Cargo workspace, and CI's `build` legs grow a webview build. The four
toolbox modules are a second domain to maintain under one roof, with their own conventions
(`apps/desktop/CLAUDE.md`). The layering test can no longer *see* the desktop crate from the root
workspace, so the rule in point 1 is enforced by a test living on the far side of the boundary it
guards. Rule 3's *only what is there* policy means an install from before the merge never receives
the desktop application through `mix self-update`; the release notes say so, and the installer is
how it arrives.

**What does not change.** Every rule in `CLAUDE.md` about the daemon, the CLI, elevation and the
platform layer. `client-surface.md` stays the list of what the API must answer — it is now checked
by a compiler as well as by reading.

## Alternatives considered

- **Keep two repositories; have MixEngine's installer bundle the standalone client's release.** Answers "one
  download" and nothing else: two updaters that do not know about each other, two pipelines, and a
  profile that hides the toolbox has no clean seam to live in. Rejected.
- **One Cargo workspace from the start.** Cleaner in the long run. Rejected for now because it
  reopens `deny.toml`, the TLS-stack choice and the helper's dependency budget in the same change
  that moves a hundred thousand lines — three decisions hidden inside a relocation. Point 5 leaves
  the door open.
- **Move MixEngine into the `<old>` repository.** The daemon is the product and carries the heavier
  CI; a client is what moves. Rejected.
- **Keep the Tauri updater for the window and MixEngine's for the daemon.** Two programs replacing
  files in one directory on two schedules, one of which restarts the other. Rejected on sight.
