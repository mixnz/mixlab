---
status: implemented
date: 2026-09-17
task: T153
---

# T153 — MongoDB is a service, and MixLab opens it

Roadmap tasks [T153–T156](../roadmap/phase-19-mongodb.md), phase 19. 2026-09-17.

**The case this comes from**: `mixengine-packages` finished its P18 and P18a on 2026-09-15 and the
published index now carries `mongodb` 6.0.29, 7.0.43, 8.0.32, 8.2.12 and 8.3.11 on five targets,
plus `mongosh` 2.11.1. Every PHP MixEngine installs already carries the `mongodb` extension, and
MixLab's `db` module already browses a MongoDB. Nothing in this repository can run one: `package.*`
lists only what a compiled-in recipe can start, so the five releases are invisible to `mix` and to
the window alike.

The packaging design left this half to this repository in so many words — *"what runs `mongod` —
a rendered `mongod.conf`, a data directory made once, a port, a health check — is MixEngine's side"*
— and handed over the contract it was to be written against:

```
kind        "mongodb"
provides    { "mongod": "bin/mongod[.exe]", "mongos": "bin/mongos[.exe]" }
requires    glibc (Linux), macos (macOS), vcredist 2022 (Windows), cpu "avx" (every cell)
```

T148's design (D2) left one more thing here, also in so many words: *"`cpu` is modelled and not
judged … the task that makes MongoDB installable is where it is read."*

## What is already true

- **A recipe is what makes a package installable.** `mixengine-daemon/src/packages.rs` filters the
  index through `Catalogue::builtin()` and refuses `package.install` for a name with no recipe. A
  `mongodb` recipe is therefore the whole of what makes the kind appear in `mix package available`
  and in MixLab's Packages screen, whose `packageCategories.ts` already files `mongodb` under
  *database*.
- **Redis is the precedent for a server with no accounts.** `DatabaseProtocol::Redis` answers
  `has_accounts() == false`; `database.open` and `database.credentials` refuse `--user` for it, a
  handoff carries no credential, and MixLab dials it the moment it arrives (`handoffArrival.ts`).
- **Memcached is the precedent for a package with no client of its own.** Its readiness is an
  accept and its health probe a TCP connect, because the artifact has nothing to ask with.
- **`Generator::render` creates every service's data directory** before the first start
  (`generate.rs`), so a server that needs only an existing, empty directory needs no ritual.
- **`Requires::cpu` parses** (`index/format.rs`) and `requirements::unmet` ignores it; the test
  `nothing_uncertain_is_ever_a_lack` asserts `{"cpu": "avx"}` against an unknown machine is not a
  lack.
- **MixLab's Mongo connection is one URI.** `ConnectionConfig.uri` carries host, port and default
  database for kind `mongo`; `username`, `password` and `database` are ignored for that kind. The
  `<old>://connect` parser (`src-tauri/src/modules/db/handoff.rs`) refuses `kind=mongo` today, and
  `open_in_<old>.rs` maps only `mysql`, `postgres` and `redis`.
- **`mixengine-elevate` refuses to open a database's port to the network** by a compiled-in list,
  `NEVER = [3306, 5432, 6379, 11211, 1025, 8025]` in `firewall.rs`.

## What was measured before a line was written

On this Windows machine, against `mongodb-8.3.11-windows-x86_64.zip` from `mixengine-packages/dist`,
unpacked to a directory whose path contains a space:

1. **`net.unixDomainSocket.enabled` is refused on Windows** — `Unrecognized option:
   net.unixDomainSocket.enabled`, exit before anything starts. The option, and its command-line
   spelling `--nounixsocket`, exist only on Unix builds.
2. **A configuration naming only `net.bindIp`, `net.port`, a single-quoted `storage.dbPath`,
   `storage.wiredTiger.engineConfig.cacheSizeGB: 0.25` and `setParameter.
   diagnosticDataCollectionEnabled: false` starts**, with `--config "<absolute path>"`. The data
   directory path with a space and backslashes arrives intact inside YAML single quotes.
3. **Readiness is announced as one structured line**: `"id":23016,"ctx":"listener","msg":"Waiting
   for connections","attr":{"port":27817,"ssl":"off"}`. It is printed after WiredTiger has opened and
   recovery has finished.
4. **A forced kill is recovered from.** `Stop-Process -Force` (`TerminateProcess`, what the
   supervisor's kill is on Windows) followed by a start logs `Detected unclean shutdown`, `Recovering
   data from the last clean checkpoint`, and reaches `Waiting for connections` 0.55 s later.
5. **On Windows a second `mongod` binds a port the first is listening on**, and both announce
   `Waiting for connections`. `mongod` sets `SO_REUSEADDR`, whose Windows meaning is *share*. On
   Unix the same flag does not permit a second listener, and the second server exits.
6. **`Access control is not enabled for the database`** is logged as a startup warning, which is
   the arrangement D2 below chooses and has to answer for.

## D1 — One recipe, `mongodb`, and nothing it does not need

`crates/mixengine-core/src/generate/recipes/mongodb.rs`, registered in `Catalogue::builtin()`.

| Question | Answer | Why |
| --- | --- | --- |
| `package()` | `"mongodb"` | the index's kind |
| `instancing()` | `Named` | `mongodb@main` beside `mongodb@legacy`: two ports, two data directories, as every database |
| `preferred_port()` | `27017` | the product's documented port; the allocator moves a second instance above it |
| `protocol()` | `Some(DatabaseProtocol::Mongodb)` | MixLab opens it |
| `administrator()` | `None` | no accounts — D2 |
| `ritual()` | `None` | the generator creates the empty data directory, and `mongod` initialises it itself |
| `databases()` | `None` | a MongoDB database exists once something writes to it; there is no account to make |
| `clients()` | empty | the artifact publishes `mongod` and `mongos`, a server and a router, and no shell (D8) |
| `smoke_test()` | `mongod --version` | the cheapest run that proves the binary loads on this machine |
| `validator()` | `None` | `mongod` has no mode that reads a configuration without becoming a server |
| `idle_probe()` | `Connections { port }` | as Redis and memcached |
| `idle_default()` | one hour | T70a's number for every database; `held_while_stopped` makes it wakeable |
| `held_while_stopped()` | `[Tcp(bind:port)]` | it listens on no socket (D3) |
| `restart_over_memory_default()` | `false` | a database loses a write |
| `certificate()` | `None` | TLS to a loopback development server is not asked for |

`spec()` refuses, by name, a row with no port, an install that publishes no `mongod`, and a bind
address that is not loopback (D2).

## D2 — No accounts, and two locks in their place

**MongoDB runs with access control off**, which is Redis's arrangement and MongoDB's own default.
Turning it on means creating the first user through the *localhost exception* and then
authenticating every readiness check, health probe and handoff — which needs a client speaking
SCRAM, and the artifact ships none (D8). A development server that a PHP application, a Node
application and MixLab all reach over loopback with no credential is what a person installing a
local MongoDB expects.

What a password would have protected is then protected twice, and neither lock depends on the other:

- **The recipe refuses a bind address that is not loopback.** `service.create` accepts `bind_addr`
  for every service; for this recipe `spec()` parses `Context::bind()` and refuses anything that is
  not a loopback `IpAddr` with `Error::SettingValue { key: "bind_addr", … }`, whose reason says that
  a MongoDB with no accounts on a network address is a database anyone on that network can read and
  delete. An address that does not parse is refused the same way rather than defaulted.
- **`mixengine-elevate` never opens 27017.** `NEVER` gains `27017`, and its test gains the port.
  This is the helper's own rule, checked without trusting the daemon, which is where the security
  model puts it.

A second instance that the allocator moved to 27018 is not in `NEVER`, exactly as a second MariaDB
on 3307 is not; the first lock is what covers it, because a firewall plan only ever names a site's
web ports.

## D3 — What is rendered, and what is passed

One file, `etc/<service-id>/mongod.conf`:

```yaml
# Generated by MixEngine for {{ service.id }} from {{ package.name }} {{ package.version }}.
net:
  bindIp: {{ service.bind }}
  port: {{ service.port }}
storage:
  dbPath: '{{ paths.data | replace("'", "''") }}'
  wiredTiger:
    engineConfig:
      cacheSizeGB: {{ settings.wiredtiger_cache_mb / 1024 }}
setParameter:
  diagnosticDataCollectionEnabled: false
{{ extra }}
```

- **`dbPath` in single quotes, with a quote doubled.** YAML's single-quoted scalar has exactly one
  escape, `''`, so a Windows path's backslashes arrive as written (measured, item 2). A double-quoted
  scalar would read `\U…` as an escape.
- **No `systemLog`.** With no destination `mongod` logs to standard output, which is the stream
  `mix service logs` reads; a `path` would be output nothing captures.
- **No `processManagement.fork`.** Its default is `false`; a forking server is one the supervisor
  loses.
- **`cacheSizeGB` from a whole number of megabytes.** `wiredtiger_cache_mb` is a `Preset::Number`
  defaulting to `256`, so an override is a typed integer rather than free text; minijinja's `/` always
  yields a float, so `256` renders `0.25` and `1024` renders `1.0`. WiredTiger's own default is half
  of RAM minus 1 GB, which on a 16 GB laptop reserves 7.5 GB for a development database. 0.25 is
  `mongod`'s minimum; a smaller override is refused by `mongod` at start with its own message.
- **Full-time diagnostic data capture off.** It writes a rolling `diagnostic.data/` of up to 200 MB
  into the data directory for MongoDB's support engineers; the dev-tuned defaults of T73 make the same
  trade.
- **`{{ extra }}` last**, as every template; a key repeated there is a YAML error `mongod` reports
  at start.

The command line is `mongod --config <absolute path to mongod.conf>`, plus **`--nounixsocket` on
every system but Windows**, decided with `cfg!(windows)` as `mariadb.rs` decides its own per-system
arguments. Without it a Unix `mongod` creates `/tmp/mongodb-<port>.sock`: a second address nothing in
MixEngine diagnoses, holds while idle or tells a client about, in a directory shared with every other
program on the machine. The flag is on the command line rather than in the file because the file's
spelling of it is a refusal on Windows (measured, item 1) and a template has no system to branch on.

Working directory: the data directory. Settings: `wiredtiger_cache_mb` (256), `ready_timeout_ms`
(60 000), `stop_grace_ms` (10 000). Nothing else is offered as a typed override; `extra` is there for
the rest.

## D4 — Ready, healthy, stopped

- **Ready is the log line**, `ReadyCheck::LogPattern { regex: "Waiting for connections", timeout }`.
  An accept is weaker in a way that matters here: on Unix a `mongod` that lost its port to another
  server exits, but a TCP check can connect to the *other* server in the window before it does, and
  report this service ready. The line is printed by this process, after recovery (measured, item 3).
- **Healthy is an accept**, `HealthProbe::Tcp`, three failures before `Degraded`. There is no client
  to ask anything more of it (D8); memcached is the precedent.
- **Stopped by a signal**, `StopBehaviour::Signal { grace: stop_grace_ms }`. On Unix that is
  `SIGTERM` to the group, which `mongod` handles as a clean shutdown. **On Windows it is a kill**, by
  ADR 0008, and the design says so rather than hiding it: WiredTiger recovers from it (measured, item
  4), and what is at risk is a write acknowledged without `j: true` inside the last journal commit
  interval, 100 ms by default. A clean Windows stop needs a program that can send `shutdown` over the
  wire, which is D8's follow-up.
- **No reload.** `mongod` re-reads nothing; a changed rendering is a restart.

**The Windows port-sharing finding (item 5) is not solved here**, and it is not MongoDB's alone:
`service.create` allocates a port by binding it, which is what keeps two MixEngine services apart,
and a program outside MixEngine that starts on the same port later is a conflict no recipe can see.
A check before spawn that the declared ports are free would cover every recipe at once; it is filed
as a follow-up in the roadmap rather than built into one recipe.

## D5 — `cpu: avx` is judged

**The probe is the platform layer's.** `MachineFacts` gains `avx: Probe<()>`, answered by one
function in `traits/machine.rs` that every system's `Facts` calls: `Present(())` or `Absent` from
`std::arch::is_x86_feature_detected!("avx")` in a build for `x86_64`, and `Unknown` in any other
build. The macro checks the CPUID bit *and* that the operating system saves the AVX state
(`OSXSAVE`/`XGETBV`), which is the question `mongod` actually depends on. `MachineFacts::unknown()`
gains `avx: Probe::Unknown`, so every existing test and the mock stay "knows nothing".

**An ARM64 build of the daemon answers `Unknown`.** It cannot ask an x86 question, and on Windows on
ARM the x86_64 artifact it would install runs under the operating system's emulation, whose AVX
support depends on the Windows release. `SmokeTest` decides that case, as T148 intends for every
uncertainty.

**The judgement** (`requirements::unmet`): `requires.cpu == Some("avx")` on an artifact whose
architecture is `X86_64`, against `facts.avx == Absent`, is a lack. Any other feature word, an
`Aarch64` artifact (the index states `avx` on those too), and `Present` or `Unknown` are not.

**The wire** gains `Need::Cpu { feature: String }`: label `CPU with AVX` — `format!("CPU with
{}", feature.to_uppercase())` — and display `a processor with AVX, and this one does not have it`.
Its remedy comes out of `judge` unchanged: every MongoDB release requires AVX, so `newest_met` finds
none and the remedy is `Unavailable`. MixLab's `requirementStep.ts` gains the `cpu` arm of its
label switch.

## D6 — MixLab opens it

**The protocol word is `mongodb`**: `DatabaseProtocol::Mongodb`, `as_str() == "mongodb"`,
`has_accounts() == false`. It is the URI scheme and the package name; MixLab's own `DbKind` is
`mongo`, and the two sides map between them.

**Both doors build the same URI**, through one function in `src-tauri/src/modules/db/handoff.rs`:

```rust
pub fn mongo_uri(host: &str, port: u16, database: Option<&str>) -> Result<String, AppError>
```

It answers `mongodb://<host>:<port>/<database>?directConnection=true`, where:

- `host` is accepted only as an IP address or a name made of ASCII letters, digits, `.` and `-`;
  anything else is `error.handoffInvalid`. The URL a web page can hand `<old>://` is the reason: a
  host of `a/?authSource=x` would otherwise write options into the connection string. An IPv6
  address is bracketed.
- `database` is percent-encoded outside RFC 3986's unreserved set, and omitted — `/?…` — when
  absent.
- `directConnection=true` because a standalone development server is not a replica set, and a
  driver told nothing tries to discover one.

`parse` accepts `kind=mongodb` and fills `ConnectionConfig { kind: Mongo, uri: Some(mongo_uri(…)),
username: None, password: None, database: None, … }`. `kind=mongo` stays refused, as the existing
test asserts: the word on the wire is the protocol's. `open_in_<old>.rs` maps `"mongodb"` to
`DbKind::Mongo` and fills `uri` the same way, with `database` the argument it was given.

**A MongoDB handoff dials at once**: `arrivesConnected` answers `true` for kind `mongo` as it does
for `redis` — there is no password to wait for.

**The Create form is not drawn for a server that makes no databases.** `DatabaseClientReport`
gains `creates_databases: Option<bool>` under ADR 0019 — `#[serde(default, skip_serializing_if =
"Option::is_none")]`. The daemon answers `Some(recipe.databases().is_some())` whenever it answers a
protocol, and `None` otherwise. `DatabasePanel.tsx` keeps the panel (the credential line and the
title) and leaves out the Create group exactly when `creates_databases === false`; an older daemon
that sends nothing keeps today's form. The answer is the daemon's, so no client learns which products
make databases. Redis is answered `false` by the same rule, which removes a button that could only
ever be refused.

**Wire compatibility.** `Mongodb` and `Cpu` are new enum variants in responses. A `mix` older than
the daemon it talks to fails to decode a response that contains one — `database.client` for a
MongoDB service, a requirement list for a MongoDB release — and nothing else. That is the same skew
T88c describes, bounded by the same restart, and it is not a change an older peer cannot survive for
the calls it could already make, so `PROTOCOL_VERSION` does not move. `bindings/` is regenerated.

## D7 — How it is proven

**In one process**, beside each change: the recipe's rendering, quoting, arguments per system, spec,
refusals and catalogue answers (`recipe.rs`'s table tests grow a `mongodb` row: no administrator, no
certificate, one hour idle, 27017, `mongod` and `mongos` among the programs no client may front);
`requirements::unmet` for each branch of D5; the platform function's answer on the machine running
the test; `handoff::mongo_uri` and `parse` for accepted and refused hosts; `open_in_<old>`'s mapping;
`handoffArrival` and the `requirementStep` label in vitest; the daemon's `database.client` answering
`creates_databases` for MariaDB (`true`), Redis and MongoDB (`false`) and memcached (absent);
`firewall.rs` refusing 27017.

**Against a real server**, `crates/mixengine-cli/tests/mongodb.rs`, `#[ignore]`d and driven by
`MIXENGINE_MONGODB_PACKAGE` on `redis.rs`' pattern: a mock registry offering the unpacked artifact,
`mix package install`, `mix service create mongodb@main <version> --port <free>`, `start`, then over
the wire — a `hello`, an `insert` of one document, `restart`, a `find` that returns it (the data
directory is a database's and survives), `stop`, and nothing answering afterwards. The wire is spoken
by about eighty lines of `OP_MSG` and BSON in the test, for the reason `mongodb_smoke.py` gives in
`mixengine-packages`: a server test that needs a separately released client goes red for reasons
that are not about the server. The rendered file is read back and asserted to name the chosen port.

**In CI**, a `Fetch a real MongoDB` step (8.3.11) on the `test` job's three legs — skipped with a
notice on Windows on ARM, as Redis's is — a `Test against a real MongoDB` step for Windows and
macOS, and a block in `.github/scripts/test-no-network.sh` for Linux, with
`MIXENGINE_MONGODB_PACKAGE` added to the variables that script forwards. The Linux leg runs on
`ubuntu-latest`, whose glibc (2.39) meets the artifact's 2.34.

## D8 — What this does not do

- **No accounts and no `database.create`.** Access control, the first user, SCRAM for every probe
  and a credential in the handoff are one design, and they wait for a client that can speak it.
- **No `mongosh` kind.** A package with no recipe cannot be installed, and a shell is not a server; a
  `mongosh` that `<root>/bin` fronts needs the package model to hold a client-only kind, which is a
  decision of its own. With it would come a clean Windows stop (`db.adminCommand({shutdown: 1})`).
- **No replica set.** A single-node replica set is what transactions and change streams need; it
  needs `replSetInitiate` sent after the first start, which is a client again.
- **No `mongos`.** It is published because upstream publishes it; nothing here routes a cluster.
- **No pre-start port check.** D4's Windows finding is filed, not fixed.
- **Nothing in `mixengine-packages`.** The contract is taken as published.

## Documentation

- `docs/features/services.md` — a MongoDB row in the catalogue, `mongodb@main/mongod.conf` in the
  generated tree, MongoDB beside Redis and memcached in *First-start initialisation*, and a paragraph
  saying it has no accounts and what stands in their place.
- `docs/guide/en/services.md` and `docs/guide/vi/services.md` — the row and one sentence each.
- `CHANGELOG.md` — each task adds its own line under `## [Unreleased]` in the commit that lands it:
  the AVX refusal (T153), MongoDB as a service (T154), MixLab opening it and dropping the Create form
  for a server that makes no databases (T155).
- `docs/roadmap/phase-19-mongodb.md` — created by T153 with all four tasks unticked and D8's
  follow-ups listed after them, with a row in `todo.md`; each task ticks itself in its own commit.

## Tasks

- **T153** — `cpu: avx` is judged: `MachineFacts::avx` and its probe, `Need::Cpu`, `unmet`, `mix`'s
  rendering through `Display`/`label`, MixLab's label, bindings. D5.
- **T154** — The `mongodb` recipe: `DatabaseProtocol::Mongodb`, the recipe and its template, the
  catalogue, `NEVER` gains 27017, bindings, the feature spec and the guide. D1–D4.
- **T155** — MixLab opens it: `creates_databases`, `mongo_uri`, `parse`, `open_in_<old>`,
  `arrivesConnected`, `DatabasePanel`, bindings. D6.
- **T156** — A real MongoDB judges the recipe: `tests/mongodb.rs`, the CI steps, the namespace
  script. D7.

## Verification

- `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check` and
  the rustdoc gate after each task; `bash packaging/bindings.sh --check` after T153, T154 and T155.
- `cd apps/desktop && npm run build && npm test && npm run lint`, and `cargo clippy --locked
  --all-targets -- -D warnings` in `apps/desktop/src-tauri`, after T153 and T155.
- `cargo test -p mixengine-cli --test mongodb -- --ignored --nocapture` on this Windows machine with
  `MIXENGINE_MONGODB_PACKAGE` pointing at the unpacked 8.3.11 artifact, before T156 is pushed.
- The `--nounixsocket` argument and the rendered file started by the Linux 8.3.11 artifact in WSL
  before T154 is committed, since this machine cannot run the Unix branch of D3.
- An `all` CI run green on the branch before the pull request is merged.

**Measured on 2026-09-17, while the work landed:**

- WSL (Ubuntu), the Linux 8.3.11 x86_64 artifact, the rendered file under a data path with a space:
  `mongod --config … --nounixsocket` announced `Waiting for connections`, no
  `/tmp/mongodb-<port>.sock` existed, and `SIGTERM` ended in `Now exiting` with the process gone.
- Windows, `tests/mongodb.rs` against the 8.3.11 artifact through a mock registry, a fresh daemon and
  `mix`: installed, created on a free port, started, one document inserted, restarted — a kill on
  that system — and read back, then stopped with nothing answering. 37.8 s.
- `mixengine-daemon`'s `domains::lookup::a_name_nothing_routes_resolves_to_nothing` fails on the
  machine that did this work and nowhere else: a MixEngine installed there routes every `.test` name
  to 127.0.0.1. It is unchanged by this branch.
