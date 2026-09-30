# 0047. The URL scheme is `mixlab://`, and `<old>://` is no longer answered

**Status**: Accepted — supersedes the URL-scheme row and the `<old>://` clause of D10 in
[the merge design](../specs/2026-09-08-the-desktop-client-in-this-repository-design.md), and D5's
*"`SCHEME` stays `<old>`"* in
[T107's design](../specs/2026-09-09-t107-where-the-daemon-and-the-window-are-design.md)
**Date**: 2026-09-22

## Context

When the standalone client became MixEngine's desktop application, the merge design kept its URL scheme: every
The standalone client install had registered `<old>://` with its operating system and links in the wild used it,
so the scheme was treated as a name other software already held. T107 wrote that down as
`window::SCHEME = "<old>"`.

Since then [ADR 0044](0044-mixlab-is-the-product-and-mixengine-is-the-engine.md) made MixLab the
product. `<old>://` is now the one place a user or a web page still meets the old name, and the
links it was kept for are few: the scheme is written by `mix database open` and by nobody else, and
MixEngine never asks the operating system who owns it — it starts the located binary with the URL
as its argument.

## Decision

1. **The scheme is `mixlab`.** `window::SCHEME`, `tauri.conf.json`'s deep-link schemes, the NSIS
   registration under `Software\Classes\mixlab` and `packaging/linux/mixlab.desktop`'s
   `x-scheme-handler/mixlab` all say so, and `tests/packaging.rs` keeps the constant and the
   desktop entry together.
2. **`<old>://` is not accepted as an alias.** The window refuses it like any other scheme; a
   second name would be a second public entry point for a web page to reach, kept for links
   nobody writes any more.
3. **The installer takes back the `<old>` key an earlier release wrote**, on install and on
   uninstall, and only while its command still points at this `$INSTDIR`. A standalone client that
   holds the key keeps it.

## Consequences

- An old `<old>://connect?…` link, and a bookmark or script holding one, stops opening MixLab. On
  Linux and macOS the old registration lingers until the system forgets it, and a click on such a
  link starts a window that shows no tab.
- The handoff contract in [features/extensions.md](../features/extensions.md) changes its scheme
  and nothing else: the query, the password variable and the keyring rules are unchanged.
- The security reasoning written for `<old>://` — a registered scheme is something any web page
  can send — applies to `mixlab://` word for word.

## Alternatives considered

- **Keep `<old>`.** The merge design's choice. It lost because the product's name is MixLab and
  the scheme is user-visible, while the compatibility it bought covers almost nothing.
- **Register both, answer both.** Lost for decision 2's reason: twice the surface for a web page to
  reach, to keep links that are rare.
