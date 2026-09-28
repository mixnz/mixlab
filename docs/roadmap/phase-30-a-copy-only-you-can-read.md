# Phase 30 — A copy only you can read

*Goal: a person's second machine has what they ticked and nothing they did not, and the server that
carried it cannot read a byte of it.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done.

Design: [2026-09-20-t177-a-copy-only-you-can-read-design.md](../specs/2026-09-20-t177-a-copy-only-you-can-read-design.md).
Decision: [ADR 0045](../decisions/0045-mixlab-has-an-account-and-mixengine-does-not.md).

---

- [x] **T177a** The key hierarchy and the record envelope, in `apps/desktop/src-tauri/src/sync/crypto.rs`:
      Argon2id, the four HKDF expansions, XChaCha20-Poly1305 with `collection || id || deleted` as
      AAD, and the wrap/unwrap of `MK` under a password and under a recovery key. No network, no
      account, no storage. It carries test vectors and is written to be read in one sitting —
      spec D1 says the promise is a property of this file and of nothing on the server.

      **Done in six commits, thirteen tests, 470 lines.** Two things it settled that the plan had
      not. `sync` is `pub mod` in `lib.rs`, because `-D warnings` fails a module nothing outside
      `#[cfg(test)]` reaches and this one has no caller until T177c — public suppresses no lint,
      and `lib.rs` carries the note that it returns to private the moment something calls it. And
      **this crate is not format-gated**: CI's `desktop` job runs clippy, `cargo test --locked` and
      `cargo audit`, with no `fmt`, and there is no `rustfmt.toml` here, so `cargo fmt --all` would
      rewrite dozens of files nobody touched. Check the files you wrote and nothing else.
- [x] **T177b** `/v1` frozen at the spec's D4, and the **conformance suite written first** — before
      the server it will judge, because a suite written afterwards only ever describes what was
      built; it lives in `server/conformance/`, beside both implementations and inside neither.
      Then `server/worker/` on Cloudflare Workers, one Durable Object per account: its
      serialized execution is what makes the compare-and-swap and the monotonic `seq` correct, its
      SQLite storage holds the record table, and its alarms reap tombstones at ninety days.
      Registration and email verification through an external provider, login, refresh and
      revocation, the device list, a per-account quota, rate limiting inside the object, and
      `/v1/capabilities`. Nothing in it parses a ciphertext.
      Its CI is `.github/workflows/server.yml`, fired by `server/**` alone: `ci.yml` gains no job
      family and no server change fires its three-OS matrix, while a push to `master` — the branch
      Workers Builds deploys from — answers for itself without being asked. The third entry in
      `docs/operations/build-and-release.md`'s list of workflows that are not in that table.

      **Done in five commits; 88 conformance assertions green against the Worker.** Four things it
      settled that the plan had not, each found by writing the suite before the server. D4 was a
      table of intentions and not a wire, so the first commit is **D4a**, which decides the bytes —
      without it the first implementation decides them and the suite copies, which is the failure
      writing the suite first exists to prevent. Verification gates **signing in**, not writing, so
      the `403` on a record route is unreachable in v1 and the suite says so rather than pretending
      to test it. Revoking a device ends **both** its tokens: the appendix had reasoned that
      closing the access token early costs a revocation check per request, which is wrong for an
      opaque token the server looks up anyway. And registration is limited **per source** in an
      object of its own — a counter inside one account cannot see an abuse that opens many.
- [x] **T177h** — *lettered last, ordered here, right after T177b.* `server/native/`: the same
      protocol in Rust over a SQLite file, excluded from the root Cargo workspace the way
      `apps/desktop/src-tauri` is, with a Dockerfile beside it — an image published to this
      repository's Packages, following `master`, for somebody self-hosting who would rather pull
      than compile. Nothing in the hosted path is a container: the default instance is the Worker.
      **Built alongside the Worker rather than after it**, because `/v1` is only a protocol once
      something other than the Worker has spoken it — each implementation is the other's proof, and
      the conformance suite is what makes that claim checkable rather than asserted. `server.yml`
      gains a second job, so one run of it answers for both implementations.

      **Done in three commits; the whole suite green against both, first run.** What it settled
      that the plan had not. The native server is a **library with a binary on top**, for the same
      reason T177a's module is `pub`: `-D warnings` rejects a module whose callers land in a later
      commit, and a library's public surface is not dead code — it is also the shape a server
      wants if it is ever to be tested without a port. Reaping is **one task sweeping every
      account**, not an alarm per account: the Worker's rule is about money, and with one process
      and one file it buys nothing. And `410 cursor-expired` needed **a second instance** of each
      server with a retention of zero, because reaping at once breaks every other tombstone test —
      four conformance legs in one run rather than two.
- [x] **T177i** — *what the server learned after T177b and T177h were ticked.* An account is
      **copied and deleted rather than relocated**: `/v1/account/freeze` holds it still, the client
      carries it across with the ordinary paged read and batch write, and `/v1/account/delete` ends
      it. Nothing is forwarded — a symbolic `home` was readable exactly when it was redundant and
      unreadable exactly when it was needed, and a URL instead would have been a phishing primitive
      because the destination learns `A`. A freeze has **no expiry**: thawing is reachable from
      every signed-in machine, while an expiry let a finished copy reopen the old server on a timer
      for a machine nobody had repointed. `closingOn` in `/v1/capabilities` is the whole of what an
      old server contributes — a date, never a destination — and the old server can now be switched
      off, which the forwarding design could never allow.
      Then the three things the rate limits had missed. **A letter is counted against the address
      it reaches**, not only against the network that asked for it, and that counter lives outside
      the account because registering over an unverified one replaces it. **A source is the peer
      address**, not `X-Forwarded-For`: measured on six forged values against a server allowing two
      an hour, the old code accepted six and the new one two. And **D6 case 2** — forgotten
      password, recovery key held — is reachable at last, in two requests so the client can unwrap
      `MK` before it re-wraps it: the letter proves who, the recovery key preserves what, and the
      records survive.
      The wire also left the spec for `docs/features/sync-protocol.md`, where it can be edited as
      `/v1` grows; a design document stops being edited when its work is implemented, and `/v1`
      does not stop.

      **152 conformance assertions, 147 green and 5 skipped against each implementation**, the
      same numbers on both — which is the claim two implementations exist to make.
- [x] **T177c** The client half of the protocol: pull by cursor, push under `If-Match`, the `409`
      resolved by `updatedAt` with the device id breaking a tie, and `batch` for the first push
      from a machine that already has a hundred saved things.

      **Done in twelve commits.** Two things it settled that the roadmap had not. D4's tie-break
      read the writing device and **no record carried one**: the server now stamps `device` from
      the session that wrote it, which costs nothing because every write was already
      authenticated with one device's token. And the engine **moves ciphertext only** — a page is
      applied before the cursor passes it, a conflict the other side wins is handed back rather
      than dropped, and three rounds of a conflict that will not settle end in an error rather
      than a loop. `tests/sync_live.rs` is `#[ignore]`, like every test here that needs what CI
      lacks; run by hand it passes against both the native server and the Worker.
- [x] **T177d** A module lends a collection without the shell learning what it is.
      `ModuleDefinition` gains the syncable set — id, label, reader, writer, default `false` — and
      `registry.ts` wires it as it already wires tabs. `npm run lint` still refuses a third file
      outside `src/modules/` that names a module.

      **Done in eleven commits.** Two things it settled that the spec had assumed. **No module
      stored when an item changed**, so `updatedAt` is the sync layer's: the store keeps a hash of
      each record's canonical plaintext as last agreed, an item whose hash differs is a change
      made now, and the hash is recorded only after the change has landed on the other side — never
      before, or a failed write would push a stale copy back over something newer. And **every
      reader is an allow-list**: a connection's sidebar width, a terminal's default shell, a
      request's last use and its credential stay on the machine, and a field added later does not
      travel until somebody decides it should. `serde_json` has `preserve_order` on here, so the
      hash is taken of an explicitly canonical form.
- [x] **T177e1** The account and the loop: the commands (register, verify, sign in and out,
      refresh, devices), `MK` and the session in the credential store, and the shell running every
      collection that is on at the D8 triggers. A pull's cursor does not pass a page until the
      module has written it; a lost conflict is written down before anything is pushed again; a
      change keeps the time it was first noticed.

      **Done in ten commits.** What it settled that the roadmap had not. **The loop is split across
      the boundary**, because readers and writers are TypeScript and `MK` is Rust: a pulled page,
      or a lost conflict's winners, leaves Rust as plain items and a token, and nothing — version,
      hash or cursor — is recorded until the token comes back saying the module wrote them. That
      split is also what closed the two holes the loop would have opened: an edit retried last used
      to win by being stamped last, and a loser that failed to write the winner used to push again
      carrying the winner's version, meeting no `409`. **D8's *"on a local change"* is a local look
      every thirty seconds** rather than a signal from each module: Rust compares hashes before it
      opens a socket, so a look that finds nothing costs no request, and eight modules did not have
      to learn to announce their writes. **A refresh is serialised under one lock**, because a
      refresh token rotates and two racing refreshes revoke the device's chain. And **`MK`, the
      refresh token and a closed server's access token share one credential entry**, so macOS asks
      once. `tests/sync_live.rs` now signs up, confirms, signs in a second machine and carries an
      item across through `SyncState` itself; run by hand against the native server, all six cases
      pass. Nothing is visible yet: every row is off and there is no screen to sign in from, which
      is T177e2.
- [x] **T177e2** The account, in Settings: sign up with the recovery-key ceremony (thirteen groups
      shown once, two typed back), sign in and out, the per-collection list with every row off, the
      device list with a revoke, the notice that an edit was replaced, the warning when a server
      reports `closingOn`, and the list of servers — `https://sync-0.lab.mixnz.com` first and fixed,
      then whatever a person hosting their own adds. The machine's name is filled in from its
      hostname and can be changed.

      **Done in ten commits.** What it settled that the roadmap had not. **A replaced edit is held
      by a store that outlives the Settings dialog**, because the dialog is not mounted while sync
      runs and T177e1's window event reached nobody; the pane reads the store when it opens. **The
      logic that can be wrong is outside the components** — the list of servers, the ceremony's
      checks, the store — because vitest here has no DOM, so a check inside a component is a check
      nothing runs. The access-token field appears **only for a server that is not the default**:
      the default is open, and a field nobody there needs is one somebody fills with a password.
      Plain `http` is accepted only for a server on this machine. And **MixLab's privacy policy
      moved into this repository's handbook** (`docs/guide/*/privacy.md`), because the one the app
      linked to was MixDB's and said there was no server of ours — which sync made false; the
      settings hint that said the same was rewritten with it. The screen has not been looked at by
      anybody but its author's type checker: every row is off by default, and the first person to
      sign up against `sync-0` is the first to see it.
- [x] **T177e3** The password, after the account exists: change it while signed in (D6 case 1),
      and recover a forgotten one — keeping the records with the recovery key (case 2), or starting
      over without it under a new `MK` and a new recovery key (case 3).

      **Done in eight commits.** What it settled that the roadmap had not. **Case 2 holds the
      ticket in Rust** after the code is spent, so a mistyped recovery key costs a retry rather
      than a second letter; the ticket is dropped only once it has worked or expired. **Case 3 holds
      the new keys until the ceremony is over**, and only then spends the code — because spending
      it is what deletes, and a person who closed the window halfway through should lose nothing.
      Once case 2 has spent a code, case 3 is no longer offered: that code can no longer delete
      anything. The three cases run against the native server in `tests/sync_live.rs` — a changed
      password signs the other machine out and keeps the records, a wrong key is refused and the
      right one keeps them, and starting over leaves the server empty under a new thirteen-group
      key. The screens themselves have been checked by `tsc`, lint and the unit tests only.
- [x] **T177e4** Leaving a server: delete the account, and move it to another server by copying
      (D4b), which asks at the end whether to delete the old one or thaw it, deletion first. Both
      ask for the password, because both need `A`.

      **Done in ten commits.** What it settled that the roadmap had not. **The copy is a module of
      its own**, `sync/copy.rs`, written against something that pages through a whole account and
      the engine's existing `Remote` — so it is tested with fakes like the engine is, and `412` as
      success is a test rather than a comment. **Registration moved ahead of the freeze**: waiting
      for a person to read a letter needs nothing to hold still, and done first it keeps the
      read-only period to the copy itself. **The recovery key's wrapped copy is now kept at every
      sign-in**, because a move sends it to the new server unchanged and only signing in hands it
      out; an entry kept before this asks for one more sign-in. A move is four calls with state held
      between them, and `move_confirm` run again after a failure picks up where it stopped without
      asking for the code twice. Any signed-in machine sees a freeze and can end it. The live suite
      deletes an account and moves one between two native servers; the screens have been checked
      by `tsc`, lint and the unit tests.
- [x] **T177e5** `closingOn` where a person will see it, not only in Settings: a strip under the
      tab bar — dismissible, once a run — once the signed-in server's closing date is within 30
      days, and
      a warning on the sign-in form when the server chosen there reports one, read from its
      `/v1/capabilities` before anybody registers on it. *Found while testing T177e4: the Sync pane
      is the only place the date appears, and a person who does not open Settings never learns it.*

      **Done in six commits.** What it settled that the roadmap had not. **The signed-in date is
      read at launch by opening the session**, not left to the first sync — with every row off, the
      first sync may never come. **The sign-in form asks a server's `/v1/capabilities` before any
      account exists** there, unauthenticated; a server that cannot be asked yet is left for signing
      in to explain. **Thirty days lives in one constant**, `NEAR_DAYS`, with tests on either side
      of it and past the date, which stays near: nothing is refused on the day. Settings learned to
      open on a given pane, so the strip leads straight to Sync. The native server was checked to
      report `closingOn` for dates 10 and 60 days out; the strip itself has been checked by `tsc`,
      lint and the unit tests only.
- [x] **T177f** The three credential collections — `connection-secrets`, `terminal-host-secrets`,
      `rest-env-secrets` — each behind its own row, each refusing to turn on until the collection it
      belongs to is on.

      **Done in seven commits.** What it settled that the roadmap had not. **The parent writers
      changed too**: a connection, host or environment arriving on a machine used to be saved with
      no credential, and saving none deletes the vault entry, which would have wiped a credential
      that arrived first. Each now fills from the vault before it saves, and a credential whose
      owner has not arrived goes into the vault under the owner's id to wait. **The environments
      writer now flushes before it returns**, which T177d's did not: the store writes on a timer,
      and a window closed inside it would have agreed on an environment never written. The loop
      syncs in the registry's order, and `registry.test.ts` now checks that every secret row comes
      after the row it belongs to. `toggleRow` holds D5's two rules and is tested; the writers
      that touch the vault have been checked by `tsc`, lint and their pure halves' tests.
- [x] **T177g** What a stranger needs to run one: the server's README, a single-binary deployment,
      and the conformance suite pointed at their own instance. Plus the refusals the spec names —
      history, drafts, workspace layout and usage counts are not in the list, and a test says so by
      enumerating it rather than by trusting the UI.

      **Done in one commit, of documentation.** Most of it had landed along the way. The single
      binary a stranger runs is **the container image**, published from `master` at
      `ghcr.io/mixnz/mixlab-sync-server`; the native README now says so first, and how to back up
      its one SQLite file with the pepper. The conformance README claimed it could be pointed at a
      self-hosted deployment as it stood, which was wrong: the suite needs the test outbox, and a
      real server must never serve it. It now says to test a throwaway copy, and the native README
      has the commands. The refusals were already a test: `registry.test.ts` enumerates exactly what
      syncs and checks that history, drafts, the workspace and usage counts are not among it (T177d).
- [x] **T177j** A way to your own server, from the app: the Sync pane always ends with a line
      saying sync can run on a server you run, and a button to a handbook page covering both ways.

      **Done.** The page is `docs/guide/{en,vi}/self-hosting.md`: the short version of each README
      (Cloudflare Workers, the container image), then how to point MixLab at the result, with the
      READMEs linked for everything else. `SyncSection` renders its screen and then `SelfHosting`,
      so the line shows in every state, an error included.
- [x] **T177k** Sync a person can see: the Settings button turns while sync runs, the account row
      has Sync now and says when the last sync was or why it failed, and alt-tabbing stops asking
      the server for anything.

      **Done.** Focus pulls only when the last full run is a minute old (`FOCUS_PULL_MS`); inside
      that it pushes, which costs no request when nothing changed, as `SyncState::push` returns
      before any socket. `requestSync` now has a trigger of its own that always pulls. The loop
      reports each run to `shell/sync/activity.ts`, which shows a spinner only after 200ms and holds
      it 600ms, keeps the last good full run's time and the last failure. `Icon` has a `spinning`
      prop, so the Settings button and the Sync entry of its nav turn without a fifth hand-written
      spin.
- [x] **T178a** — *found by a review of T177, before sync's first release.* A pull that keeps what
      it has not agreed: local changes stamped before the first page, each pulled record weighed by
      D4 against them, a record whose content is already agreed never handed to the module, a
      module's `write` reporting what it skipped, and every `write` resolving only once its save is
      on disk. Design:
      [2026-09-22-t178-a-pull-that-keeps-what-it-has-not-agreed-design.md](../specs/2026-09-22-t178-a-pull-that-keeps-what-it-has-not-agreed-design.md).

      **Done.** `apps/desktop/src-tauri/tests/sync_scenarios.rs` is its proof: two machines and a
      server that answers the way both do, and every story the review told as a test. Writing them
      found the worst fault of all, which the review had missed: a push does not move the cursor,
      so the next pull handed a machine its own writes back and the module wrote them over any edit
      made since. The spec had not foreseen three things. `notice` runs once per full run, so a run
      reads every collection twice. A credential parked in the vault for an item not yet here is
      also "skipped", because `read` does not return it. And terminal settings were a third writer
      that never waited for the disk.
- [x] **T178b** A resync that ends: `410` forgets the cursor and not the agreements, a resync
      removes what it did not meet, the last page's `nextSince` is the account's latest `seq`, and
      `resync=1` keeps a read from `0` from being expired halfway — on both servers and in
      `server/conformance`. Same design as T178a.

      **Done.** Three conformance tests, green on both native instances. Two things the spec
      settled only once the code was read. Native reads the latest `seq` inside a read transaction,
      because its pool of WAL connections lets a writer commit between two statements. And an
      account copy (`copy.rs`) pages from `0` just as a resync does, so its later pages send
      `resync=1` too; without that, one reap during a move ended the copy halfway.
- [x] **T178c** The review's remaining four: a tombstone's `updatedAt` (`DELETE` carries none), a
      batch whose first failed entry hides the rest, moving an account without checking the
      password it re-registers with, and a store keyed by server URL. Design:
      [2026-09-22-t178c-the-reviews-other-four-design.md](../specs/2026-09-22-t178c-the-reviews-other-four-design.md).

      **Done.** A tombstone keeps the time its deletion was made, a push reads every entry of every
      batch, a move asks `POST /v1/account/check` before it registers anywhere, and the client's
      store is scoped by the new `accountId`. The code settled five things the spec had not:
      - The Worker read no body for a `DELETE` in two places, the router and the object.
      - Both servers needed a migration for `public_id`. Native adds the column when it opens an
        existing file. The Worker's object adds it on first use, which was checked by opening data
        made by `master`'s code with the new code.
      - `check` and account deletion share one verifier function on each server, so they share
        the attempt limit by construction; the suite cannot read that limit to test it.
      - A push the server refuses whole, such as a frozen account, now arrives in
        `PushedChanges.error` rather than as an `Err`.
      - Every `sync_live.rs` test passes against two native servers.
- [x] **T178d** — *named by T178a's design, and again by T178c's.* A record a module skipped reaches
      it once the app can read it. T178a's L4 remembers such a record's version but never agrees on
      it, so an older app no longer deletes or overwrites it; but the cursor has moved past it, and
      after an upgrade nothing delivers it again. Design:
      [2026-09-22-t178d-a-skipped-record-is-delivered-again-design.md](../specs/2026-09-22-t178d-a-skipped-record-is-delivered-again-design.md).
- [x] **T189** Sync asks before it pulls: `POST /v1/records/heads` names the collections another
      machine changed, in one request per run instead of one per collection, and moves the cursor
      past this machine's own writes so their echo is never fetched. Mandatory in `/v1`, on both
      servers and in the conformance suite. Design:
      [2026-09-28-t189-sync-asks-before-it-pulls-design.md](../specs/2026-09-28-t189-sync-asks-before-it-pulls-design.md).

      **Done.** Both servers answer the route, and `server/conformance/src/heads.test.ts` holds
      them to it, the reaper instances included. `engine::stale` asks, and moves the cursor of every
      collection it was not told about, never backwards; `sync_heads` carries it to `loop.ts`, whose
      full run asks once and pulls only what the answer names. A push that meets its own
      unremembered write at the same stamp writes it again instead of handing it back (D5).
      `sync_live.rs` proves the echo is stepped over against two native servers. An account nobody
      has written to keeps its cursors at 0, and so is pulled every full run until something is.


**Milestone M30** — on two machines: a fresh install signs in and reproduces exactly the
collections that were ticked, with the rows that were not ticked absent; revoking a device from the
other machine ends its next sync; and **the server's SQLite file, opened by hand, yields no
plaintext** — no host name, no URL, no collection name, nothing but opaque ids and ciphertext. The
last of those is the milestone the other two exist to protect.

And one more, which is what [ADR 0046](../decisions/0046-the-sync-server-lives-beside-the-client-it-serves.md)
was decided to buy: **one CI run passes `server/conformance/` against both implementations**, so
`/v1` is demonstrably a protocol rather than a description of whichever server was written first.
