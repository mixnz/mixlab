# Parked

*Revisit deliberately, do not start early.* Part of the [build plan](todo.md).

---

- **Buying an Apple Developer ID / Authenticode certificate.** Would remove the Gatekeeper and
  SmartScreen friction and would reopen the persistent-helper option — the two decisions are linked
  ([ADR 0005](../decisions/0005-on-demand-elevation.md)).
- **T198: a release for every Linux.** A per-user `.tar.gz` of the headless programs, and perhaps
  an AppImage for the window, so Arch, NixOS and the rest can install without a package; musl
  is a build of its own. Both shipped until
  [ADR 0053](../decisions/0053-the-helper-has-its-own-version-and-follows-the-product.md) removed
  them, so this starts with a new ADR. Left out of T197 on purpose
  ([design](../specs/2026-10-05-t197-one-command-installs-mixlab-design.md)).
- Optional Docker escape hatch for exotic services (see
  [ADR 0003](../decisions/0003-no-container-isolation.md)).
- Remote tunnels (Cloudflare/ngrok) as an extension.
- Team-shared blueprint registries.
- Editor extensions (VS Code / JetBrains) as additional API clients.
- Xdebug one-click profiles and a built-in profiler view.
