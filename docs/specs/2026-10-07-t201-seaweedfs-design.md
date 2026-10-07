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

**`ready` is the S3 port** (`{listen}:{s3_port}`). `services.port` is the port `ready` watches
(T81, D8), so `mix service list` shows the S3 port, the address an application is given.

**The flags are not final until a real run confirms them.** Their names, whether 4.48 still opens
a gRPC port for S3, and whether `-ip` alone binds and advertises are measured in T201, against all
three platforms, before the fixture is final.

### D2 — No change to the format, and none to the schema (T201)

The manifest uses `[artifact.<target>]`, `[ports]`, `[service]`, `[ui]` and `[permissions]` as
they are. `SCHEMA` stays 1, and every MixEngine that reads `[ui]` reads this entry. Releases from
before T200a count it as *"an entry this build cannot read"*, as they already count Mailpit's.

### D3 — `[ui]` opens the filer's page (T201)

The filer serves a file browser on its HTTP port. `[ui] port = "filer_port"` gives the row
**Open** through what T200a built. The master's status page is for someone debugging the cluster
and is not the page a person opens. T201 confirms that the filer's page exists in 4.48.

### D4 — No authentication on the S3 API (T201)

`weed` serves S3 without credentials unless it is given an identity file. The extension declares
`network = "loopback"`, so only this machine can reach it, the same answer MixEngine gives Redis and
MongoDB, which run with no accounts. An application's S3 client accepts any access key against it.

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
