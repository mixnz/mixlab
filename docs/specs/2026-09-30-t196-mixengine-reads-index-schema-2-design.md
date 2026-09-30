---
status: approved
date: 2026-09-30
task:
  - T196a
  - T196b
  - T196c
  - T196d
  - T196e
---

# T196 — MixEngine reads index schema 2

Roadmap tasks T196a–T196e, in [phase 35](../roadmap/phase-35-an-index-that-stays-small.md).
2026-09-30.

## The problem

`index.json` is cumulative by promise: no version is ever removed. The client
(`crates/mixengine-core/src/index.rs`) downloads it whole every six hours with no way to ask whether
anything changed, and **every call** re-reads, re-hashes and re-parses the whole cached file — a
single `runtime.list_available` does that once, and an install does it again at each step that
consults the index.

`mixengine-packages` has published a second encoding since 2026-09-30: one small signed root,
`index-v2.json`, and one unsigned file per kind named by hash in that root. Its design, and the
contract a client must keep, is
[2026-09-30-index-schema-2-design.md](https://github.com/mixnz/mixengine-packages/blob/HEAD/docs/superpowers/specs/2026-09-30-index-schema-2-design.md)
("What a client must do"). No client reads it yet. This is the design of the one that does.

## What was measured, and what is an estimate

**Measured on 2026-09-30** against the `index` release, one request each from one machine with
`curl` (a fresh connection per request). These are readings, not a benchmark.

| | Reading |
| --- | --- |
| `index-v2.json` | 1,914 bytes, 18 kinds, `generated_at` 2026-09-30T15:09:10Z |
| `index-v2.json.minisig` | 308 bytes |
| The 18 kind files | 151,117 bytes in all (the sum of the root's `size`s); largest `php` 30,670, smallest `memcached` 1,121 |
| `index-v2-php.json` | 17 packages, 85 artifacts, 21 shapes; its sha256 and length match the root |
| Keys its shapes carry | `provides`, `requires`, `extension_dir`, `extensions` and no other. Only this one kind file was downloaded and read |
| `index.json` | 339,764 bytes, schema 1, 119 packages, 635 artifacts, **the same `generated_at` as the root** |
| Its 635 `url`s | every one is what D12's pattern composes; 448 `tar.zst`, 159 `zip`, 28 `tar.gz`; three `os` values and two `arch` values |
| `extensions.json` | 8,457 bytes, 3 entries |
| One asset | one `302` and then a `200`; 0.66–1.06 s each |
| An asset that does not exist | `404` |

**Estimates, none of them ours**: about 16 MB for `index.json` and about 2.5 MB across the kind
files in ten years, and "a few hundred KB" for the largest kind file then. They are the packaging
repository's, from upstream patch cadence, and its design says so.

**Not measured, and said where it matters below**: what the parsed catalogue weighs in the daemon's
memory (D6), and how long a first listing takes through `reqwest` with a reused connection (D8).

**Counted in this repository**: `MockRegistry::publish_asset` has 30 call sites in 22 files, and
every one passes `Packed::path()`; no test asserts the URL an install recorded (D13).

## Principle

**Nothing on disk is believed because it was believed once.** The root is verified against the
compiled-in key every time it is read from disk, and a kind file is hashed against that verified
root every time it is read from disk — the trust boundary `index.rs` already describes, one level
down.

**A failed refresh changes nothing.** The old root and the old kind files stay, and the answer is
`Stale`, as a failed fetch is today.

**The clients learn nothing new.** What is installable, and what could not be read, is composed by
the daemon.

## D1. Two clients over shared primitives

`Client<D: Document>` is the right shape for one signed document and stays as it is. It keeps
reading `extensions.json` and `latest.json`, and it becomes the reader of schema 1 `index.json` for
D9's fallback. Its behaviour does not change in this work.

The package index gets a client of its own, `index::PackageIndex`, because a root plus files named
by hash is not a `Document`: there is more than one fetch, more than one cache file, and a rule
between them. What the two share moves into free functions both call — the signature check, the GET
with its timeout and size cap — so there is still one verification path.

```
index.rs            Client<D>, Document, Freshness, Catalogue, the shared primitives
index/format.rs     Package, Artifact, Target, Timestamp … and the Index view (D2)
index/schema1.rs    the index.json document, as a Document
index/schema2.rs    Root, KindFile, and decoding a kind file into Vec<Package>
index/packages.rs   PackageIndex: the contract, the cache, the memory
```

## D2. `Index` becomes a view over the kinds that were asked for

Today `Index` is the document: `schema`, `generated_at`, and every package. Fifteen call sites in
the daemon hold a `&Index` and ask it `artifact`, `select`, `installable`, and
`requirements::judge` and `adopt::walk` take one.

`Index` keeps that role and those methods, and stops being a document:

```rust
pub struct Index {
    generated_at: Timestamp,
    kinds: BTreeMap<String, Arc<[Package]>>,   // the kinds this view was built for
    missing: Vec<Missing>,                     // asked for, could not be read (D5)
}
```

- `select`, `artifact`, `installable`, `installable_for` keep their signatures.
- The public fields become methods: `packages()`, an iterator over the kinds in the view, and
  `generated_at()`. Outside tests, two callers read `packages` today — `runtimes::offered` and
  `adopt::walk` — and none reads the other two fields.
- `Index` no longer derives `Serialize`/`Deserialize`. The schema 1 document is `schema1::Document`,
  and `Index::from_packages(generated_at, packages)` is what it, and every test that builds an index
  from JSON, goes through.
- A kind file is **decoded into the existing `Package` and `Artifact`**: the shape an artifact
  points at is copied into it and the URL is composed. `Artifact` is used as an owned value by the
  installer, the markers, the update feed and the extension installer, and none of them changes.
- Building a view clones an `Arc` per kind, never a package.

**A view answers only for the kinds it was built for.** A kind the root does not name is a kind
that does not exist, and the view holds it as empty. A kind nobody asked for is not in the view, and
asking the view about it is a bug in the caller: `debug_assert!`, and nothing in a release.

## D3. What callers ask

```rust
impl PackageIndex {
    /// These kinds, from memory or the cache while the root is fresh, from the network otherwise.
    pub async fn kinds(&self, kinds: &[&str]) -> Result<Catalogue<Index>>;
    /// The same, asking the network whatever the age of the root.
    pub async fn refresh(&self, kinds: &[&str]) -> Result<Catalogue<Index>>;
    /// Every kind the root names.
    pub async fn all(&self) -> Result<Catalogue<Index>>;
}
```

`Fetcher::index` becomes a `PackageIndex`. Every caller already knows its kinds:

| Caller | Asks for |
| --- | --- |
| `runtime.list_available` | the filter's kind, or `RuntimeKind::ALL` — six kinds, not eighteen |
| `package.list_available` | the filter's package, or every recipe's package |
| install, requirements, upgrade, `adopt::by_hand`, blueprint apply | the one kind in hand |
| the start walk, `adopt::with_index` | the names in `Walked::left`, which the first pass already holds |
| the `#[ignore]`d published-index test | `all()` — its only caller |

So **no daemon path lists every kind**. The two unfiltered listings between them ask for what this
build can install, and a kind the index publishes ahead of a release is never fetched.

`Err` means what it means today: no root could be obtained at all. A root that was obtained and a
kind that could not be is D5's `missing`.

## D4. A refresh

All of it under one `tokio::sync::Mutex`, so two callers never race over the cache.

1. `GET index-v2.json.minisig`. **Byte-identical to the cached signature, and the cached root still
   verifies against it**: nothing was published. The cached root's modification time is set to now
   and the answer is `Fetched`. Nothing else is requested. A cached root that no longer verifies is
   no cached root, and the refresh goes on to step 2 whatever the signature says.
2. Otherwise `GET index-v2.json`. Verify it against the new signature **before parsing**, check
   `schema == 2`, that `base_url` is an `http` or `https` URL, and refuse a `generated_at` older
   than the floor (D10).
3. For every kind file in the cache that verifies against the *old* root and whose hash differs in
   the *new* one: fetch it, refuse it unless its length is exactly `size`, check its sha256
   **before parsing**, then parse it and check that its `schema` is 2 and its `kind` is the kind
   asked for. Concurrently (D8).
4. Only when every one passed: commit (D5).
5. Anything failed: nothing is written, and the answer is the old set with `Stale`.

With no cached root, step 3 has nothing to do and step 4 stores the root and its signature.

## D5. A kind on first use, and a kind that cannot be read

A kind the view needs, and the cache does not hold for the cached root, is fetched against the
cached root with step 3's checks and stored. When that fails — wrong length, wrong hash, a `404`, no
network:

- if the root was fetched from the network in this same call, the kind is **missing**;
- otherwise refresh once (D4). A refresh that brought a new root is followed by one more try; one
  that found the signature unchanged, or failed, is not — the root is not behind, the file is
  wrong — and the kind is missing.

`Missing { kind, reason }` is carried by the view. One kind that cannot be read does not fail the
other five of a listing.

**A file that is wrong is not asked for again on every call.** A kind refused for its length, its
hash or a `404` is remembered in memory against the root it was refused under, and is missing
without a request until the root changes or a caller asks for `refresh`. A kind that could not be
*reached* is not remembered: that is the network, and the next call tries it again as it does today.

**A failure means one new variant**, `Error::IndexKind { kind, url, problem }`, where the problem is
a wrong length, a wrong hash or a file that names another kind. It maps to the wire as
`IndexSignature` does — the file is not what the signed root says it is. Transport, an unreadable
file and an unknown schema reuse the existing `Index*` variants with the kind file's URL.

- `runtimes::offered` and its package twin ask the view for a missing kind first, and answer the
  wire error of an index that could not be read for that kind, never "the index does not publish
  php 8.4.25".
- `RuntimeCatalogue` and `PackageCatalogue` gain `unavailable: Option<Vec<CatalogueGap>>`, a
  `{ name, reason }` per missing kind — additive, `None` from an older daemon, regenerated into
  `bindings/`. `mix runtime available` and `mix package available` print a line above the table, as
  they do for `stale`.

## D6. What is kept in memory

`PackageIndex` keeps the verified root and each parsed kind (`Arc<[Package]>`) it has read.

**An entry is used while the file it was read from still has the length and modification time it
had when it was read**, and is dropped and re-read — and re-verified — when it does not. One `stat`
per kind asked for replaces a read, a hash and a parse. That is also what makes `daemon.cleanup`
right without telling it anything: it removes the files, the next call finds no file, and memory
goes with it.

Memory is not the disk: what is held there was verified when it was read, and is not verified again
until the file changes. A file rewritten to the same length inside one tick of the file system's
clock is not noticed, and need not be — what memory holds is still what was verified. When D4's
first step moves the root's modification time itself, it records the new one.

**Not measured.** What 119 decoded packages weigh on the heap is unknown; T196c measures it and
writes the number beside the field. At ten years the estimate is the expanded catalogue of the kinds
a home uses, because decoding copies a shape into each artifact. Keeping shapes shared in memory
would mean `Artifact` no longer owning its fields, which changes every caller in D2's list; it is
not done here, and the measurement is what would justify it.

## D7. The cache

Flat in `cache/`, under the names they are published with:

```
index-v2.json
index-v2.json.minisig
index-v2-<kind>.json
```

- **The age of the index is the root's modification time**, as it is `index.json`'s today. A
  modification time in the future still reads as "as old as possible".
- **The invariant**: every kind file in the cache hashes to what the cached root says. A kind file
  that does not is *absent* — it is not an error, it is fetched again under D5 — which is what makes
  a commit interrupted half way heal itself.
- **Commit order**: each file is written beside its destination and renamed over it; kind files
  first, then the signature, then the root. A kind file the new root no longer names is removed and
  logged, because the root promises that never happens.
- **The window, stated**: a daemon killed between the first rename and the last leaves an old root
  beside some new kind files. Those kinds are absent until the next successful fetch; offline, they
  are unavailable until then. Kind files named by hash would close it and are not proposed: the
  window is milliseconds and the repair is one fetch.
- A kind name reaches a file name or a URL only after it matched `^[a-z][a-z0-9-]*$`, the root
  schema's own pattern. A root entry that does not is skipped and logged.
- `daemon.cleanup`'s list of cached documents (`disk/measure.rs`) gains the three names above; the
  third is a prefix and a suffix, where the list holds whole names today. That list names no
  `.minisig` for the three older documents, so a cleanup leaves their signatures behind. It is
  harmless and is left as it is.

## D8. Timeouts, sizes, concurrency

- **`FETCH_TIMEOUT` stays 30 seconds and applies to each file**, set on the request rather than
  inherited from the transport. At the estimated ten-year size of the largest kind file that is a
  connection of about 80 kbit/s; at 16 MB in one request it was about 4 Mbit/s.
- **The first failure ends the refresh.** A machine with no network waits for one timeout, not one
  per file.
- **A body is never read past what it may be**: a kind file stops at `size` bytes and a
  `Content-Length` that disagrees is refused before the body; the root stops at 1 MiB (1,914 bytes
  today, about 100 more per kind); a signature at 4 KiB (308 today).
- **Kind files are fetched four at a time.** Sequentially, a first `runtime.list_available` is
  eight requests at the 0.66–1.06 s measured above. Four is a choice, not a measurement; the first
  listing through `reqwest` is timed in T196c and the number replaces this sentence's.

## D9. A source with no schema 2

`--index-url` / `MIXENGINE_INDEX_URL` names a document, and the schema 2 set is **derived beside
it**: the last path segment is replaced, as `IndexSource::registry_url` already does for
`extensions.json`. No second setting. A setting that already names `index-v2.json` gets the same
set, and `index.json` beside it is the fallback.

**When `index-v2.json.minisig` answers `404`, and only then, this refresh reads `index.json`**
through `Client<schema1::Document>`, exactly as today, and builds the same `Index` view from it. A
timeout, a `5xx` or a signature that does not verify is a failed refresh, not a reason to fall
back. The fact is logged once per daemon run.

- **It is not a downgrade anybody gains from.** Both encodings are signed by the same key, carry the
  same `generated_at` (measured) and the same packages (the publisher's `verify.py` holds them
  equal), and D10's floor is one floor across both.
- **One layout on disk at a time.** A refresh that succeeds through schema 1 removes the schema 2
  set from the cache, and the reverse (D10).
- **It applies to the published URL too**, not only to a mirror: one path, and no way for the set
  going missing to leave a home with nothing.
- **Until when**: for as long as `mixengine-packages` generates `index.json`. Its design leaves that
  date open on purpose. Removing the schema 1 reader is a task of its own, ordered after that date
  is chosen and after `runtime-packaging.md` has told mirror operators to copy the whole `index`
  release.

## D10. A home that already has a cache

An installed home holds `cache/index.json` and its signature. Nothing is migrated at start.

- **The floor is carried across.** The first schema 2 refresh refuses a root older than the newest
  of the two verified things the cache may hold: a schema 2 root, and a schema 1 `index.json`. A
  home cannot be walked backwards by being upgraded. Equal is accepted, which is what the publisher
  writes today.
- **Until that refresh succeeds, the old cache answers.** Offline right after an upgrade, the
  schema 1 file is read through D9's reader and the answer is `Stale` — not an empty list.
- **When it succeeds**, `index.json` and `index.json.minisig` are removed from the cache.

## D11. `extensions.json` is not touched

It is one document and stays on `Client<Registry>`, unchanged. **"Ask for the signature first" is
not applied to it here.** It would save 8,457 bytes per six hours (measured), it would change the
request order of a client two other documents share, and the publisher's promise that the signature
is uploaded last is made for the schema 2 set and for nothing else. The shared primitives of D1
make it a small change later, if its size ever asks for it.

## D12. Decoding, and where it is stricter than the reference

`tools/catalogue.py`'s `decode_kind` is the reference, and the client must produce what it produces
for every file the publisher writes:

- the URL is `{base_url}/{kind}-{version}/{kind}-{version}-{os}-{arch}.{format}`, with a trailing
  `/` on `base_url` ignored and nothing percent-encoded — byte for byte what schema 1's `url` is;
- `base_url` comes from the root the kind file was verified against.

Three places are stricter, and none is reachable from what `encode` writes:

- **A shape cannot overwrite what an artifact says about its bytes.** The reference merges the shape
  over the artifact (`**shapes[at]`), so a shape carrying `sha256` would win. The client reads a
  shape into the four fields it knows and ignores the rest.
- **`format` is a string, not an enum**, checked against `^[a-z0-9.]+$` and composed into the URL.
  A format this build has never heard of is then refused by the installer for that artifact —
  runtimes and packages install with `NotAnArchive::Refuse` — instead of making the whole kind
  unreadable.
- **A `shape` past the table, or a `kind` that is not the file's**, makes that kind unreadable.

An unknown field anywhere is ignored, on `format.rs`'s rule.

## D13. `MockRegistry`

It keeps serving `/index.json`, and **serves the schema 2 set encoded from the same value** on
every `start` and `publish`: root signed with the test key, kind files compact with sorted keys and
a trailing newline, named by hash. Every suite that uses it then runs through the schema 2 path with
no change of its own.

- **The encoder is the testkit's own.** `mixengine-testkit` does not depend on `mixengine-core` and
  must not start, so the client is tested against a second implementation of the format rather than
  against itself. It already carries `sha2`, `serde_json` and `minisign`.
- **An artifact's URL.** Schema 2 cannot say one. The root's `base_url` points at the registry
  itself, and for every artifact whose `url` the registry serves, the composed path answers with
  the bytes `publish_asset` was given. So the 30 call sites stay as they are: all of them publish at
  `Packed::path()`, and the index they hand over is what names kind, version, system and
  architecture. A `url` ending in none of `.zip`, `.tar.zst`, `.tar.gz` panics with the fixture's
  name; the one-file fixtures that exist are an extension's (`adminer-6.0.1.php`) and the update
  feed's (a `.pkg` in `api/rpc.rs`), and neither enters a package index.
- **A value that is not an index.** Some suites start the registry only for its assets, with
  `{"schema": 1}` and nothing else. A value with no `packages` or no `generated_at` is served at
  `/index.json` as it is and gets no schema 2 set, which is D9's `404`.
- **What an install records changes in tests**: the composed URL, not the one the fixture wrote. No
  test asserts it today.
- **The states in between**, each one call:
  - `publish_signature_only` — a new signature beside the old root;
  - `publish_root_unsigned` — a new root beside the old signature;
  - `corrupt_kind(kind)` — bytes that do not hash to the root's entry; `truncate_kind(kind)` — the
    wrong length;
  - `withhold_kind(kind)` — `404` for that file;
  - `without_schema_2` — `404` for every `index-v2*` path, the mirror of D9;
  - `unplug` / `plug`, as today.
- **`requests()`** — every path asked for, in order. "Nothing but the signature was requested" can
  only be asserted on this.

## Tests

In `crates/mixengine-core/tests/index.rs` unless said otherwise, each against `MockRegistry` over a
real socket.

- a fresh home: signature, root, and only the kinds asked for are requested;
- an unchanged signature after six hours: one request, `Fetched`, and the age starts again;
- a PHP release on a home that cached PHP and Node: signature, root, `index-v2-php.json`, and not
  Node's;
- a root older than the cached one is refused and everything is kept, `Stale`;
- a new root whose cached kind is corrupt, truncated or withheld: root and kind files on disk are
  byte-identical to before, `Stale`;
- a new signature beside the old root, and a new root beside the old signature: the same;
- a kind on first use that does not hash to the cached root: one refresh, one retry, then `missing`
  for that kind while another kind in the same call is answered;
- a kind file, and a root, rewritten on disk: ignored and fetched again;
- memory: a second call reads no file (asserted on the request log and on an unreadable cache file
  being irrelevant), and a changed file is re-read;
- a home holding only a schema 1 cache: offline it answers `Stale`; online a root older than that
  cache is refused; after success `index.json` is gone;
- a source with no schema 2: read through schema 1, the schema 2 set removed; a `503` on the
  signature does not fall back;
- decoding: a Windows/Unix PHP pair with different shapes, an artifact with no optional field, a
  shape carrying `sha256`, a `shape` past the table, an unknown `format`;
- every existing test of `Client<D>` stays, for `extensions.json` and the feed;
- daemon: `unavailable` on both listings; `offered` on a missing kind;
- the `#[ignore]`d published-index test reads `all()`, and also fetches `index.json` and asserts
  the two decode to the same packages, value for value — the client's half of `verify.py`.

The 30-second timeout is not waited for by any test; the per-file limit is asserted with a duration
the test injects.

## MixLab

No new screen and no new method. `unavailable` (D5) is drawn where `stale` is drawn today: the
Packages screen's two lists (`Languages.tsx`, `PackageList.tsx`) show one line naming what could not
be read, beside `StaleBadge`. With MixEngine
switched off nothing changes — the window never reads the package index itself.

## What the format does not give a client

Nothing here is changed in `mixengine-packages`. For whoever owns that repository:

- **The reference decoder lets a shape overwrite `sha256`, `size` and `url`** (D12).
  `index-v2-kind.schema.json` constrains a shape to "an object with `provides`" and no more. The
  client is strict on its own; a `not`/`propertyNames` in the schema would make `verify.py` strict
  too.
- **A new `format`, `os` or `arch` value is a change a deployed client may not read.** `os` and
  `arch` are closed enums in the client, as in schema 1, so an artifact for a fourth system makes
  its whole kind unreadable to every client already installed. D12 makes `format` survivable; the
  other two need a schema number, or a client that skips the artifact, and that is a decision for
  both repositories.
- **A home with no cache, asking during an upload, gets an error.** The signature is uploaded last,
  so for that window a new root sits beside the old signature. A home with a cache answers `Stale`;
  a fresh one has nothing to fall back to and says the index is not signed by this build's key,
  until it is asked again. The publisher cannot replace several assets at once and the client does
  not retry on a timer.
- **`MIXENGINE_MIRROR_URL` is documented in `runtime-packaging.md` and read by nothing.** The
  root's `base_url` is what now does that job, for a mirror that signs with its own key.

## Documentation

- `index.rs`: the module note's "about fifty kilobytes" is wrong — the published `index.json` was
  584 KB before it was written compact and is 339,764 bytes now. It is rewritten around what is
  actually hashed: a root of about two kilobytes, and a kind file of at most 31 KB today.
- `docs/operations/runtime-packaging.md`: the index is a root and a file per kind; a mirror copies
  the whole `index` release; what a mirror with only `index.json` gets.
- `CHANGELOG.md`, and this spec to `implemented`.

## Out of scope

- Removing the schema 1 reader (D9).
- Signature-first for `extensions.json` and the update feed (D11).
- Shapes shared in memory (D6).
- Anything in `mixengine-packages`.
