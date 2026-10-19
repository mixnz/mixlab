# 0063. Outbound TLS trusts the operating system, not a bundled root store

**Status**: Accepted
**Date**: 2026-10-20

This records a decision taken when the first outbound request arrived (T20). Until now it lived as
a section of `docs/standards/rust.md`. The decision is unchanged. It moved here because it is an
argument about a dependency, and a standards page should hold rules.

## Context

`hyper` serves the local IPC socket and answers loopback health checks. Neither needs TLS, and
`mixengine-supervisor` refuses an `https://` health check rather than pull a TLS stack in for
`127.0.0.1`. The first request that leaves the machine is a download: the package index and the
artifacts it names, from the publisher or from a `MIXENGINE_MIRROR_URL` mirror.

A TLS client has to get its trusted roots from somewhere. There are three choices: a root store
bundled into the binary, the roots read out of the OS store and handed to `rustls`, or the OS's
own verifier.

## Decision

- **`reqwest` keeps its default `rustls` feature, which enables `rustls-platform-verifier`.** The
  OS's own verifier judges every certificate. Enterprise roots are honoured, CA constraints and
  OCSP/CRL revocation apply, and a machine behind a TLS-inspecting corporate proxy works. The
  workspace manifest says so beside the `reqwest` line, and its default features are not trimmed.
- **No bundled root store.** It would refuse the corporate-proxy machine and the internal
  certificate a mirror is likely to carry, and the user could do nothing about it.
- **`rustls-native-certs` does not verify downloads.** Its own maintainers point at the platform
  verifier for that. It is in the tree for a different job: `mixengine-platform` reads the system
  roots with it to write the bundle that runtimes with their own trust mechanism are handed. That
  use is unaffected by this decision.

## Consequences

- Being permissive here is affordable because **TLS does not decide whether a document is ours**.
  The Ed25519 index signature does, end to end, and it is checked after the bytes arrive, however
  they arrived.
- From phase 5 MixEngine installs its own CA into that same OS store, so its own downloader trusts
  its own CA. This is not a hole. The CA's private key sits on the user's machine, and anybody
  holding it already owns the machine. It also cannot forge the index signature.
