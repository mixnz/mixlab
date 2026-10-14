---
status: draft
date: 2026-10-14
task: T207j
---

# `mixengine-core`'s `Error`, one enum per subject (design)

`crates/mixengine-core/src/lib.rs` is 1,943 lines, and lines 92–1935 are one `enum Error` with
135 variants: 971 lines of doc comment and 878 of code. An enum cannot span files, so this is the
one file in phase 45 that cannot shrink by moving code. Decided on 2026-10-14, after a survey, was
the direction the crate already took once: `Error::Graph(#[from] services::GraphError)`. Most
variants move into **nine enums, one per subject**. Each sits in the module of its subject and is
wrapped by one transparent variant of `Error`. Variant names do not change (agreed on 2026-10-14).

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207j. This is a
change to code, like T207k and T207l, and D4 says what it must not change.

## D1 — the groups

Measured over the workspace. "Sites" counts the places outside `lib.rs` and the daemon's
`error.rs` that name one of the group's variants. Each site is an edit.

| Enum | File | Variants | Lines | Sites (files) |
| --- | --- | --- | --- | --- |
| `BlueprintError` | `blueprints/error.rs` | blueprint manifest, schema, name, source, signature, key; the three `Archive*` a blueprint archive meets (11) | 118 | 45 (9) |
| `ExtensionError` | `extensions/error.rs` | every `Extension*`, `UnreadableExtensionRow`, `UnknownExtensionSchema`, the three `Registry*` (18) | 231 | 91 (14) |
| `InstallError` | `install/error.rs` | every `Index*`, `Timestamp`, every `Artifact*`, `ArchiveUnreadable`, `UnsafeArchiveEntry`, `MissingFromArtifact`, `SmokeTestFailed`, `InstallCancelled`, `KeytoolRefused` (19) | 265 | 109 (13) |
| `UpdateError` | `updates/error.rs` | every `Update*`, `InstallerUnavailable`, every `Helper*`, `SettingUnwritable` (11) | 133 | 24 (3) |
| `ServiceError` | `services/error.rs` | state transitions, rows and documents, recipes and settings, templates, `Unrunnable`, declared/data-directory/ports, `*ProvidesNothing` of services and packages, the database account variants (21) | 280 | 128 (30) |
| `JobError` | `jobs/error.rs` | every `Job*` and job-row variant (6) | 84 | 20 (1) |
| `ProjectError` | `projects/error.rs` | `Manifest`, `ManifestEdit`, `NotAbsolute`, project rows and names, domains and TLDs, doc roots, `HttpsRedirectNeedsHttps`, site rows (16) | 198 | 84 (11) |
| `RuntimeError` | `runtimes/error.rs` | `AlreadyInstalled`, `RestoreRefused`, `UnrecordedInstall`, `AlreadyRecorded`, `ExtensionCompiledIn`, package and runtime rows, the three runtime-resolution variants, `RuntimeProvidesNothing`, `ShimMissing` (13) | 163 | 84 (22) |
| `ElevationError` | `elevation/error.rs` | every `Elevate*`, `OpUnwritable` (7) | 97 | 36 (4) |

**Staying in `Error`**: `NotFound`, `Io`, `Config`, `ConfigEdit`, `Database`, `Backup`,
`Migration`, `IncompatibleDatabase`, `HomeHasNoId`, `Graph`, `Certificate`, `EmptyHome` and
`Platform`. Each is either used by every subject or is about the home itself. Next to them come the
nine new wrappers. `lib.rs` comes to about 350 lines.

The exact membership is the table in the survey script and is applied by it. A variant whose only
users are in one subject goes with that subject even if its name says otherwise.
`JobOutcomeUnwritable` and `SettingUnwritable` are examples: each goes where it is written.

## D2 — the shape

Each enum looks like `GraphError`: `#[derive(Debug, thiserror::Error)]`, `#[non_exhaustive]` like
`Error`, and the variants moved word for word with their doc comments and `#[error]` attributes.
`Error` gains:

```rust
/// … — see [`ExtensionError`].
#[error(transparent)]
Extension(#[from] extensions::ExtensionError),
```

`lib.rs` re-exports the nine (`pub use extensions::ExtensionError;` …), so a crate outside writes
`mixengine_core::ExtensionError`.

`#[error(transparent)]` forwards both `Display` and `source()` to the inner error. So the message a
log prints, the chain `anyhow` walks, and the wire error's `chain(self)` are unchanged character for
character.

## D3 — rewriting the sites

The edit is the same everywhere, in expressions and in patterns:

```text
<path>::Error::ExtensionField { … }   →   <path>::Error::Extension(<pkg>::ExtensionError::ExtensionField { … })
Core::IndexTransport { .. } | Core::ArtifactTransport { .. }
                                       →   Core::Install(InstallError::IndexTransport { .. }) | …
```

A wrapped variant is still a valid expression and a valid pattern, so constructions, `matches!`,
`if let` and match arms are all rewritten alike, and every arm keeps its place in its `match`. A
script does it with brace matching (strings and comments skipped). Within core it names the enum by
its `crate::` path; elsewhere it names it `mixengine_core::`. Doc-comment links to a moved variant
(`[`Error::ExtensionField`]`) are rewritten to the new path, and rustdoc checks each one.

**The daemon's mapping** (`mixengine-daemon/src/error.rs`, 87 arms naming a moved variant) gets the
same rewrite, so it stays one flat `match` with its `_ => internal` fallback at the bottom. Every
variant is therefore classified exactly as it was. A second step would make classification
**complete by construction**: a nested `match` per group with no wildcard, where a variant that used
to fall through is listed explicitly under `internal`, and the nine enums are not
`#[non_exhaustive]` so the daemon may match them exhaustively. A variant added later would then fail
to compile until someone gives it a code. An arm that joins variants of two groups with `|` cannot
survive that, and the survey found exactly one: the four `Unreadable*Row` variants of runtimes,
packages, projects and sites, all `internal` with the hint "the row was written by a different
version of MixEngine, or edited by hand". It becomes two arms, one per group, and the hint becomes a
constant they share. The second step therefore runs, as its own commit after the nine groups.

## D4 — what the change must not alter

- **Every message and every source chain**, by `#[error(transparent)]` (D2). The set of string
  literals across the touched crates is the same before and after.
- **Every wire error**: the same code and hint for every variant. In the flat form this holds by
  construction, because each arm's pattern names the same variant. It is checked by the daemon's 22
  `error.rs` tests and its integration suites, and by `mixengine-cli`'s, which assert codes and
  hints end to end.
- **Every caller's behaviour**: a `match` that named a variant still names it, now one layer deeper.
- **Public API outside the workspace**: none. `apps/desktop/src-tauri` and `server/native` do not
  depend on `mixengine-core`.

## D5 — commits

One commit per group, smallest first: `JobError`, `ElevationError`, `UpdateError`,
`BlueprintError`, `RuntimeError`, `ProjectError`, `ExtensionError`, `InstallError`,
`ServiceError`. Each commit moves the variants, adds the wrapper and the re-export, rewrites every
site, and compiles. Each gets `cargo clippy --workspace --all-targets -D warnings`, rustdoc
`-D warnings`, `cargo fmt --check`, and the unit-test counts of every crate by `--list` (unchanged).
Then the D3 second step, if the survey allows it. Then `docs/standards/rust.md` gains one
sentence: a crate's `Error` may wrap per-subject enums the way `GraphError` is wrapped. Then the
roadmap, with `lib.rs` out of the baseline, which is then empty. The full workspace tests run on
Windows and on Linux (WSL) before the push.

## MixLab

MixLab has no part in this. The window reaches the daemon through the wire, and every wire error
stays as it was. `apps/desktop` does not depend on `mixengine-core`.

## Not in scope

- Renaming variants to drop a prefix the enum now carries (`ExtensionError::ExtensionField` →
  `ExtensionError::Field`). That was considered and set aside on 2026-10-14.
- Narrowing function signatures to return a subject's error (`Result<T, ExtensionError>`). Every
  signature keeps `crate::Result`.

## Is there an ADR here? No

`docs/standards/rust.md` asks each library crate for its own `Error` enum. `mixengine-core` still
has one, and nested enums are already its practice (`GraphError`).

## Documentation, when it lands

`docs/standards/rust.md` (the sentence in D5), the phase file ticks T207j and M45, and this spec
flips to `implemented`.
