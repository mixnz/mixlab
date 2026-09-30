# Phase 35 — An index that stays small

*Goal: the package index costs a home one small request when nothing was published, and only the
kinds it uses when something was — however many versions the index has grown to.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-30-t196-mixengine-reads-index-schema-2-design.md](../specs/2026-09-30-t196-mixengine-reads-index-schema-2-design.md).

---

- [x] **T196a** The schema 2 documents and their decoding into the existing `Package` and
      `Artifact`; `Index` as a view over the kinds that were asked for.
- [x] **T196b** `MockRegistry` serves the schema 2 set beside `index.json`, its half-published
      states, and a log of what was requested.
- [x] **T196c** `index::PackageIndex`: signature first, the root, kinds by hash, the cache and its
      invariant, what is kept in memory, the schema 1 cache carried across, and the fallback for a
      source with no schema 2.
- [x] **T196d** The daemon asks for kinds: `Fetcher`, every caller, `unavailable` on both
      catalogues with its line in `mix` and in MixLab's Packages screen, and the cleanup list.
      Bindings.
- [x] **T196e** `runtime-packaging.md`, the comments in `index.rs`, the published-index test
      reading the set and comparing it with `index.json`, the changelog, and the design flipped to
      `implemented`.

**Milestone M35**: a daemon with a cached index and nothing published makes one request for a
signature every six hours and no other; a fresh home that lists runtimes fetches the root and six
kind files, not eighteen; and a home upgraded from a schema 1 cache cannot be offered an older index
than it held.
