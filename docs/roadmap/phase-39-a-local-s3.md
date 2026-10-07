# Phase 39 — A local S3

*Goal: a project that stores files in S3 has a local S3 to point at, installed as an add-on.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-07-t201-seaweedfs-design.md](../specs/2026-10-07-t201-seaweedfs-design.md).

---

- [ ] **T201** SeaweedFS as a `service` extension: a fixture written from a measured 4.48 run on
      all three systems, every port `weed server -s3` opens in `[ports]`, `ready` on the S3 port,
      `[ui]` on the filer. No change to the manifest format. The Add-ons row shows the ports an
      add-on holds. **(P)**
- [ ] **T201a** Publish SeaweedFS from `mixengine-packages` at the `master` commit that carries
      T201. Builds released before T200a read the entry as one they cannot read, as they already
      read Mailpit's.

**M39** SeaweedFS installed from the window answers an S3 client on all three systems: a bucket
created, an object written and read back.

Not in this phase, and why: pgweb, phpRedisAdmin, OPcache GUI and mongo-express were considered
beside SeaweedFS and left out (the spec's *Out of scope*). cloudflared becomes a MixLab module, with
a spec of its own.
