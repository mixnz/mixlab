# The sync protocol, `/v1`

**This is the normative contract.** `server/conformance/` is written against *this document* and
against neither implementation, which is the only reason a suite could be written before the first
server existed. The Cloudflare Worker in `server/worker/` and the native binary in
`server/native/` both answer to it; when one disagrees with what is written here, the
implementation is the one with the bug.

**It is kept here rather than in the design spec because `/v1` grows.** A spec is a point in time
and stops being edited once its work is implemented
([plans-and-specs](../standards/plans-and-specs.md)); this reference is edited whenever a route is
added. The design and the arguments behind it stay in
[the T177 design](../specs/2026-09-20-t177-a-copy-only-you-can-read-design.md) — D1 to D4 for the
shape, D4b for copying and deleting an account, D5 to D9 for everything around it.

**The `D` labels are kept exactly as they were.** 123 comments in `server/` cite them, and a label
names a section rather than a file, so every one of them still lands here.

---

## D4a. The wire

D4 is a table of intentions. This appendix decides the bytes, because `server/conformance/` is
written against **this document** and against neither implementation — and a suite can only be
written first if the document decides first. Nothing below is a new decision; each line is
something D4 left open that two implementations would otherwise settle differently and discover in
the suite.

**The numbers here are configuration, not protocol.** Every limit is reported by
`/v1/capabilities`, and a server may report any value it likes. The suite asserts that a limit is
present and that the server honours the value it reported — never that it equals the default. The
defaults are what this project's instance ships with.

### Encoding, and what both sides ignore

- JSON, UTF-8, `Content-Type: application/json`. Field names are camelCase, as in D3.
- Opaque ids — `collection` and `id` — are lowercase hex, 64 characters. Everything else that is
  bytes (`nonce`, `ciphertext`, `a`, `saltAccount`, `wrappedMkPassword`, `wrappedMkRecovery`) is
  **standard base64 with padding**, not base64url. One spelling, written down here, because two
  implementations will otherwise each pick a reasonable one.
- **Every one of those has a fixed length except the ciphertext**, and a server refuses anything
  else with `400 invalid-request`:

  | Member | Bytes | Why that many |
  | --- | --- | --- |
  | `a` | 32 | HKDF-SHA256 output (D2) |
  | `saltAccount` | 16 | So an invented salt cannot be told apart by length |
  | `wrappedMkPassword`, `wrappedMkRecovery` | 72 | XChaCha20-Poly1305 over 32 bytes: 24 nonce, 32 sealed, 16 tag |
  | `nonce` | 24 | XChaCha20-Poly1305 (D3) |

  This is not fussiness. The account row is the one thing the per-account quota does **not** count,
  so without a bound `POST /v1/auth/register` takes as many bytes as anybody cares to send and
  stores them for ever. Fixing the lengths also turns a client bug into a `400` instead of a row
  nobody can decrypt.
- **Both sides ignore members they do not recognise**, in requests and in responses. This is
  [ADR 0019](../decisions/0019-an-added-response-member-is-optional.md)'s rule applied to two
  parties that upgrade separately: a server somebody else is running is older than this document
  (D9, R4), and a client that refused an unknown member would break the moment a newer server
  added one.
- `Authorization: Bearer <access token>` on every route except `/v1/capabilities`.
- Times on the wire are **seconds** since the epoch. `updatedAt` is the client's clock and the
  server stores it without ever comparing it (D1).

### The account key

```
account_key = SHA-256("mixlab-sync/account/v1" || 0x00 || lowercase(trim(email)))   -> 32 bytes, hex
```

**Frozen, and deployment-independent on purpose.** It is the only name an account has. The Worker
uses it as the name of the Durable Object that holds the account; the native server uses it as the
unique key of the row. Both arrive at the same value for the same address, which is what makes a
row mean the same thing on either — see *Moving the default instance* in D8.

It carries no pepper, and that is the trade this makes: a peppered value could not be moved between
servers, which is the whole point of it. What is given up is that somebody holding a stolen
database and a list of candidate addresses can confirm which of them have accounts. What is bought
is that the database holds no addresses at all.

The label is a frozen constant in the same way the five HKDF labels of D2 are: changing it does not
corrupt anything, it makes every existing account unfindable. One vector, which both
implementations assert:

```
alice@example.com -> 176d00c0673f7e1e711ea55a7d9345f43949376bd9777c4854be01448b5b74a4
```

### One shape for every failure

```json
{ "error": { "code": "quota-exceeded", "message": "…", "limit": 20971520, "used": 20971520 } }
```

`code` is a stable identifier a client switches on. `message` is for a log and is **never shown to
a person** — MixLab's strings live in `src/i18n/` and are chosen by `code`. Any further members are
particular to that code and optional.

### `/v1/capabilities`

`200`, no authentication, `Cache-Control: public, max-age=3600`. It never reaches an account object
(D8). *No authentication* means exactly that: an `Authorization` header that is absent, malformed
or expired changes nothing about the answer, because a client reads this route before it has an
account at all.

```json
{
  "protocolVersions": ["v1"],
  "maxRecordBytes": 1048576,
  "maxBatchOperations": 100,
  "maxBatchBytes": 8388608,
  "maxPageRecords": 500,
  "accountQuotaBytes": 20971520,
  "tombstoneRetentionDays": 90,
  "closingOn": null,
  "features": []
}
```

`features` is how a server announces something optional it has; an empty list is a complete v1
server. A client must run against an empty list forever.

**`maxBatchBytes` exists because the other two do not bound a request.** `maxBatchOperations` times
`maxRecordBytes` is a number no server intends to buffer — a hundred records of a megabyte each is
not what batching is for — so without a third figure a client can compose a request that every
limit says is legal and the server refuses at the door. It is the size of the whole encoded body,
and a client chunks by whichever of the three binds first.

**`closingOn` is how a server says it will not be here for ever.** It is a Unix timestamp in
seconds, or `null`, which is what almost every server answers and what an unconfigured one always
answers. A server that names a date is telling clients the operator intends to switch it off then,
so that a person has warning enough to copy the account somewhere else (D4b) instead of finding out
on the day.

**Nothing enforces it.** The server does not refuse a write after the date, does not freeze, and
does not change any other answer; the date passing is not an event in the protocol at all. It is a
notice, and treating it as a deadline in code would turn an operator's estimate into an outage —
including for the operator, who may be running late and would rather the thing kept working.
Operators move dates, and a server that had already locked itself could not.

**It is a timestamp and not a sentence, and carries no link.** MixLab renders the date in the
person's own language and calendar, which it cannot do with prose the server composed; the same
reason `code` exists on every failure rather than a translated `message` (D4a). And a server that
could hand a client a URL to show would be the phishing surface D4b spends a section refusing —
*where to go next* is not a thing the old server gets to say. An operator with more to explain than
a date explains it the way they already reach their people.

**It answers on a route that needs no account**, so a person who has never signed in, or who cannot
sign in any more, still sees it. A client reads this route before every sync, so the notice arrives
without anything having to remember to ask for it.

### Every code a client can meet

A client switches on `code` and never on `message`, so this is the complete list: one dictionary
key each, and a client that handles all of them handles everything `/v1` can say.

**One code per sentence a person would be shown**, which is the rule that decides how fine-grained
this list is. An earlier draft answered a mistyped address, a wrong-length key and an empty batch
all with `invalid-request`, and a wrong verification code with the same `invalid-token` as an
expired session — so a translated application had one string to cover *"check that address"* and
*"you have been signed out"*. Where two situations want different words they get different codes;
where a client can only ever say *"something went wrong, and it is our bug"*, one code is enough.

| Code | Status | Carries | What a person is told |
| --- | --- | --- | --- |
| `invalid-request` | 400 | | Something in what was sent is malformed. In almost every case a client bug |
| `invalid-code` | 400 | | A code from a letter that is wrong, spent or expired |
| `invalid-token` | 401 | | The session is over; sign in again |
| `invalid-access-token` | 401 | | This server is private; ask whoever runs it for the token |
| `invalid-email` | 400 | | That is not an address a letter could reach |
| `invalid-device-name` | 400 | | This machine needs a name |
| `invalid-credentials` | 401 | | That address and password do not match an account |
| `email-not-verified` | 403 | | Confirm the address before signing in |
| `email-taken` | 409 | | That address already has an account |
| `not-found` | 404 | | No such route. A client bug |
| `method-not-allowed` | 405 | | A client bug |
| `unknown-device` | 404 | | No such device on this account |
| `unknown-record` | 404 | | Nothing to delete |
| `already-exists` | 412 | the record | Something was created twice |
| `version-conflict` | 409 | the record | Somebody else wrote first; resolve and retry (D4) |
| `precondition-required` | 428 | | A client bug: no `If-Match` and no `If-None-Match` |
| `record-too-large` | 413 | `limit` | That one item is larger than this server takes |
| `request-too-large` | 413 | `limit` | The whole request is; send fewer at a time |
| `quota-exceeded` | 507 | `limit`, `used` | The account is full |
| `cursor-expired` | 410 | | Away too long; start again from empty (D3) |
| `too-many-requests` | 429 | `retryAfter` | This network has asked too often; try again in so many seconds |
| `too-many-attempts` | 429 | `retryAfter` | Too many tries on this account. A different sentence, and a different thing to be told |
| `account-frozen` | 423 | | A copy is under way; nothing may change until it ends (D4b) |
| `letter-not-sent` | 502 | | The confirmation letter could not be sent, so no account was made |
| `server-misconfigured` | 503 | `missing` | This deployment is not finished. For whoever runs it |
| `server-error` | 500 | | Something went wrong there |

**Anything a framework would answer on its own is wrapped into this shape too** — a method that
does not exist on a route, a body larger than the server will buffer. A client that met a bare
`405` with an empty body would have nothing to translate, and `server/conformance/` checks that
neither implementation produces one.

### `/v1/auth/params`, and the salt that is invented for a stranger

**Without this route a second machine cannot sign in at all.** `A` is
`HKDF(Argon2id(password, salt_account))`, `salt_account` is random and lives on the server, and a
fresh install has the password and the address and nothing else. An earlier draft of D4 had no way
to hand it back, which made *"a person's second machine has what they ticked"* — the milestone this
whole design is for — unreachable.

```
GET /v1/auth/params?email=…  ->  200  {"saltAccount": "<16 bytes base64>", "argon": {"m": …, "t": …, "p": …}}
```

**An address with no account gets an answer anyway**, and it has to be one nobody can tell from a
real one, or this route becomes the cheapest account-enumeration oracle in the protocol:

```
salt_account (invented) = first 16 bytes of HMAC-SHA256(pepper, "salt/v1" || 0x00 || account_key)
```

Three properties, each of which the alternative gets wrong. It is **stable**, so asking twice gives
the same answer — a value that changed between two probes would announce itself. It is
**unguessable**, because the pepper is this deployment's secret and is never shared, so nobody can
compute what a given address *would* get and compare. And it is **the same shape**, which is why
`salt_account` is fixed at sixteen bytes rather than left to the client: a real salt of some other
length would stand out beside an invented one, and a server cannot invent a length it does not
know.

The Argon2 parameters in an invented answer are the ones this server would hand a new account.
**This leaks something small and known**: an account registered with unusual parameters is
distinguishable from a stranger. MixLab derives with one fixed set, so in practice there is nothing
to see; a client that ever offers a choice would be trading that away.

**Rate limited per source.** It is unauthenticated, it can be asked about any address, and on the
Worker every question wakes that address's object whether or not an account is there — so probing
costs the deployment something. The allowance is generous: a person signs in a handful of times,
and a company behind one address may install on fifty machines in a morning.

**`wrapped_mk` is not here, and that is the line this route is drawn around.** The copy wrapped
under the password is what an offline attack needs, and handing it to anybody who asks turns a
password into the only thing standing between a stranger and an account's contents. It travels on
the answer to `/v1/auth/login`, after the verifier matched, and nowhere else.

### A server one company runs for itself

**A self-hosted instance can be closed with a shared token**, and the hosted ones never are.

```
X-MixLab-Access: <whatever the operator chose>
```

When a deployment is configured with one, **every route requires it** — `/v1/capabilities`
included. The point is that somebody who finds the address cannot use the host at all, and a
capabilities document that answered anybody would tell them the server is there, what it allows,
and that it is worth coming back to. A client is given the token by the person setting it up,
before it makes its first request, so there is no order-of-operations problem to solve.

A request without it, or with the wrong one, is **`401`, code `invalid-access-token`** — its own
code, not the `invalid-token` that means a session has ended: one is *ask your administrator for
the token* and the other is *sign in again*, and MixLab cannot pick between those two sentences
from a status alone.

**It is not a password and it is not per person.** It is one string a company knows, changed
whenever they like; changing it locks out every client until each is told the new one, which is the
behaviour they are asking for. It protects the *host*, not the accounts: everything else in this
design — the verifier, the wrapping of `MK`, what the server can read — is exactly as it was, and
somebody who has the token still cannot read a record.

**A wrong one is counted per source**, in the same window as signing in. The operator picks this
string and may pick a short one, so guessing has to cost something.

**The default instances never set it.** Anybody may make an account there; that is what they are
for. A deployment sets it by configuration alone, and a client learns it is needed by being
refused.

### Accounts

| Route | Body in | Out | Refuses with |
| --- | --- | --- | --- |
| `POST /v1/auth/register` | `email`, `a`, `saltAccount`, `argon: {m, t, p}`, `wrappedMkPassword`, `wrappedMkRecovery` | `201`, `{}` | `400 invalid-request` · `409 email-taken` · `429` · `502 letter-not-sent` |
| `POST /v1/auth/verify` | `email`, `token` | `200`, `{}` | `400 invalid-token` · `429` |
| `POST /v1/auth/login` | `email`, `a`, `deviceName` | `200`, `{accessToken, refreshToken, deviceId, expiresIn, wrappedMkPassword, wrappedMkRecovery, accountId}` | `401 invalid-credentials` · `403 email-not-verified` · `429` |
| `POST /v1/auth/refresh` | `refreshToken` | `200`, `{accessToken, refreshToken, expiresIn}` | `401 invalid-token` |
| `POST /v1/auth/password` | `a`, `newA`, `newSaltAccount`, `newWrappedMkPassword` | `200`, `{}` | `401 invalid-credentials` |
| `POST /v1/auth/reset` | `email` alone | `202`, `{}` | `429` |
| `POST /v1/auth/reset` | `email`, `token`, `a`, `saltAccount`, `wrappedMkPassword`, `wrappedMkRecovery` | `200`, `{recordsDeleted: 214}` | `400 invalid-code` |
| `POST /v1/auth/reset` | `email`, `token` | `200`, `{wrappedMkRecovery, ticket, expiresIn}` | `400 invalid-code` |
| `POST /v1/auth/reset` | `email`, `ticket`, `a`, `saltAccount`, `wrappedMkPassword`, `wrappedMkRecovery` | `200`, `{recordsDeleted: 0}` | `401 invalid-token` |

- **`accountId` names the account, not the address.** It is 32 hex characters, random at
  registration, survives a reset, and is never reused: an address deleted and registered again is a
  new account with a new id. A client keys what it remembers by it, because an account moved away
  and back is registered again under the same `MK`, and its `seq` starts over.
- **Registration is not complete until the letter is accepted.** If the provider refuses it, the
  account is removed again and the answer is `502 letter-not-sent`. Keeping the account would be
  worse than it sounds: the address is now taken, so registering again answers `409`, and there is
  no route in `/v1` that re-sends a verification letter. A provider outage would hand somebody an
  address they can never use and never free.
- **Registering over an *unverified* account replaces it**, and sends a fresh letter. `409
  email-taken` is for an address with a **verified** account and for nothing else. Without this, a
  verification token that expires unused — twenty-four hours is not long — leaves the same trap by
  a different road: cannot verify, cannot register, and cannot reset, because a reset is only
  offered to an address that proved itself. Replacing it loses nothing, since D4 forbids writing
  any record before verification, so there is never anything there to lose. It also narrows the
  enumeration below: an address with an unverified account no longer answers differently.
- **`/v1/auth/reset` is one path told apart by which fields are present**: ask for the letter;
  complete with its code and new keys, which deletes the records (D6 case 3); or spend the code for
  the wrapped key and a ticket, then finish with the ticket and keep them (D6 case 2). One path
  because a forgotten password is one thing a person does, in steps.
- **Asking for a reset always answers `202`**, whether or not that address has an account. Unlike
  registration — which has to refuse a taken address and therefore leaks one (see below) — this
  route has no such obligation, so it does not leak.
- **Verification is the gate on signing in, not on writing.** Until an address is verified,
  `/v1/auth/login` answers `403 email-not-verified` and issues nothing, so in v1 **no token exists
  that could reach a record route with `verified` false**. D4's *"no record may be written before
  this"* is therefore enforced at the door, and the `403 email-not-verified` listed on the record
  routes below is defence in depth that `/v1` cannot currently reach. `server/conformance/` asserts
  the login refusal and does not assert the record one, because a suite that claimed to test an
  unreachable path would be claiming something false. Issuing tokens for an unverified address was
  the alternative, and it means handing credentials to whoever typed an address that may not be
  theirs.
- **Both codes are eight Crockford base32 characters**, shown as `XXXX-XXXX`: the same alphabet the
  recovery key uses (D2), without `I`, `L`, `O` and `U`, so nothing read off a screen is ambiguous.
  A server accepts them in any case and with any separators, and is strict about the alphabet —
  the rule `parse_recovery_key` already applies, so a person learns one way of typing a code from
  this product rather than two.
- **A code, and not a link.** A link has to carry an address the server believes it is reachable
  at, which is a second piece of configuration that is silently wrong until the first person clicks
  one — and this is a desktop application, so the person is already in front of the window that
  wants the code. It also removes a class of bug worth naming: mail scanners and link previewers
  fetch every URL in a message, so a link that verified on `GET` would be spent before the person
  read the letter, and a link that did not would need a page with a button. There is no link, so
  there is nothing to prefetch and no page to serve.
- **Eight characters are only safe because guessing is bounded**, so verification attempts are rate
  limited per account and `server/conformance/` asserts that they are. This is the one allowance
  the suite deliberately exhausts; every other limit it only reads.
- **Signing in hands back both wrapped copies of `MK`.** A fresh install has proved the password
  by this point, and without them it has an account it cannot read: `MK` lives nowhere else. This
  is the other half of what a second machine needs, and the reason it is on this answer rather than
  on `/v1/auth/params` is the paragraph above.
- **A verification code lives 24 hours**; a reset code, one hour.
- **`400 invalid-token` covers wrong, expired and already-used alike.** Telling them apart is an
  oracle and buys a client nothing: the remedy is the same sentence in all three cases.
- **`POST /v1/auth/reset` deletes every record** and says how many (D6, case 3). It is the only
  route in `/v1` that destroys data, and the count exists so the client can show what it did rather
  than claim it. It deletes them outright rather than writing tombstones — a tombstone exists to
  tell another machine that something it can read is gone, and after a reset no machine can read
  anything. Every refresh token is revoked with them, so the other machines are signed out rather
  than left syncing an account whose `MK` they still hold and the server no longer serves. `seq`
  does not restart: it is monotonic for the life of the account.
- **`POST /v1/auth/password` re-wraps and does not re-encrypt.** `MK` is unchanged, so no record is
  touched and no `seq` moves (D6, case 1). Every refresh token except the calling device's is
  revoked.

### Devices

| Route | Out | Refuses with |
| --- | --- | --- |
| `GET /v1/devices` | `{devices: [{id, name, createdAt, lastSeenAt, current}]}` | `401` |
| `DELETE /v1/devices/{id}` | `204` | `401` · `404 unknown-device` |

Deleting a device **ends both its tokens at once**, and a request carrying either answers `401`
from the next one. An earlier draft of this appendix let the access token live out its fifteen
minutes, reasoning that closing it immediately would cost a revocation check on every request. That
reasoning was wrong for the servers actually being built: a token here is an opaque string the
server looks up (see below), so the lookup that would notice a revocation is the same lookup that
authenticates the request, and there is nothing to pay. Cutting off a lost machine is the whole
purpose of the route, so it cuts it off now. Deleting your own device is how a person signs out.

### Records

`PUT /v1/records/{collection}/{id}` carries `{updatedAt, nonce, ciphertext}` — **not** `version`,
`seq` or `device`, which are the server's to assign. `DELETE` carries `{updatedAt}`: the time the
deletion was made, which the tombstone keeps as its own so that D4 weighs a deletion like any other
edit; without it the answer is `400 invalid-request`. Both answer with the
stored record of D3 and an `ETag` holding its `version` as a quoted decimal.

**`device` is the id of the device whose session made the write**, as `/v1/devices` lists it. A
`device` in the body is ignored rather than refused, the way every unrecognised field is — a
client cannot claim another machine's writes by naming it. A tombstone carries the device that
deleted it.

| Condition | Answer |
| --- | --- |
| `If-None-Match: *`, no such record | `201` + the record |
| `If-None-Match: *`, it exists | `412 already-exists` + the current record |
| `If-Match: "41"`, current is 41 | `200` + the record, `version` 42 |
| `If-Match: "41"`, current is 42 | `409 version-conflict` + the current record |
| neither header | `428 precondition-required` |
| `DELETE`, `If-Match` matches | `200` + the tombstone |
| `DELETE`, already a tombstone, `If-Match` matches it | `200` + that same tombstone, **no new version and no new `seq`** |
| `DELETE`, never existed | `404 unknown-record` |
| body over `maxRecordBytes` | `413 record-too-large` |
| account over `accountQuotaBytes` | `507 quota-exceeded`, with `used` and `limit` |
| `collection` or `id` not 64 lowercase hex characters | `400 invalid-request` |
| address not yet verified | `403 email-not-verified`, unreachable in v1 — see above |

The tombstone rule is worth its row: without it, a delete retried after a dropped connection bumps
`seq` and every other machine pulls a change that is not one.

**A tombstone is a version like any other.** `If-Match` on its version writes over it and the
record comes back — which is what happens when somebody deletes a saved query on one machine and
the same local uuid is written again from another — and `If-None-Match: *` counts it as existing
and answers `412`. The alternative, treating a deleted row as absent, would let a creation slip
past a deletion and leave the two machines disagreeing about which one won.

`GET /v1/records?collection={c}&since={seq}`:

- **`collection` is optional**, and omitting it means every collection. D4 writes the narrow form;
  the broad one is what a burst sync actually wants, and D8's first rule is about how often a
  client wakes an object rather than how much it carries.
- `since` is **exclusive**, and `since=0` means from the beginning.
- `200`, `{records: [...], nextSince: 903, more: false}`, ordered by `seq` ascending, at most
  `maxPageRecords`. A client that sees `more: true` calls again with `nextSince`. **On a page with
  `more: false`, `nextSince` is the account's latest `seq`** when that is past the last row: no
  record of the collection lies between them, and a cursor stopped at the row would sit below every
  later write, where one reaped tombstone expires it for good.
- `410 cursor-expired` when `since` is older than the oldest surviving tombstone — D3's *"told to
  resync from empty rather than told incomplete news quietly"*, made into a status code.
- `resync=1` says the cursor came from a read that began at `0` — a resync after `410` — which has
  missed nothing, so it is never expired. A client sends it on every page of a resync after the
  first, and nowhere else. Any other value is `400 invalid-request`.

`POST /v1/records/heads` — which collections a pull would bring news of, in one request (T189):

```json
{ "cursors": { "<collection>": 903, "<collection>": 0 } }
```

```json
{ "stale": ["<collection>"], "nextSince": 911 }
```

- **Mandatory in `/v1`**, and not announced in `features`: a client asks it before every full run
  and has no other way to learn what changed.
- A collection is **stale** when `0 < since` and `since` is below the oldest surviving tombstone
  (a pull would answer `410`); when `since` is `0` and the collection holds any row; or when it
  holds a row with `seq > since` written by **another device** than the one asking. A row whose
  device is unknown counts as another's. A collection the asking device alone wrote to since the
  cursor is not stale.
- `nextSince` is the account's latest `seq`, read with the answer. A client moves the cursor of
  every collection it asked about and was not told is stale to `nextSince`: nothing another
  machine wrote lies between, and the rest this device already has.
- `cursors` holds between 1 and `maxBatchOperations` entries, each a collection id and an integer
  `≥ 0`. Anything else is `400 invalid-request`.
- A read: a frozen account answers it.

`POST /v1/records/batch`:

```json
{ "operations": [
  { "op": "put", "collection": "…", "id": "…", "ifNoneMatch": true,
    "record": { "updatedAt": 1758300000, "nonce": "…", "ciphertext": "…" } },
  { "op": "delete", "collection": "…", "id": "…", "ifMatch": 41, "updatedAt": 1758300060 }
] }
```

```json
{ "results": [ { "status": 201, "record": {…} }, { "status": 409, "record": {…} } ] }
```

One result per operation, in the order sent, each carrying exactly the status and body that the
single-record route would have. **The envelope is `200` whatever the entries say** — it is a batch
of independent compare-and-swaps and not a transaction (D4), so a `409` in entry seven is news for
the client, not a failure of the request. `400 invalid-request` when the list is empty or longer
than `maxBatchOperations`; `507` on the envelope only when the account is already over quota.

### Tokens

An access token lives **fifteen minutes**, a refresh token **ninety days**, and a refresh **rotates
on use**: the answer carries a new one and the old one dies. Presenting a rotated refresh token
again revokes that device's whole chain and answers `401` — either it was stolen, or two clients
raced, and both want the person to sign in again rather than to continue quietly.

**They are opaque strings, not JWTs.** The only party that reads a token is the server that issued
it, so the stateless validation a JWT buys has no customer here, and a signed token that cannot be
withdrawn is the wrong shape for a route whose whole purpose is cutting off a lost machine.

### Rate limiting

`429` with `Retry-After` in seconds. The limits are configuration and are not reported by
`/v1/capabilities` — publishing the number that stops abuse helps only the abuser. The suite
asserts the shape of the refusal and never trips it deliberately.

**Two different abuses, two different counters.** A count kept per account cannot see somebody
working through a list of addresses, and a count kept per source cannot see somebody working
through one account from a botnet. Guessing at one account is counted against that account;
opening accounts, asking for letters and asking where a salt is are counted against the source.

#### A letter is counted against the address it is sent to

`register` and `reset` are the only routes that send one, and both are counted per source — which
bounds what one network can send, and **bounds nothing at all about what one mailbox receives**.
An address is a fixed target: somebody with a hundred sources can ask a hundred times, and every
one of those letters lands in the same inbox. That is a mail flood aimed at a person, and a bill
aimed at whoever runs the server.

So there is a second allowance, counted **per address per hour**, and it is small: a person who
did not get the letter asks again once or twice, not thirty times.

- On `reset`, an address over the allowance still answers **`202`**, and no letter is sent. It
  cannot answer `429`: this route answers alike for an address that has an account and one that
  does not (D4a), and a refusal that only throttled addresses could meet would tell an attacker
  which is which.
- On `register`, it answers `429`. Registration already refuses a taken address with `409`, so it
  has no secret left to keep, and the caller being told is the one asking for the letters.

**This counter does not live with the account**, because registering over an unverified account
replaces that account — and everything that hangs off it. A counter the counted party can clear by
re-registering is not a counter. It is kept against the hash of the address, in the same place the
per-source counters live, and it outlives both the account and its deletion.

#### What a source is, and why the server decides it

A source is an address, hashed. **The server works out which address; it never takes the request's
word for it.** `X-Forwarded-For` is a request header like any other — a server reachable directly
that believed it would let anybody mint a fresh bucket per request by writing a different value,
which is not a weakened limit but no limit at all.

So a source is the peer address of the connection, unless the deployment says otherwise. A server
behind a reverse proxy sees only the proxy and must be told to read the header instead; that is
one setting, off by default, and `server/native/README.md` names it. When it is on, the value read
is the **last** entry rather than the first: a proxy that appends leaves the address it saw at the
end, and a client that writes its own value leaves it at the front, so the end is the only part a
client cannot choose.

The hosted Worker has neither problem: Cloudflare sets `CF-Connecting-IP` and a request cannot
reach the Worker without passing through it.

**A server that cannot tell its callers apart says so by putting them all in one bucket**, rather
than by not counting. That is the honest reading of a missing address, and it is what a server
reached over a Unix socket or from a test harness gets.

### The one door that is not `/v1`

Verification arrives by email, which no HTTP suite can read. A server under test therefore serves
`GET /__test__/outbox?email=…`, returning the tokens it would have sent, **and answers `404` unless
it was started with that mode explicitly enabled**. It is outside `/v1` so that it is never part of
the protocol, and a deployed server cannot be asked for it. Both implementations carry it, because
`server/conformance/` requires it.

```json
{ "messages": [ { "kind": "verification", "token": "…", "sentAt": 1758300000 } ] }
```

Oldest first, so the newest of a kind is the last one. `kind` is `verification` or `reset`. This
shape is written down for the same reason everything else here is: it is the seam between the suite
and both implementations, and a seam nobody specified is a seam that differs.

### Four choices that could have gone the other way

1. **`409 email-taken` lets an attacker learn which addresses have a *verified* account.** The alternative —
   always answer `201`, and send a *"somebody tried to register your address"* letter instead — is
   what a password manager does, and it costs a client that cannot tell a person they already have
   an account, plus a new way to send mail to a stranger. The leak it prevents is *"this address
   uses MixLab"*, against a threat model (D1) that is about the server operator and whoever takes
   the database, not about an enumerator. Rate limiting makes the sweep slow and loud. **Revisit
   this if MixLab ever holds something where membership itself is sensitive** — this is a developer
   tool, and it does not.
2. **A code the person types, rather than a link they click.** The link is the obvious design and
   it loses on three counts: it needs the server to know its own public address, which is a setting
   that is wrong silently; it is fetched by mail scanners before the person reads the letter, so
   either it does not verify on `GET` and needs a page with a button, or it is spent; and the
   person is already looking at the window that wants it. The cost is eight characters of typing
   and a rate limit that has to be real.
3. **Reusing a rotated refresh token revokes the chain** rather than being ignored. It is the one
   signal this design gets for free that a token has been copied.
4. **`updatedAt` is never compared by the server**, including here, where it would have been easy
   to reject a write whose clock runs backwards. D1 promises the server compares nothing; a client
   with a wrong clock is a client problem, and a server that enforced monotonic clocks would be
   unable to accept a legitimate write from a machine that had just fixed its own.

## D4b routes

The two routes D4b adds. **Why they are shaped this way is in D4b**; what a client sends and
receives is here.

### Two states

| State | Reads | Writes | What it means |
| --- | --- | --- | --- |
| `active` | yes | yes | The normal state |
| `frozen` | yes | **no** | A copy is under way, and nothing may change under it |

`GET /v1/account/freeze` answers `{"state": …, "frozenAt": <seconds> | null}`. `POST` takes
`{"state": "active" | "frozen"}` and moves between them. Both need an access token, and **`GET`
answers in both states** — a machine that meets a refusal has to be able to find out why.

**Thawing is a route, and the only way out.** Posting `active` is how a client ends a copy it has
finished and how it abandons one it has given up on. Nothing else ends a freeze, and the section
below is about why nothing else should.

Reads stay open while frozen because **the copy is a read**: the machine doing the work needs the
same route any machine uses, and a second machine that wants to take over needs it too.

### Deleting an account

| Route | Body in | Out | Refuses with |
| --- | --- | --- | --- |
| `POST /v1/account/delete` | `a` | `200`, `{recordsDeleted: 214}` | `401 invalid-credentials` · `401 invalid-token` · `429 too-many-attempts` |

### Checking the password

| Route | Body in | Out | Refuses with |
| --- | --- | --- | --- |
| `POST /v1/account/check` | `a` | `204` | `401 invalid-credentials` · `401 invalid-token` · `429 too-many-attempts` |

`check` answers whether `a` is this account's, under an access token. A move asks it before it
registers anywhere with the password just typed, so a typo is refused at the first step rather than
becoming the new server's password. It counts where login and `delete` count, in the same
per-account window, so it is no cheaper way to guess a password than signing in.
