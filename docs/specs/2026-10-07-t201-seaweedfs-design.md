---
status: draft
date: 2026-10-07
task:
  - T201
  - T201a
---

# T201 — SeaweedFS, a local S3, as an extension

Roadmap tasks T201 and T201a, in a new phase 39. 2026-10-07. They follow phase 38, which ended
with three published extensions: Mailpit (`service`), phpMyAdmin and Adminer (`web-app`, PHP).

## The problem

A project that stores files in S3 in production has nothing local to point at. MixEngine runs its
databases and caches natively, but no object store. SeaweedFS is one native binary, `weed`, that
serves an S3 API (`weed server -s3`), and upstream publishes it for five of the six targets.

`mixnz/mixengine-packages` publishes extensions as manifests, and its generator reads them with
`crates/mixengine-core/src/extensions/manifest.rs`, so the question is whether the format can
already say what SeaweedFS needs. Read at `8081802a`, it can:

- **Artifacts.** Upstream 4.48 (`seaweedfs/seaweedfs`) publishes `darwin_amd64.tar.gz`,
  `darwin_arm64.tar.gz`, `linux_amd64.tar.gz`, `linux_arm64.tar.gz` and `windows_amd64.zip`, about
  40–47 MB each, none for Windows ARM64. Each holds one binary at its top level: `weed`, or
  `weed.exe` on Windows. That is Mailpit's shape: `program = "{install_dir}/weed"` names both,
  because a path with no extension gets `.exe` appended on Windows. The `.tar.gz` entries keep
  their mode (`archive.rs` sets `preserve_permissions`).
- **Data.** Everything it writes goes under `-dir {data_dir}`, which outlives an uninstall
  (T81, D13).
- **Listeners.** `-ip {listen}` binds and advertises the address `permissions.network` allows.

What needs care is the ports, and the window.

### The ports nobody allocated

`weed server` runs four components (master, volume, filer, S3), each on its own HTTP port. By
default each also opens a **gRPC port at its HTTP port + 10000**. MixEngine allocates every port a
manifest names in `[ports]` and nothing else, so a default run listens on four ports nobody
reserved. A second home, or anything else on 19333, makes it fail at start with an error that
names a port the person never saw.

### The window cannot show the S3 address

`ExtensionSummary.ports` carries the ports an installed extension holds, and `mix extension list`
prints them. MixLab's Add-ons screen does not: it shows ports only in the plan dialog, as the ports
the extension *asks for*. For Mailpit that did not matter, because `[ui]` (T200a) opens its page.
For SeaweedFS the one thing a person needs after installing it is the S3 endpoint, and the window
has no way to show it.

## Decisions

### D1 — Every port in `[ports]`, the gRPC ones included (T201)

```toml
[ports]
master_port = 9333
master_grpc_port = 19333
volume_port = 8080
volume_grpc_port = 18080
filer_port = 8888
filer_grpc_port = 18888
s3_port = 8333
s3_grpc_port = 18333
```

Each is passed with its own flag (`-master.port`, `-master.port.grpc`, and so on), so `weed`
derives nothing. The wanted numbers are upstream's defaults. The allocator moves any that is
taken; `volume_port = 8080` will often be.

**Two more listeners are switched off, not allocated.** 4.48 also opens an Iceberg REST catalog on
8181 (`-s3.port.iceberg`) and a Lance namespace server on 9101 (`-s3.port.lance`), both fixed
defaults and both on unless given `0`. Nothing a local S3 is for needs either, so the fixture passes
`0` for both rather than allocating two more ports.

**Measured on 4.48** (`weed server -s3 -ip 127.0.0.1` with every port given): macOS 15.7 on arm64,
Windows 11 on x86-64, Ubuntu 24.04 on x86-64 under WSL. With the two ports above set to `0`, the
process listens on exactly the eight ports `[ports]` names, all on `127.0.0.1`, on all three. `-ip`
alone binds them; `-ip.bind` is not needed. A data directory whose path has a space works.

### D1a — No telemetry (T201)

The master reports anonymous cluster statistics to `telemetry.seaweedfs.com` once 10 GiB are
stored, unless told `-master.telemetry=false`. MixEngine does not send anything from a person's
machine unasked, and nothing in `permissions` describes a call out, so the fixture turns it off.

### D1b — Ready when S3 answers HTTP, with a margin on restart (T201)

`ready` is `GET http://{listen}:{s3_port}/healthz`, expecting 200. `services.port` is the port
`ready` watches (T81, D8), so `mix service status seaweedfs` shows the S3 port, the address an
application is given.

**A TCP check is too early, and no check is exact.** On a restart with existing data, the S3 port
opens before the master has become raft leader, and `/healthz` turns 200 about two seconds before
the volume server has re-registered its volumes. An object read in those two seconds gets
`500 InternalError` ("volume id … not found"); nothing on any port answers differently in that
window. S3 clients retry a 500, so `/healthz` is the honest choice: it is later than the port
opening and as late as anything `weed` reports. The timeout is 60 s, because a restart took 13 to
20 s against 4 s for a first start.

### D1c — A stop is given 30 seconds (T201)

`weed` takes 25 to 26 s to leave after `SIGTERM` (macOS and Linux). Ten of them are the volume
server's `-volume.preStopSeconds`, a pause for a cluster to stop sending it writes, which a single
machine has no use for; the fixture passes `0`, and a stop then takes 16 s, most of it the filer
closing its gRPC streams. `stop` is a signal with a 30 s grace, so the default 10 s does not end
every stop in a kill. A kill is not a loss: after `taskkill /F` on Windows the object was there on
the next start.

**On Windows the grace is never used.** `mix service stop seaweedfs` returns at once: `weed` exits on
the console control event without running its shutdown, the way a kill does. Measured through
MixEngine with a sandbox home: an object written, the service stopped, the extension uninstalled
without `--delete-data` and installed again, and the object read back.

**Left as it is:** `weed` creates its local sockets under `/tmp`, named by port, whatever `-dir`
says, and leaves the two S3 ones behind after a stop. They are named by the port MixEngine
allocated, so two homes do not collide, and a restart replaces them.

### D2 — No change to the format, and none to the schema (T201)

The manifest uses `[artifact.<target>]`, `[ports]`, `[service]`, `[ui]` and `[permissions]` as
they are. `SCHEMA` stays 1, and every MixEngine that reads `[ui]` reads this entry. Releases from
before T200a count it as *"an entry this build cannot read"*, as they already count Mailpit's.

### D3 — `[ui]` opens the filer's page (T201)

The filer serves a file browser on its HTTP port (measured: `200 text/html`, "SeaweedFS Filer").
`[ui] port = "filer_port"` gives the row **Open** through what T200a built. The master's status
page is for someone debugging the cluster and is not the page a person opens.

### D4 — No authentication on the S3 API (T201)

`weed` serves S3 without credentials unless it is given an identity file. The extension declares
`network = "loopback"`, so only this machine can reach it, the same answer MixEngine gives Redis and
MongoDB, which run with no accounts. An application's S3 client accepts any access key against it.

**Its log says otherwise at every start**, and is wrong about what matters: `Failed to load IAM
configuration: no signing key found for STS service`, and from the second start a warning that the
SSE-S3 key is stored on the filer in plaintext. Measured on all three systems: a bucket is created,
an object written and read back, with no credentials. The lines are noise for this use, and a
person reading `mix service logs seaweedfs` should not take them for a failure.

**Rejected: generating an identity file with keys.** The keys would have to be on disk for `weed`
to read them, and "secrets are never on disk" is a rule this format keeps. A person who wants keys
can add `-s3.config` to their own copy of the manifest and install it with `--path`.

### D5 — The Add-ons row shows the ports an extension holds (T201)

Each installed row lists `ExtensionSummary.ports` as `name port`. The data is already in the
response, so no method or member is added. This helps Mailpit's SMTP port as much as SeaweedFS's S3
port.

## Out of scope

The other five extensions considered beside this one, and why each is not here:

- **cloudflared.** A quick tunnel is something a person starts, uses and stops while watching. It
  becomes a MixLab module with its own spec, not an extension the daemon supervises.
- **pgweb.** It needed a database address in `[service]`, the superuser's password passed to a
  service, a rename of a per-platform binary, and a new link table. MixLab's `db` module already
  opens every database MixEngine runs, with its credential.
- **phpRedisAdmin.** It needed database resolution by protocol rather than by account, and a
  pipeline in `mixengine-packages` to rebuild upstream's source with `vendor/`. MixLab's `db`
  module already has a Redis client.
- **OPcache GUI.** Every `web-app` runs on a php-fpm master of its own (T82a, D1), so on Linux and
  macOS it sees only its own cache. Running it in the sites' pool would reverse T82a's isolation for
  one extension, and it would still see a single PHP version.
- **mongo-express.** It needs a `web-app` served by a Node process, which the format does not have,
  and `node_modules` built somewhere. MixLab's `db` module already has a MongoDB client.
- **Keys for S3** (D4).
- **Windows ARM64.** Upstream publishes nothing for it. The plan says so through
  `ExtensionNoArtifact`, which names the targets that exist.

## MixLab

- **Add-ons screen** (`screens/Extensions/`): each installed row lists the ports it holds (D5), and
  SeaweedFS has **Open** through `[ui]` (D3).

The client surface is unchanged. Strings go into `en.ts` and `vi.ts` together. The changelog gets:
- `Added`: SeaweedFS in Add-ons, a local S3;
- `Changed`: Add-ons shows the ports an add-on holds.

## Acceptance

- **T201.**
  - `crates/mixengine-testkit/fixtures/extensions/seaweedfs.toml` exists, with all five targets.
    It parses, its plan lists eight ports, and its spec renders every port flag and `-dir` under
    `{data_dir}`.
  - Measured on Linux, macOS and Windows with the 4.48 binary, and recorded in the fixture's
    comments: the flags, every port `weed server -s3` listens on (nothing outside `[ports]`), and
    the filer page.
  - Installed with `--path` on each platform, it starts, `ready` passes, and an S3 client creates a
    bucket and writes and reads an object on `s3_port`. The data survives an uninstall without
    `--delete-data`.
  - The Add-ons row shows its ports, and Open opens the filer page.
- **T201a.** The manifest is published from `mixengine-packages` at the `master` commit that
  carries T201 (that repository's `publish-extensions` workflow). SeaweedFS installs from the
  registry and passes the same S3 check.
