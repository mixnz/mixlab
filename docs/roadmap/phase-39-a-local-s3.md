# Phase 39 — A local S3

*Goal: a project that stores files in S3 has a local S3 to point at, installed as an add-on.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-07-t201-seaweedfs-design.md](../specs/2026-10-07-t201-seaweedfs-design.md).

---

- [x] **T201** SeaweedFS as a `service` extension: a fixture written from a measured 4.48 run on
      all three systems, every port `weed server -s3` opens in `[ports]`, `ready` on S3's
      `/healthz`, `[ui]` on the filer, telemetry off and a 30 s stop. No change to the manifest
      format. The Add-ons row shows the ports an add-on holds, and shows a start or stop in flight
      as the Dashboard does. **(P)**
- [x] **T201a** Publish SeaweedFS from `mixengine-packages`. Published ahead of the T201 merge, at
      `master` `17468ddd`, because the manifest needs no new key: every build that reads Mailpit's
      `[ui]` reads it. Builds released before T200a read it as an entry they cannot read.

**M39** SeaweedFS installed from the window answers an S3 client on all three systems: a bucket
created, an object written and read back. **Met**: through MixEngine with a sandbox home on Windows 11, Ubuntu 24.04
(WSL) and macOS 15.7 arm64, and from the published registry in MixLab on Windows.

Not in this phase, and why: pgweb, phpRedisAdmin, OPcache GUI and mongo-express were considered
beside SeaweedFS and left out (the spec's *Out of scope*). cloudflared becomes a MixLab module, with
a spec of its own.
