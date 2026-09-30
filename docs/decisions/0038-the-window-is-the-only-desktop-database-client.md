# 0038. The window is the only desktop database client, and `desktop-app` is not an extension kind

**Status**: Accepted
**Date**: 2026-09-17

## Context

T80 gave extensions four kinds, and `desktop-app` — *an application MixEngine finds and hands a
connection to* — had exactly one entry ever published: the standalone client (T84). ADR 0027 then brought the standalone client into
this repository as MixLab, the window every installer ships, and T107 made `mix database open` reach
that window without any extension, keeping the kind alive behind one rule: the window answers unless
an installed `desktop-app` names some other scheme. On 2026-09-17 the standalone client's entry was withdrawn from
the registry. The kind was left with no entry, a precedence rule written for one product, per-OS
lookups (App Paths and the uninstall table, Spotlight, XDG desktop entries) nothing needed, and a
`no_client` hint telling people to install an entry that no longer exists.

## Decision

**A database is opened in this install's MixLab window or nowhere.** `desktop-app` is removed from
the manifest format, the API, the daemon, the platform layer, `mix` and MixLab. `DesktopClient` is
`installed { name, program }` or `no_client`. Migration 24 deletes installed rows of the kind.

**`PROTOCOL_VERSION` is not bumped**, although [ADR 0019](0019-an-added-response-member-is-optional.md)
bumps it for a removed member. Everything removed is written only by a daemon, and a daemon from this
release writes none of it, so an older client reading a newer daemon sees shapes it already handles:
`extension` and `ExtensionPlan.client` were optional and skipped when absent, `opens` was `null` for
every other kind, and `not_installed` and `desktop-app` simply never arrive. The direction that can
fail — a newer client reading an older daemon that still holds such a row, or a registry cache listing
The standalone client — lasts from a binary swap to the daemon's restart, which `updates::apply` performs in the same
step. A bump would turn that moment into one where `mix daemon stop` is refused by the handshake,
which is ADR 0019's own reason not to bump.

## Consequences

`mix database open` on the headless archive says the install has no window, rather than naming
something to install. A hand-written `desktop-app` manifest is refused by the reader like any unknown
kind. The two empty directories a `desktop-app` install created stay on disk. The `<old>://` scheme,
the handoff contract and the keyring convention are unchanged: they are the window's.

## Alternatives considered

**Keep `desktop-app` as a general mechanism, the window always first.** Nothing would use it, and
three per-OS lookups would stay tested and maintained for an entry nobody publishes; a future
external client is a new decision with a real product behind it, not a reason to keep this one.

**Keep the fixture and the rule, fix only the hint.** It leaves the product describing the standalone client as
something separate from the window it became.
