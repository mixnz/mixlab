# MixLab, and MixEngine inside it

**MixLab is the product.** A desktop application whose toolbox — a database client, a REST client,
a terminal, tools — is complete on its own, and which carries **MixEngine** as one optional module.
Many MixLab users never start MixEngine, and nothing MixLab needs from itself may depend on it:
updates, Settings, sync and the toolbox all work with `mixengined` absent, stopped, or never run.
See `docs/decisions/0056-mixlab-stands-without-mixengine.md`.

**MixEngine is the engine**, and the whole product only for its headless distribution and the
person who drives it with `mix`: a local web development environment (ServBay-style) that runs and
switches multiple PHP / Node.js / Python / Ruby versions, bundled Nginx/Caddy +
MariaDB/MySQL/PostgreSQL/Redis/Memcached, local domains with automatic HTTPS — without Docker,
without hand-written config files. A headless install may run on a server and later in production.

When a change touches the window, ask first: *does this still work with MixEngine switched off?*

## Architecture in one paragraph

MixEngine's Rust core is split into three layers. **`mixengined`** (daemon) owns all of MixEngine's
state and supervises every managed process. **`mix`** (CLI) is a thin client over a JSON-RPC API on
a local IPC transport (Unix socket / Windows named pipe). **MixLab**, under `apps/desktop/`, is an
application of its own; its `mixengine` module is a second thin client over that same API — typed
against the published contract in `bindings/`, and reaching no further into this workspace than
`mixengine-proto` and `mixengine-platform` (see
`docs/decisions/0027-the-desktop-client-lives-in-this-repository.md`). **Nothing runs as root.** For the few
one-shot operations that need it (hosts file, OS trust store, resolver config, firewall rules), a
short-lived **`mixengine-elevate`** is spawned through the OS elevation prompt, does the work, and
exits. Cross-platform (Windows, macOS, Linux) from day one — all
OS-specific behaviour lives behind traits in `mixengine-platform`.

## Workspace layout

```
crates/
  mixengine-core/        Domain logic: projects, sites, runtimes, services, blueprints
  mixengine-proto/       Shared API types (requests, responses, events) — single source of truth
  mixengine-platform/    OS abstraction traits + per-OS impls (hosts, trust store, DNS, limits)
  mixengine-supervisor/  Process supervision, health checks, log capture
  mixengine-daemon/      `mixengined` binary: API server + orchestration
  mixengine-elevate/     One-shot elevated binary (minimal, audited, self-validating)
  mixengine-cli/         `mix` binary
  mixengine-shim/        the version-resolving shim, copied into `<root>/bin` per command name
  mixengine-trampoline/  what `<root>/bin` holds per name on Windows: asks the shim, then becomes it
  mixengine-testkit/     Shared test fixtures — **dev-dependency only**, never in a shipped binary
apps/
  desktop/               the desktop application (MixLab from phase 12): a Vite + React frontend,
                         and under src-tauri/ a Cargo workspace of its own, excluded from this one
server/                  MixLab's sync server (ADR 0046): conformance/ the suite, worker/ the
                         Cloudflare Worker, native/ a third Cargo workspace, also excluded
```

**`cargo` at the root sees neither `apps/desktop/src-tauri` nor `server/native`**, and the three
Node projects here — `apps/desktop/`, `server/worker/`, `server/conformance/` — each carry their
own `package-lock.json` and are installed separately. `apps/desktop/CLAUDE.md` is that
application's own set of rules; `server/README.md` is the server's.

## Non-negotiable rules

- **MixLab stands without MixEngine.** Nothing MixLab does for itself — its toolbox, updates,
  Settings, sync — requires `mixengined` to be installed, running or ever started, and the window
  never starts the daemon to do a job of its own. Starting MixEngine is something a person asks for.
  See `docs/decisions/0056-mixlab-stands-without-mixengine.md`.
- **Nothing updates unasked.** MixEngine never downloads or installs a release on its own and
  never reads the update feed unprompted — a headless install may be a production server. An
  install that carries the window is updated by MixLab's own updater (`apps/desktop/src-tauri/`,
  importing nothing from `crates/`), which may announce a release but installs only on a click.
  `mix self-update` stays, and is the headless distribution's updater.
- **No business logic in MixEngine's clients.** `mix` and the `mixengine` module only render what
  the daemon returns.
- **MixLab reaches what `mix` reaches.** A daemon method ships with its MixLab screen in the same
  task; a spec is not done until its `## MixLab` section says which screen does it, or why the window
  has no part in it. `node scripts/check-client-surface.mjs` fails on a method the window cannot
  reach unless `apps/desktop/client-surface-exceptions.json` gives the reason (its `knownGaps` list
  only shrinks), and `node scripts/check-docs.mjs` fails on a new spec with no `## MixLab` section.
  Both run in `scripts/gate.sh` and in CI's lint job.
- **No client-only capability.** Every mutating API method is reachable from `mix`. A gap in the
  CLI is a gap in MixEngine — `docs/features/client-surface.md` is what any full graphical
  client must be able to ask for, and MixLab's `mixengine` module draws every screen from it.
- **MixLab's `mixengine` module is a client, not a second daemon.** It reaches
  the daemon only through the JSON-RPC API and the streams, typed against `bindings/`; its Rust may
  depend on `mixengine-proto` and `mixengine-platform` and on nothing else here
  (`apps/desktop/src-tauri/tests/layering.rs`).
- **The toolbox modules never touch the daemon.** `db`, `rest`, `terminal` and `tools` run in the
  application's own process against servers of the user's choosing; nothing in them dials
  `mixengined`, and nothing in the `mixengine` module imports from them (`npm run lint` in
  `apps/desktop`).
- **No direct OS calls outside `mixengine-platform`.** No `#[cfg(windows)]` in core/daemon code.
- **No persistent root process, ever.** Elevation is one-shot and per-operation.
  `mixengine-elevate` never runs arbitrary commands, validates every request itself rather than
  trusting the daemon, and is excluded from auto-update. The single standing thing MixEngine
  installs is macOS's boot-time `pfctl -e` job — one fixed command, root-owned, no arguments from
  anywhere — argued in
  [docs/decisions/0012-a-boot-time-job-enables-the-packet-filter-on-macos.md](docs/decisions/0012-a-boot-time-job-enables-the-packet-filter-on-macos.md).
- **Generated config is disposable.** Everything under `etc/` is regenerated from state in SQLite;
  never parse a generated file back into state.
- **Cross-platform or not merged.** A feature must compile on all three OSes; unsupported paths
  return a typed `Unsupported` error, never `todo!()`.
- **No Docker, no VM.** Managed processes are native. See `docs/decisions/0003-no-container-isolation.md`.

## Detailed documentation

All design detail lives in [docs/](docs/) — start at [docs/README.md](docs/README.md).

- Architecture → [docs/architecture/](docs/architecture/)
- Feature specs → [docs/features/](docs/features/)
- Coding standards → [docs/standards/](docs/standards/)
- Build & packaging → [docs/operations/](docs/operations/)
- Decision records → [docs/decisions/](docs/decisions/)
- Specs, with their status → [docs/specs/README.md](docs/specs/README.md)
- **Ordered build plan → [docs/roadmap/todo.md](docs/roadmap/todo.md)**

## Common commands

```bash
cargo check --workspace --all-targets   # fast feedback loop
cargo clippy --workspace -- -D warnings  # must be clean before commit
bash scripts/gate.sh                     # fmt, clippy, rustdoc, helper-lock, check-docs: what scripts/ask-ci.sh runs before every push
bash packaging/helper-lock.sh --check    # after touching anything the helper is built from; --bump if it says so (T182b)
git config core.hooksPath .githooks      # once per clone: the pre-commit hook runs the check above for you
cargo fmt --all --check                  # CI's lint job gates on this too; clippy clean != fmt clean
cargo test --workspace                   # unit + integration
cargo nextest run --workspace --all-targets --all-features --profile ci  # what CI runs; optional here, cargo test stays the contract
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --all-features  # intra-doc links, for this OS only
cargo sqlx prepare --workspace -- --all-targets --all-features  # after editing any sqlx::query!
bash packaging/bindings.sh               # after changing a type in mixengine-proto (T56)
cargo run -p mixengine-cli -- status      # drive the daemon from the CLI
node scripts/check-docs.mjs              # documentation links + spec status; --write-index after adding a spec
cd apps/desktop && npm ci && npm run build && npm test && npm run lint     # the desktop frontend
cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings  # its own workspace
```

## Working agreements

- Before implementing a feature, read its spec in `docs/features/` — specs are authoritative.
- Changing a cross-cutting decision requires a new ADR in `docs/decisions/`, not an edit to an
  accepted one.
- Keep the roadmap current: tick tasks in their phase file (`docs/roadmap/phase-*.md`) as they
  land, add follow-ups where they belong in the order, do not append them at the end.
  [docs/roadmap/todo.md](docs/roadmap/todo.md) is the index over those files.
- Specs are written to `docs/specs/` and plans to `docs/plans/`, whatever path a skill defaults
  to. A spec opens with a `status`/`date` header, and the commit that ticks its task `[x]` flips
  it to `implemented` — see [docs/standards/plans-and-specs.md](docs/standards/plans-and-specs.md).
- **Never link to a file under `docs/plans/`.** Those are local-only implementation
  plans, gitignored and absent on every other machine. Link to `docs/specs/` instead —
  see [docs/standards/plans-and-specs.md](docs/standards/plans-and-specs.md).
- When splitting a batch of fixes across subagents, group the work by the invariant the findings
  share, not by the file they sit in — two agents editing around one invariant undo each other.
- Adding to or editing the **blueprint gallery** leaves a second repository stale: `mixengine-packages`
  publishes the same manifests as signed files, and its `publish-blueprints` workflow has to be
  re-run at the new **full** commit SHA. Pushing the change to `master` fires
  `.github/workflows/gallery.yml`, which asks that repository to check what it published — so the
  reminder arrives in minutes, but it is a reminder: nothing publishes on its own. See
  [docs/features/blueprints.md](docs/features/blueprints.md).
- CI is asked for, not automatic: `master` builds itself, any other branch is pushed and then
  requested — see [docs/operations/build-and-release.md](docs/operations/build-and-release.md).
