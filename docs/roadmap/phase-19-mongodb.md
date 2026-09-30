# Phase 19 — MongoDB

*Goal: the MongoDB releases `mixengine-packages` publishes install and run as a service, what they
ask of the processor is read before they download, and MixLab opens one.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-17-t153-mongodb-is-a-service-design.md](../specs/2026-09-17-t153-mongodb-is-a-service-design.md).

---

**The case this comes from**: `mixengine-packages` published `mongodb` 6.0 through 8.3 on five
targets on 2026-09-15. Every PHP MixEngine installs carries the `mongodb` extension and MixLab
browses a MongoDB, and nothing in this repository could run one: a package with no recipe is not
offered, so the five releases were invisible to `mix` and to the window alike. T148's design left
the other half here too — `requires.cpu` was modelled and not judged until a kind that states it
became installable.

## The processor

- [x] **T153** **(P)** `cpu: avx` is judged. `MachineFacts` gains `avx`, answered by
      `is_x86_feature_detected!` in an x86_64 build and could-not-tell in any other; an x86_64
      artifact stating `avx` on a processor certainly without it is `Need::Cpu`, whose remedy is
      `Unavailable` because every MongoDB release needs it. MixLab labels it. Design D5.

## The server

- [x] **T154** **(P)** The `mongodb` recipe. A rendered `mongod.conf`, `mongod --config` with
      `--nounixsocket` everywhere but Windows, ready on its own `Waiting for connections` line,
      healthy on an accept, stopped by a signal. No accounts, so a bind address off loopback is
      refused and `mixengine-elevate` never opens 27017. `DatabaseProtocol::Mongodb`. Design D1–D4.

## The window

- [x] **T155** MixLab opens it. `<old>://connect?kind=mongodb` and the Services screen's Open both
      build one `mongodb://…?directConnection=true` URI, and `database.client` answers
      `creates_databases` so the Create form is not drawn for a server that makes none. Design D6.

## The proof

- [x] **T156** **(P)** A real MongoDB judges the recipe: `tests/mongodb.rs` installs 8.3.11 from a
      mock registry, writes a document, restarts, reads it back and stops, and CI runs it on all
      three test legs. Design D7.

## The gallery

- [x] **T164** Two blueprints ask for it — `laravel-mongodb` (PHP-FPM, `[php] extensions =
      ["mongodb"]`, `laravel`'s scaffold) and `express-mongodb` (a `node-app` on 3000, no scaffold).
      Both name `mongodb@main` and no `database`, because the recipe makes none, and both say in
      their description that MongoDB needs AVX. See `docs/features/blueprints.md`.

## Follow-ups

Not started, and each needs a design of its own (D8):

- Accounts and `database.create` for MongoDB — access control, the first user, SCRAM for every
  probe, a credential in the handoff.
- A `mongosh` kind that `<root>/bin` fronts, which needs the package model to hold a client-only
  kind — and with it a clean Windows stop, `db.adminCommand({shutdown: 1})`, in place of a kill.
- A single-node replica set, for transactions and change streams.
- A check before a spawn that a service's declared ports are free: on Windows a `mongod` shares a
  port another program is listening on, and announces itself ready.

## Milestone

**M19** `mix package install mongodb 8.3.11` and `mix service create mongodb@main 8.3.11` produce a
server that keeps a document across a restart, and MixLab's Open lands in a Mongo tab connected to it
— **the first half measured** on Windows on 2026-09-17, by `tests/mongodb.rs` against 8.3.11: a
document inserted, the service restarted (a kill, on that system) and the document read back. The
MixLab click waits for a person at the window.
