# Phase 11 — The desktop app comes home

*Goal: the standalone client's application builds, tests and packages from this repository, and a user notices
nothing.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-08-the-desktop-client-in-this-repository-design.md](../specs/2026-09-08-the-desktop-client-in-this-repository-design.md),
on [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md).

---

**This phase moves code and changes no behaviour.** Every task here is a relocation, a replaced
copy, or a CI job; the window a person opens at the end of it is the standalone client 0.0.33 under another roof.
The phase is deliberately short so that the import commit — a hundred thousand lines — carries
nothing that needs reviewing as code.

- [x] **T100** ADR 0027 supersedes ADR 0011, and the documentation says so everywhere it said the
      opposite. `CLAUDE.md`'s architecture paragraph and workspace layout, `README.md`,
      [build-and-release.md](../operations/build-and-release.md)'s *"Rust only"*,
      [client-surface.md](../features/client-surface.md)'s opening,
      [extensions.md](../features/extensions.md)'s the standalone client section, `packaging/README.md`, and
      `docs/README.md`'s table gain a `desktop/` row. The "Non-negotiable rules" gain the two
      rules a desktop crate needs — it reaches the daemon only through the API and the published
      contract, and its toolbox modules never reach the daemon at all — and lose *"no frontend
      toolchain"*. The root `rust-toolchain.toml` and `rust-version` move to 1.98.0 in a commit of
      their own, so a lint the newer compiler adds is fixed here and not inside the import.

- [x] **T101** the standalone client arrives under `apps/desktop/` with its history, as a `git subtree add`. The
      Rust half is a Cargo workspace of its own, excluded from the root one (design, D1 and D5 of
      the ADR); its own pin file is deleted. `AGENT.md` becomes `apps/desktop/CLAUDE.md`, `.agent/`
      becomes `docs/README.md`, the 26 specs join `docs/specs/`, and the standalone client's
      changelog is frozen as `apps/desktop/CHANGELOG.md`. `apps/desktop/src-tauri/tests/layering.rs`
      fails on any `path` dependency other than `mixengine-proto` and `mixengine-platform`, with
      `mixengine-testkit` allowed under `[dev-dependencies]` alone. **The version stays the standalone client's
      through this phase** — the design's D6 says why the workspace-version check waits for T104.
      The standalone client's release scripts are deleted rather than moved. After this commit `mixnz/<old>`
      receives one more: a README pointing here.

- [x] **T102** The two things the standalone client kept in step by hand become imports. The vendored
      `api/types/` goes and the `@mixengine/api` alias resolves to `bindings/` (D4); the pipe-name
      fingerprint, the named-pipe dial and the owner check copied out of `mixengine-platform` go,
      and the desktop crate takes `ipc::Endpoint` and `ipc::Connection` from it (D5) — with
      `windows-sys` leaving its manifest. The pinning test moves with the function.
      **What this task found and did not fix**: `[daemon] ipc_path` in `config.toml` is parsed by
      `mixengine-core` and used by nothing, so the standalone client was honouring a knob the daemon ignores; the
      desktop stops reading it, and the key itself is a debt for whoever next touches `Config`.
      **(P)** — the endpoint is computed differently on each OS, and the transport test binds a
      real `Listener` and dials it on all three.

- [x] **T103** CI knows the window exists. A `desktop` job on ubuntu runs the frontend's build,
      tests and lint and the nested workspace's clippy and tests, on every request and every tag;
      `release` waits for it. The five `build` legs set up Node, build the frontend, and run
      `npm run tauri -- build --no-bundle` (`--bundles app` on macOS) on the runner itself — the
      two Linux legs' hosts move to `ubuntu-22.04` and `ubuntu-22.04-arm`, because the manylinux
      container cannot link WebKitGTK 4.1 (D12). `cargo audit` runs in the `desktop` job against
      the crate's `.cargo/audit.toml`. Nothing is staged or packaged yet; the executable is uploaded
      as its own artifact so M11 can be checked by hand on each OS.
      **Three things the first green run taught.** `createUpdaterArtifacts` is off in
      `tauri.conf.json`, or bundling on macOS demands a signing key the standalone client's updater owned (run
      34216184433). The `build` legs set `MIXENGINE_RELEASE=1` for the window, as `stage.sh` does
      for the four binaries: without it the window's `mixengine-platform` names the home
      `MixEngine-dev` (ADR 0024) and reports the daemon at `MixEngine` as not running — the
      artifact of run 34244691840 did exactly that against a daemon `mix status` reached, and the
      crate's live test computed the wrong pipe until the switch was set. And a new toolchain pin
      is a new cache key: the Windows `test` leg was cut off at 30 minutes building cold, so the
      job has 45 and saves its cache from a red leg too. Found by the same run and fixed beside it:
      **T33c**, two bootstraps sharing `/tmp` deleting each other's temporary tables.

**Milestone M11** — the desktop application builds in this repository's CI on three operating
systems, its 1547 tests and its clippy are green there, and a build downloaded from the `build`
artifacts opens against a running daemon exactly as the standalone client 0.0.33 does.

**Reached 2026-09-08**, run 34244691840 on `desktop-app-comes-home`: nineteen jobs green, the
`desktop` job with 1849 frontend tests, 528 crate tests and the two layering tests. Checked by hand
on Windows against the installed daemon 0.0.6: the window from `desktop-windows-latest` opened, its
toolbox tabs worked against real servers, and against a sandbox home it drove the daemon — services
started, the web server switched — while the MixEngine tab reported the default home's daemon as
not running, which is the release switch above; a local release build with the switch set, the
build the corrected `build` legs produce, showed that daemon's Dashboard.
