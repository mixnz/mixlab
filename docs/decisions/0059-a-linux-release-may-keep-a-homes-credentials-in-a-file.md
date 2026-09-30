# 0059. A Linux release may keep a home's credentials in a file

**Status**: Accepted. It supersedes one sentence of
[0052](0052-a-build-that-is-not-a-release-keeps-its-own-credentials.md) — *"A release refuses
`home`"* — and one of [the T33 design](../specs/2026-08-20-t33-mariadb-design.md#no-credential-store)
— *"It does not fall back to a file"* — and nothing else in either.
**Date**: 2026-09-30

## Context

A release on a Linux machine with no desktop cannot run a database. Its root password goes to the
Secret Service, and an SSH login on a server has none
([ADR 0013](0013-reading-the-d-bus-error-name-to-tell-an-absent-store.md) measured it). T33 accepted
that because every desktop has a store; the headless distribution and a mini PC controlled from
elsewhere are ways MixEngine is now meant to be used, and on them the refusal is the whole database
half of the product.

T184 already built the file store, as one more `Keyring` behind the same host, and ADR 0052 kept it
for development builds only.

## Decision

1. **The store is a property of the home**, recorded in `settings` as `credential_store`, and read
   after the database opens. A flag or `MIXENGINE_CREDENTIAL_STORE` changes it on a release only as
   a checked switch that is then recorded; a development build's flag stays per start.
2. **A release may choose `home` on Linux, and only while the OS store is absent** — `keys`
   answering `UnsupportedPlatform`. Windows and macOS releases still refuse it.
3. **Nothing switches on its own.** A missing Secret Service fails the first run as before, and the
   error's hint names `mix daemon credential-store home`.
4. **A switch is refused while the current store holds anything for this home**, and takes effect
   at the next start.

## Consequences

- A headless Linux home runs databases, and its passwords are protected by file permissions only:
  safe from other accounts, readable from a backup or a removed disk. `mix doctor` says so, and
  says whole-disk encryption is what protects a stolen disk.
- A Linux desktop with a keyring cannot choose the file, so the window beside a daemon never meets
  a home whose passwords it cannot read.
- Moving credentials between stores is not built; a home that must change store after storing
  something recreates what stored it.
- The design is [the headless-home design](../specs/2026-09-30-a-headless-home-keeps-its-credentials-in-a-file-design.md).
