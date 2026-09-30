---
status: approved
date: 2026-09-30
task: T176g
---

# What is left of MixDB becomes MixLab

## The problem

MixDB was the database client that became MixLab's window in phase 12. Phase 29 renamed the
product, and the window's own text no longer says MixDB: no dictionary under `apps/desktop/src`
(`en`, `vi`) holds the word, and neither does the handbook. Beneath the text it is everywhere:
about a thousand occurrences in tracked files — code identifiers, comments, five live
`localStorage` keys, a URL scheme, the prefix on the window's stderr lines, a line of `mixengined --help`, the
names the window gives temporary objects in a person's database, and the documentation that
describes all of it.

The product is MixLab. A contributor reading `open_in_mixdb.rs` beside a button labelled *Explore
data*, a person reading `mixdb: ignoring a URL nothing here answers` in a terminal, or a database
administrator finding `orders__mixdb_rebuild_1` in their ClickHouse should not have to learn that a
second product once existed.

## The word has two meanings

Every occurrence means one of two things, and the design is the rule for each:

1. **The window, or its `db` module, under its old name.** *"Where MixDB keeps what it remembers
   between runs"*, `databaseOpenInMixDB`, `mixdb-theme`. These become MixLab.
2. **The standalone application that existed before phase 12**, which a person may still have
   installed and whose data MixLab still reads. The `MixDB` keyring service MixLab reads its
   passwords from, the `io.github.haiquang9994.mixdb` directory T104 imports, the `mixdb://`
   scheme the installer takes back and the window refuses (ADR 0047), the `mixdb` extension a
   migration removes and the `extensions/mixdb/config` address its secret had. These stay:
   renaming them would stop MixLab finding what it exists to find. Each keeps a comment saying it
   names the old application.

## D1. Code identifiers and comments

Everything of the first meaning is renamed, in both of the desktop's workspaces and in `crates/`:

- `open_in_mixdb.rs` → `explore_data.rs`, the command `mixengine_database_open_in_mixdb` →
  `mixengine_database_explore_data`, `databaseOpenInMixDB` → `databaseExploreData` — named after
  the button (*Explore data*), which opens a cache as readily as a database, rather than after
  either product.
- The window-internal event `mixdb_disconnected` → `mixlab_disconnected`, in `events.rs`,
  `daemonWatch.ts` and `daemonState.ts`. Its prefix exists so the event *"can never collide with a
  MixEngine `type`, including one added in a later version"*; `mixlab_` keeps that, since the
  daemon names no event after the window. Both ends ship in one build.
- Comments: *MixDB* becomes *MixLab*, or *the `db` module* where the sentence is about the database
  client specifically. `apps/desktop/src-tauri/.cargo/audit.toml`'s comments are comments too.
- `crates/mixengine-proto`'s doc comments that say MixDB (`SecretAddress`, the T84 note in
  `service.rs`) change, and `bash packaging/bindings.sh` regenerates `bindings/` in the same change
  — phase 29 left `bindings/` stale in exactly this way.
- Test fixtures' temporary names (`mixdb-test-…`, `mixdb-sqlite-…`, `mixdb-download-…`,
  `mixdb-absent-…`, `mixdb-new-…`) become `mixlab-…`.
- A document that is not history (D5) and links a renamed file — the headless-host design links
  `open_in_mixdb.rs` — follows the rename; `check-docs` fails on the link otherwise.

The launcher-credential rule in `handoff.rs` is unchanged: it accepts `MIX…_…PASSWORD` names, and
the daemon sends `MIXENGINE_DB_PASSWORD`. Only its doc comment and its test name `MIXDB_PASSWORD`;
both move to `MIXLAB_DB_PASSWORD`, which the same rule accepts (`MIX` + `LAB_DB_` + `PASSWORD`).

## D2. What a person can see

- **The window's stderr prefix** `mixdb:` → `mixlab:`, on every line `instance.rs`, `launch.rs`
  and `handoff.rs` print.
- **`mixengined --help`**: the `--credential-store` help says *"hand-off to MixDB or the desktop
  window"* ([main.rs](../../crates/mixengine-daemon/src/main.rs)); it names the window once.
- **Temporary objects in a person's database.** A table rebuild creates `__mixdb_rebuild_<table>`
  (SQLite) or `<table>__mixdb_rebuild_<millis>` (ClickHouse), and the MySQL script check prepares a
  statement `mixdb_check` from the session variable `@mixdb_check`. These become `mixlab`. Nothing
  looks for the old names: the ClickHouse name carries the clock precisely so that it *"never
  collides with one a previous, failed run left behind"*, and the SQLite one is chosen to avoid any
  name already in the schema, inside a rebuild that commits or rolls back as one transaction. A leftover under the old name is what it already was — the person's table,
  visible and theirs to drop.

## D3. The keys a person's machine already holds

`localStorage` holds five live keys of the first meaning: `mixdb-theme`, `mixdb-accent`,
`mixdb-lang`, `mixdb-modules` and `mixdb-session`. They become `mixlab-…`, **with a one-time move**,
so nobody loses a theme, a language, the modules they chose or the tabs they had open.

Two more carry the prefix and are **not** moved, because they are already on their way out:
`mixdb-glass` is in `RETIRED_KEYS` and removed on sight, and `mixdb-terminal-font-size`
(`LEGACY_FONT_SIZE_KEY`) is read once by `settingsStore.ts` into the terminal's settings file and
deleted. Moving the second would hide it from the one reader that still wants it. Both keep their
old names, as keys earlier builds wrote, and are listed as such in D6.

This reverses a decision written beside the keys in
[profiles.ts](../../apps/desktop/src/shell/profiles.ts): *"The prefix is the origin's rather than
the product's; renaming all five is a migration for nothing, and two prefixes in one origin is
worse than an old name."* Both halves are answered:

- **Not for nothing.** The product's name is the reason, and it is the owner's decision to make.
- **Never two prefixes.** The move runs once per page load, before anything reads a key, and
  leaves no old key behind.

**Where it runs is the whole difficulty.** Three readers come before any code a module could call:

- `public/theme-preload.js`, a classic script both `index.html` and `tray.html` load before the
  bundle, reads the theme so a dark window does not flash white;
- `theme.ts` calls `clearRetiredKeys(localStorage)` when it is imported, and `i18n` reads the
  language as the provider mounts;
- the tray is a page of its own, `tray.html`, which reads the theme, accent, language and modules
  without the main window having run at all — the tray can be the first page of a run.

So the move is **a classic script of its own, `public/storage-keys.js`, loaded first by both pages**,
ahead of `theme-preload.js`, under the same `script-src 'self'` that already admits that file. For
each pair: when the new key is absent and the old one present, copy the value across; then remove
the old key either way. It is idempotent, and whichever page runs it first leaves nothing for the
other. The pair list lives in that script, and a test holds each new name equal to the constant its
module reads (`theme.ts`, `i18n/index.tsx`, `profiles.ts`, `session.ts`, `preferencesSync.ts`,
the tray's `FOLLOWED` set), so the two cannot drift.

What else follows:

- `LEGACY_SHELL_KEYS` (profiles.ts's "this profile has been used before") names the new keys: by
  the time it is read the move has run. It drops `mixdb-glass`, which the move never touches and
  `RETIRED_KEYS` may already have removed.
- **The tray reloads once, harmlessly.** It reloads on a `storage` event for a key it follows; the
  main window's move writes a followed key only when the tray has not already moved it, and a
  reload is what the tray does for any such change.
- **Sync is unaffected.** `preferencesSync.ts` sends records by id (`theme`, `accent`, `language`,
  `modules`), never by storage key, so a machine on this build and one on the last agree.
- **A downgrade loses these preferences**: an older build reads `mixdb-…`, finds nothing, and
  starts on defaults. Accepted — nothing installs an older MixLab on its own, and the loss is
  settings, not data.
- `demo/args.mjs` seeds the new keys.

## D4. The REST preview's scheme

`mixdb-preview` → `mixlab-preview`: `SCHEME` in `modules/rest/preview.rs`, `PREVIEW_SCHEME` in
`modules/rest/api.ts`, and **both forms** in `tauri.conf.json`'s `frame-src`, for `csp` and
`devCsp` — `mixlab-preview:` and `http://mixlab-preview.localhost`, which Windows and Android
rewrite it to. The scheme serves one response body per request and keeps no state, so nothing moves.

## D5. Documentation

**Living documents** describe the product as it is, and change by the rule of the two meanings:
`apps/desktop/README.md`, `apps/desktop/CLAUDE.md`, `docs/README.md`, `docs/architecture/`
(`data-model.md`, `desktop/backend.md`, `desktop/overview.md`), `docs/features/`
(`client-surface.md`, `extensions.md`), `docs/operations/build-and-release.md`,
`docs/standards/desktop/`, specs that are `draft` or `approved`, and the open tasks and *Where we
are* of the roadmap. Several of their mentions are the second meaning — `extensions.md`'s *"Its one
entry was MixDB, which became MixLab"* is history told in a living page — and stay.

**History is not edited**: specs that are `implemented`, `superseded` or `abandoned`, accepted ADRs
(including `docs/decisions/desktop/`), `docs/reviews/`, `CHANGELOG.md`'s released entries,
`apps/desktop/CHANGELOG.md` (MixDB's own history, frozen at 0.0.33), and the text of ticked roadmap
tasks. They describe what was true when they were written, and
[plans-and-specs.md](../standards/plans-and-specs.md) already forbids editing them. A link to a
historical spec keeps its path, `mixdb` in the filename included.

## D6. It stays done

Phase 29's own lesson was that `check-docs` reads only the markdown under `docs/`, and three live
references survived it. So this task ends with a check rather than a sweep:
`node scripts/check-names.mjs` reads every tracked text file (`git grep -I`) and fails on `mixdb`,
in any case, except:

- **History, by where it lives**: `docs/decisions/`, `docs/reviews/`, `docs/roadmap/`, both
  changelogs, the generated `docs/specs/README.md`, and a spec whose header says `implemented`,
  `superseded` or `abandoned`. The roadmap is exempt whole, because a phase file mixes ticked and
  open text line by line; its living half is kept by review, not by this check.
- **Link targets** in markdown — `](…)` — which are paths, not words.
- **The second meaning**, listed in `scripts/legacy-names.json` as
  `{ "path", "count", "reason" }`: the file, how many occurrences it holds, and which part of the
  old application they name. The check fails when a listed file holds more *or fewer* than its
  count, so a new leftover cannot hide in a listed file and a removed one has to be taken off the
  list. The list only shrinks — the shape `client-surface-exceptions.json`'s `knownGaps` already
  has.

This design itself says MixDB throughout; it lands in the commit that flips it to `implemented`,
which is also the commit the check lands in, so it is history by then.

The check runs in `scripts/gate.sh` and in CI's lint job, beside `check-client-surface.mjs`.

## MixLab

**Nothing a person sees in the window changes, and that is the requirement.** After the first start
of this build — through the main window or through the tray, whichever comes first — the theme,
accent, language, modules and open tabs are what they were (D3); the REST
preview renders as it did (D4); *Explore data* opens what it opened. What changes is outside the
window: the stderr prefix, one line of `mixengined --help`, and the names of temporary objects in a
person's database while an operation runs (D2).

## Testing

- `storage-keys.js`, run in `node:vm` against a fake `localStorage`: old key only → moved and old
  removed; both present → the new value kept and the old removed; neither → nothing written; a
  second run changes nothing; an unrelated key, `mixdb-glass` and `mixdb-terminal-font-size` are
  untouched.
- Each new name in `storage-keys.js` equals the constant its module reads (the list in D3).
- `index.html` and `tray.html` load `storage-keys.js` before `theme-preload.js` (a test over the two
  files).
- The existing REST preview tests, against the new scheme; the CSP names `mixlab-preview` in both
  forms, for both policies.
- The SQLite and ClickHouse rebuild tests and the MySQL script check, against the new names.
- `handoff.rs`: `MIXLAB_DB_PASSWORD` is accepted, and a name outside the namespace is still refused.
- `check-names.mjs`: a tracked file saying MixDB outside the allowed set fails, naming the file and
  line; a historical spec, a link target and a listed file at its count pass; a listed file above or
  below its count fails.
- The whole gate: `npm run build`, `npm test`, `npm run lint` in `apps/desktop`; `cargo fmt --check`
  and `cargo clippy --locked --all-targets -D warnings` in `src-tauri`; `bash scripts/gate.sh` at
  the root, with `bindings/` regenerated.
- By hand, once, on Windows: MixLab from the last release with a dark theme, Vietnamese, a module
  switched off and two tabs open, updated to this build and started from the tray first, keeps all
  four, with no white flash on the first paint.

## Not in this design

- **The old application's names** (the second meaning). They stay while MixLab still reads what the
  old application left behind. Retiring them is the day MixLab stops importing from MixDB, which is
  its own decision.
- **`tauri_app_lib`**, the desktop crate's library name, and any other name that says neither
  product. This task is about one word.
- **The engine's names.** `mix`, `mixengined`, `MIXENGINE_HOME` and the crates are MixEngine by
  ADR 0044, not leftovers.

## Tasks

**T176g** in [phase 29](../roadmap/phase-29-one-name-to-find-it-by.md): D1–D6 in one change, and
this design flipped to `implemented` in the commit that ticks it.
