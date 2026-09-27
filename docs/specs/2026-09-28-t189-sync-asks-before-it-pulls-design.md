---
status: implemented
date: 2026-09-28
task: T189
---

# T189 — Sync asks before it pulls

Phase 30, after [T178d](2026-09-22-t178d-a-skipped-record-is-delivered-again-design.md). 2026-09-28.

## The case

A full run ([`shell/sync/loop.ts`](../../apps/desktop/src/shell/sync/loop.ts)) walks every collection
that is on, and for each one stamps this machine's changes, then asks
`GET /v1/records?collection=…&since=…` for the next page, then pushes. With eleven collections on,
that is eleven reads of the account every time the app starts, every time the window regains focus
after a minute, and every fifteen minutes. On the Worker each read wakes the account's Durable
Object, which is exactly what D8's first rule is about: how often a client wakes an object, not how
much it carries. Almost every one of those reads comes back empty.

There is a second cost. After this machine pushes, its own write has a new `seq`, so the next pull
of that collection fetches it again — **the echo**. `incoming` recognises the content as already
agreed and drops it ([lend.rs](../../apps/desktop/src-tauri/src/sync/lend.rs)). The client pays a
request to learn what it already knew.

## Principle

**One question per run, and a pull only for the collections whose answer is yes.** Something is
news when another machine wrote it, or when this machine's cursor can no longer be trusted. A
machine's own accepted writes are not news. A push already remembered their versions
([engine.rs](../../apps/desktop/src-tauri/src/sync/engine.rs), `store.remember` on `200 | 201`) and
agreed on their content (`settle_pushed`).

## D1. `POST /v1/records/heads`

**Mandatory in `/v1`**: it is not announced in `features`, and a client does not fall back to pulling
blind. Every server this project runs, and every native build from this change on, answers it.

```json
{ "cursors": { "<collection>": 903, "<collection>": 0 } }
```

```json
{ "stale": ["<collection>"], "nextSince": 911 }
```

- `Authorization: Bearer`, like every route but `/v1/capabilities`. `401 invalid-token` as elsewhere.
- **A read**, so a frozen account answers it (D4b). Rate limiting treats it exactly as it treats
  `GET /v1/records`.
- `cursors` has at least one entry and at most `maxBatchOperations`. Each key is a collection id (64
  lowercase hex) and each value an integer `≥ 0`. Anything else is `400 invalid-request`. No new
  capability: the batch limit already bounds how much a client may name in one request.
- `stale` names the collections D2 calls stale, in no particular order. It is a subset of the keys
  sent. A collection the account has never held is simply not stale unless D2's first rule applies.
- `nextSince` is the account's latest `seq` (`next_seq`), the same value a `GET /v1/records` page
  with `more: false` ends at (M3). It is read in the same transaction as `stale`. The Durable Object
  gives that for free, and the native server reads both inside one transaction.

`sync-protocol.md` gains this route under *Records*, and D4a's *Every code a client can meet* is
unchanged: the route introduces no new code.

## D2. What stale means

For a collection `c` sent with cursor `since`, by a session of device `d`, `c` is **stale** when any
of these holds:

1. **`0 < since < reaped_below_seq`**: the pull would answer `410 cursor-expired`. Stale, so the
   client pulls, meets the `410` and resyncs exactly as it does today (T178b). The rule is the same
   test `listSince` makes, and the resync flag does not apply here because a resync is never asked
   about (D4).
2. **`since = 0` and `c` holds any row**: a cursor of zero has seen nothing, including this device's
   own rows.
3. **A row of `c` with `seq > since` and `device ≠ d`**: another machine wrote or deleted something.
   A row whose `device` is `''`, written before D3 stamped devices, counts as another machine's.

**What is not stale:** a collection whose only rows above `since` were written by `d` itself. These
are the echoes, and D3 lets the client step over them.

## D3. A cursor moves without a pull

For every collection the client sent that is **not** stale, it sets its cursor to `nextSince`.

**Why this is safe.** Between `since` and `nextSince`, `c` holds only rows written by this device,
and those are exactly the rows a pull would have delivered and `incoming` would have dropped as
already agreed. Moving the cursor records the same thing the pull would have recorded, without the
pull. Rows of other collections in that range are not `c`'s business, which is the same reasoning
M3 already uses to end a page at the account's latest `seq`.

**Why it is needed.** Without it, a machine that is the only writer never moves its cursor, since
nothing ever makes its collections stale. Ninety days after one of its own deletions, reaping lifts
`reaped_below_seq` past that cursor, D2's first rule fires, and a machine that missed nothing
downloads the whole account again. With it, the cursor tracks `next_seq` at every run, as pulls keep
it today, and falls below `reaped_below_seq` only when the machine has been away longer than a
tombstone lives. That is the case `410` exists for.

**One exception is D5's**: a write the server accepted and this machine never remembered. D3 steps
over it too, and D5 is what makes that harmless.

## D4. The client

A new command, `sync_heads(collections: Vec<String>) -> Vec<String>`, in
`src-tauri/src/sync/commands.rs`, backed by `Session::heads`:

1. Each collection is mapped to its opaque id, as `pull_page` does.
2. **Stale without asking**: a collection whose cursor is `0`, that is mid-resync
   (`store.resyncing`), or that is owed under another version (`store.owed_elsewhere`, T178d). These
   are exactly the states in which `engine::fetch` already does something other than a plain read
   from the cursor, and the server cannot see any of them.
3. The rest go in one request, chunked by `maxBatchOperations` (one chunk in practice).
4. For each collection sent and not named in `stale`, `store.set_since(collection, nextSince)`.
5. The result is the plain names of every stale collection, from step 2 and from the server.

**The heads call and the cursor moves happen inside the same lane as the run** (`loop.ts`'s
`inLane`), so no pull or push of this machine interleaves with them.

`startSyncLoop`'s full run becomes:

```ts
const stale = new Set(await backend.heads(collections.map((c) => c.id)));
for (const collection of collections) {
  stale.has(collection.id)
    ? await syncCollection(backend, collection, onSending)
    : await pushCollection(backend, collection, false, onSending);
}
```

A push-only run is unchanged: it never asked the server and still does not. `pushCollection`'s
`needsPull` path is unchanged: a collection never pulled has cursor `0` and is stale by step 2
anyway.

`SyncBackend` gains `heads`, `tauriSync` implements it, and the loop's tests hand it a fake like the
rest.

**What a quiet account costs:** one request per full run, instead of one per collection. A
collection another machine touched costs what it costs today. An echo costs nothing.

**A server without the route** answers `404`. The run fails through the existing refusal path
(`transport::refusal` → `error.syncServerRefused`), shown in the Sync pane like any other refusal.
This is the price of D1's "mandatory", taken knowingly: there is no server this project knows of
that will not have the route.

## D5. A write the server kept and this machine forgot

The one thing the echo did that nothing else does: if the app dies after the server accepted a
batch and before `store.remember` ran, the next pull brought that write back, and `incoming` saw
this machine's own stamp with the same hash and agreed on it. D3 now steps over that row.

What happens instead: the change is still stamped, so the next push sends it again with the
version this machine last remembered. The server answers `409 version-conflict` (or `412
already-exists` for a creation) with the current record, **written by this device at this same
`updatedAt`**. Today `merge::resolve` breaks that tie against the local side (equal time, and
`local_device > remote_device` is false for equal devices), so the push hands the record back as
`superseded`, the module writes identical content, and the person is told one of their edits was
replaced by a newer one when none was.

**The fix, in `engine::push` only:** a `409 | 412` whose `current.device == device` and
`current.updated_at == change.updated_at` is this machine's own write. It is remembered and retried
under `If-Match` with its version, like a `Keep::Local`.

**Why that is right even if the content differs.** A stamp's time is kept only while its hash is
unchanged ([store.rs](../../apps/desktop/src-tauri/src/sync/store.rs), `Store::stamp`), so the same
time from the same device is, apart from two edits inside one second, the same change. In either
case the local item is this device's latest word on that record, and writing it over this device's
earlier word is what the person did. The retry costs one extra write in a rare case, and needs no
decryption in the push path.

`merge::resolve` itself is not changed. `incoming` handles the same tie already, through its
`stamp.hash == digest` check.

## D6. The two servers

- **Worker** (`server/worker/src/records.ts`): `staleSince(sql, cursors, device)` beside
  `listSince`. For each entry it makes one `SELECT 1 … LIMIT 1` for rule 3, or for rule 2's
  any-row check, after the rule-1 comparison against `reaped_below_seq`. Routed in `account.ts` as
  a `case "POST /v1/records/heads"` beside `POST /v1/records/batch`. `index.ts`'s `BY_TOKEN`
  already admits every path under `/v1/records/`, and the single-record handler only takes `PUT`
  and `DELETE`, so nothing else there changes.
- **Native** (`server/native/src/records.rs`): the same function over `(account_id, …)`, inside one
  transaction with the read of `next_seq`, routed in `lib.rs` as
  `.route("/v1/records/heads", post(records::heads))`. It cannot collide with
  `/v1/records/{collection}/{id}`, which is one segment longer.
- **An index on `(collection, seq)`** (`(account_id, collection, seq)` on native), added with
  `CREATE INDEX IF NOT EXISTS` in both schemas, so each lookup is a range seek rather than a walk
  of `record_by_seq` filtered by collection.

## Cost

- Server: one route and one index, twice. The query per collection is bounded by `LIMIT 1`.
- Client: one command, one `SyncBackend` method, a few lines in the loop, and the tie rule in
  `engine::push`.
- Protocol: a route in `/v1` that a client now requires.

## Alternatives not taken

- **`GET /v1/records/heads` answering each collection's latest `seq`, with the client deciding.**
  Simpler as a route, but the cursor-expiry rule (D2's first) would then live in the client as well
  as the server, and the two could disagree. The `POST` form keeps "would a pull return anything" a
  server fact, which the suite checks against the pull itself.
- **Counting a machine's own writes as news.** The echo would cost one pull per push, and nothing
  else would change. Rejected at design review: the point of the route is to stop reading what is
  already known.
- **Excluding a machine's own writes without D3.** The cursor would stand still for a machine that
  is the only writer, and a reaped tombstone would eventually expire it into a full resync (D3,
  *why it is needed*).
- **Announcing the route in `features`, with a blind-pull fallback.** Rejected at design review:
  every server in use gets the route in the same release, and a fallback would be a second code path
  that nothing exercises.
- **Advancing the cursor after a push instead.** Safe only when the pushed rows' `seq`s are
  contiguous from the cursor, which the client can check but cannot always obtain. D3 is the same
  idea, asked of the one party that knows.

## How it is proven

- **`server/conformance/src/heads.test.ts`**, against both servers:
  - device A writes; A's `heads` from before the write does not name the collection, B's does;
  - `nextSince` equals the `nextSince` of a `GET /v1/records` read to its last page;
  - `since = 0` is stale for a collection with rows and not for one without;
  - B deletes, and the tombstone is reaped past A's cursor: A's `heads` names the collection, and
    A's `GET` from that cursor is `410`. Like `tombstones.test.ts`, this needs a server configured
    to reap at once, and is skipped against one that is not;
  - **agreement with the pull**: for a spread of cursors, a collection is stale exactly when a `GET`
    from that cursor returns `410` or a record written by another device;
  - an empty `cursors`, one over `maxBatchOperations`, a malformed id and a negative cursor are each
    `400 invalid-request`; a frozen account answers `200`.
- **`engine.rs` / `session.rs` tests**: cursor `0`, a resync and an owed collection are stale
  without asking; a collection not named is moved to `nextSince`; a named one is left alone.
- **`engine::push` test**: a `409` carrying this device's own record at the same `updatedAt` is
  retried and landed, and `superseded` is empty.
- **`loop.test.ts`**: a full run calls `heads` once, pulls only what it names, and pushes the rest.
- **`sync_live.rs`**, against two native servers: two machines, one writes; the other's run pulls
  only that collection. The writer's next run makes one request and pulls nothing.

## MixLab

**No screen changes.** This is how the Sync loop that already runs behind Settings → Sync talks to
its server, and the window draws nothing new. The Sync pane's last-synced line and error line read
the same run results as before, so a failed `heads` shows there like any failed pull. The icon that
shows a download while the run asks is the follow-up below, not this task.

## What this does not do

- **The icon.** `activity.ts` still shows `down` for every full run past 200ms. With `heads` it can
  show `down` only when some collection is stale. That is a separate task, and this one leaves the
  signal in place for it: `sync_heads` returns the stale list.
- **The push-only run and `LOCAL_CHECK_MS`.** Unchanged. They never asked the server.
- **`/v1/capabilities` before every sync.** Unchanged.
