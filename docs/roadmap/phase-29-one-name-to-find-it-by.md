# Phase 29 — One name to find it by

*Goal: the thing people open, the thing they search for and the repository they land in all have
one name, and the engine keeps its own where it is still the right one.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done.

Decision: [ADR 0044](../decisions/0044-mixlab-is-the-product-and-mixengine-is-the-engine.md), which
supersedes the naming paragraph of [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md).
It carries the scope table; this phase is the order the work happens in.

---

- [x] **T176a** The repository is `mixnz/mixlab`. Renamed on GitHub, `repository =` in the root
      `Cargo.toml` follows, and every URL in the tree that names it. **Find out what happens to
      `https://mixnz.github.io/mixengine/` and write the answer down** — a repository rename
      redirects the git remote and the web UI, and Pages is the one that may not follow. ADR 0044
      accepts losing the inbound links; it does not accept not knowing.

      **Measured on 2026-09-20, straight after the rename:**

      | Asked for | Answer |
      | --- | --- |
      | `github.com/mixnz/mixengine` | `200`, redirected to `github.com/mixnz/mixlab` |
      | `github.com/mixnz/mixengine/releases/latest/download/latest.json` | `200`, the asset itself |
      | `mixnz.github.io/mixengine/` | **`404`** |
      | `mixnz.github.io/mixlab/` | `200` — GitHub moved the site with the repository |

      So **v0.0.1–v0.0.6 keep updating**: the `updates::feed::DEFAULT_URL` compiled into them still
      resolves through the redirect. An earlier draft of ADR 0044 decision 5 asserted the opposite
      and was wrong; this is why that decision asks for a measurement instead of a prediction. The
      handbook's old address is the one thing genuinely lost, as the ADR's *Hard, and accepted*
      paragraph says. The site at the new address still serves the deployment from before this
      task — the next run of `pages.yml` rebuilds it with the new `BASE_URL`.
- [x] **T176b** `README.md` says what the product is called. MixLab is what you download; MixEngine
      is the engine inside it and the second download, for a machine with no screen. The sentence
      that answers *"do I need both?"* is one sentence and it is above the fold.
- [x] **T176c** `docs/guide/` — 32 files, 398 occurrences, **read one at a time**. Where the word
      names the product it becomes MixLab; where it names the daemon, the CLI, the service manager
      or the headless distribution it stays. English and Vietnamese move together, because the
      handbook is one corpus ([ADR 0021](../decisions/0021-the-handbook-is-one-corpus-published-three-ways.md))
      and `mix docs` compiles both. A blind substitution here produces "the MixLab daemon", which is
      a worse sentence than the one it replaced.
- [x] **T176d** What the packaging says about itself, and nothing it is built from. The `.deb`
      `Homepage:` and `packaging/README.md` follow the new repository; `tauri.conf.json`'s
      `productName` and `MIX_WINDOW_APP` already say MixLab and are left alone. **Release artifact
      names, the `mixengine/` directory inside the payload, `Package: mixengine`,
      `Programs\MixEngine` and `install::program_dirs` are not touched** — ADR 0044 decision 5 and
      decision 2 make all five identifiers, and renaming any of them ripples through `feed.sh`,
      `feed-check.sh`, `sign.sh` and `core::install` to buy a file name.

      **T176f changed its answer on artifact names and `Package: mixengine`**
      ([ADR 0049](../decisions/0049-a-download-is-named-after-what-it-installs.md)): a file that
      carries the window is named `mixlab-…`, and the Linux packages are `mixlab`. The payload's
      `mixengine/` directory, `Programs\MixEngine` and `install::program_dirs` still stand.
- [x] **T176e** `mixnz/mixengine-sync` is renamed `mixnz/mixlab-sync` before it has a first commit,
      and the working copy beside it. Phase 30 is written against the new name already.

      **That repository no longer exists.** It was deleted without ever receiving a commit, and the
      server moved into `server/` in this one —
      [ADR 0046](../decisions/0046-the-sync-server-lives-beside-the-client-it-serves.md). Nothing
      here is wrong about what was done at the time; this note is so that a reader does not go
      looking for it.
- [x] **T176f** What a person downloads is named after what it installs — [ADR 0049](../decisions/0049-a-download-is-named-after-what-it-installs.md),
      superseding ADR 0044 decision 5 and T176d's answer on artifact names and `Package: mixengine`.
      Every artifact carrying the window becomes `mixlab-…`, the `.deb` and `.rpm` become the
      package `mixlab` taking over from `mixengine`, and the headless archives, the helper and every
      identifier inside an artifact keep their names.
      [Design](../specs/2026-09-23-t176f-a-download-is-named-after-what-it-installs-design.md).
- [ ] **T176g** What is left of the name MixDB goes, under the window. No string a person reads
      says MixDB any more; about a thousand places beneath it still do. Code identifiers and
      comments (`databaseOpenInMixDB`, `open_in_mixdb.rs`, `MIXDB_PASSWORD`) and the living
      documentation become MixLab; the keys a person's machine already holds (`mixdb-theme`,
      `mixdb-lang`, `mixdb-accent` and their siblings, `mixdb-preview.localhost`) move with a
      one-time migration, so nobody loses a setting; the read-only `MixDB` keyring service stays,
      because it is how a standalone MixDB's passwords are still found, and says so. Implemented
      specs and accepted ADRs are history and are not edited; `scripts/check-names.mjs` keeps it
      that way.
      [Design](../specs/2026-09-30-t176g-what-is-left-of-mixdb-becomes-mixlab-design.md).

**Milestone M29** — **met.** `mixnz/mixlab` serves the repository and `mixnz.github.io/mixlab/` the
handbook; `README.md` answers *"do I need both?"* above the fold; no name among `mix`, `mixengined`,
`MIXENGINE_HOME`, `Programs\MixEngine`, `Package: mixengine`, the payload's `mixengine/` directory
or the ten crates moved, and a grep for `mixlabd`, `MIXLAB_HOME` and `Programs\MixLab` returns
nothing. `cargo fmt`, `clippy --workspace --all-targets -D warnings`, `RUSTDOCFLAGS="-D warnings"
cargo doc`, `cargo test -p mixengine-cli --test docs`, `--test corpus`, the 115 `mix` unit tests and
`node scripts/check-docs.mjs` are all green.

**Three things this phase deliberately did not do.**

- **The `mix` command line keeps its own vocabulary.** Roughly seventy strings in
  `crates/mixengine-cli/` say MixEngine — *"MixEngine command line"*, *"Update MixEngine itself"*,
  *"starts with MixEngine"*. Under [ADR 0044](../decisions/0044-mixlab-is-the-product-and-mixengine-is-the-engine.md)
  decision 3 that is correct rather than missed: `mix` is the engine's command line and the headless
  distribution *is* MixEngine. Only the two strings that point at the handbook moved, because the
  handbook is MixLab's now. Renaming the rest would be a decision of its own, not a sweep.
- **MixLab's tray labels are untouched.** `services.md` quotes **Stop MixEngine** and **Start
  MixEngine when I log in** because those are the words in the window and in `mix autostart`.
  Changing the documentation alone would make it describe a menu that does not exist.
- **`Programs\MixEngine` did not move**, and neither did the NSIS `UNINSTALL_KEY` or
  `Software\MixEngine`. The installer's `NAME` and `PUBLISHER` are display strings and did; an
  install from before this phase therefore still finds itself.

**The sweep reached further than `check-docs` can see.** That script reads the markdown under
`docs/`, so ticking this phase on its word left three live references behind: MixLab opened the
install page at the old Pages address — a `404` after the rename — `shell/version.ts` still asked
GitHub about `mixnz/mixengine`, and `bindings/` had gone stale, because it is generated from
`mixengine-proto`'s doc comments and those moved earlier in this phase. A `git grep` over tracked
files is what found all three; it is the check this phase should have run first.

**Two things the plan had wrong, found by executing it.** `docs/guide/en/cli.md` is generated by
`bash packaging/docs.sh --reference` and must never be hand-edited — the plan said to `sed` it. And
the published site's own branding is not in the corpus at all: it lived in
`crates/mixengine-docs/templates/page.html` and `examples/support/generate.rs`, including
`index.json`'s `"product"`, which `for-agents.md` prints and `features/client-surface.md` describes.
