# Architecture decision records

One file per decision, numbered, immutable once accepted. To change a decision, write a **new** ADR
that supersedes the old one and update the old one's status line — never edit its body.

## Index

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-rust-core-daemon-gui-split.md) | Rust core + daemon, thin CLI and GUI clients | Accepted |
| [0002](0002-cross-platform-from-day-one.md) | Cross-platform from day one via a platform trait layer | Accepted |
| [0003](0003-no-container-isolation.md) | Native processes, no Docker/VM isolation | Accepted |
| [0004](0004-caddy-as-default-web-server.md) | Caddy as the default web server, Nginx optional | Accepted |
| [0005](0005-on-demand-elevation.md) | On-demand elevation, no persistent privileged helper | Accepted |
| [0006](0006-servicespec-in-proto-and-secret-free.md) | `ServiceSpec` lives in `mixengine-proto` and never carries a secret | Accepted |
| [0007](0007-supervised-child-owns-a-process-group.md) | A supervised child owns a process group, and "no orphans" means three different things | Accepted |
| [0008](0008-no-signal-stop-on-windows.md) | A service is asked to stop with a signal on Unix and with a command on Windows | Accepted |
| [0009](0009-logs-travel-on-their-own-stream.md) | Log lines travel on their own stream, never on the event stream | Accepted |
| [0010](0010-supervised-child-never-inherits-administrators.md) | A child started to run a user's software never inherits Administrators | Accepted |
| [0011](0011-no-gui-in-this-repository.md) | MixEngine ships a CLI; a GUI is a client in another repository | Superseded by 0027 |
| [0012](0012-a-boot-time-job-enables-the-packet-filter-on-macos.md) | A boot-time job enables the packet filter on macOS | Accepted |
| [0013](0013-reading-the-d-bus-error-name-to-tell-an-absent-store.md) | The D-Bus error name is what tells an absent credential store from a refusing one | Accepted |
| [0014](0014-an-extension-is-not-an-api-client.md) | An extension is not an API client, and gets no token | Accepted |
| [0015](0015-the-helper-installs-itself.md) | The privileged helper installs itself, and the installer does not | Accepted |
| [0016](0016-autostart-is-registered-by-mixengine.md) | MixEngine registers the daemon's autostart entry, and the installer does not | Accepted |
| [0017](0017-smart-app-control-is-an-unsupported-configuration.md) | A machine with Smart App Control enforcing is a configuration MixEngine does not support | Accepted |
| [0018](0018-a-signed-candidate-is-what-lets-a-path-cross-the-boundary.md) | A path may cross into the elevated process only when that process itself checks a signature over the bytes at it | Accepted |
| [0019](0019-an-added-response-member-is-optional.md) | A member added to a response is optional on the wire, and the protocol does not bump for it | Accepted |
| [0020](0020-the-published-contract-is-the-shape-the-daemon-writes.md) | The published contract is the shape the daemon writes, not everything it accepts | Accepted |
| [0021](0021-the-handbook-is-one-corpus-published-three-ways.md) | The handbook is one Markdown corpus published three ways, and help is not an API method | Accepted |
| [0022](0022-a-crash-report-is-recorded-by-default-and-sent-by-nothing.md) | A crash report is recorded by default and sent by nothing | Accepted |
| [0023](0023-an-arm64-windows-machine-runs-the-x86_64-build.md) | An ARM64 Windows machine installs the x86_64 build, and is told that it did | Accepted |
| [0024](0024-a-build-that-is-not-a-release-keeps-its-own-home.md) | A build that is not a release keeps its own home | Accepted |
| [0025](0025-a-credential-is-answered-only-by-a-method-that-exists-to-answer-it.md) | A credential is answered only by a method that exists to answer it | Accepted |
| [0026](0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md) | The active front end is a row, and switching it is a job | Accepted |
| [0027](0027-the-desktop-client-lives-in-this-repository.md) | The desktop client lives in this repository, behind the same API | Accepted; naming superseded by 0044, rule 3 by 0056 |
| [0028](0028-the-appimage-does-not-carry-webkitgtk.md) | The AppImage does not carry WebKitGTK, and the window's floor is the distribution's | Superseded by 0053 |
| [0029](0029-every-install-format-carries-a-helper-to-install-from.md) | Every install format carries a helper to install from | Accepted |
| [0030](0030-the-project-token-expands-to-a-slug.md) | A blueprint's `{project}` expands to a slug, not to the project's name | Accepted |
| [0038](0038-the-window-is-the-only-desktop-database-client.md) | The window is the only desktop database client, and `desktop-app` is not an extension kind | Accepted |
| [0040](0040-a-development-builds-home-follows-its-checkout.md) | A development build's home follows its checkout, unless root cannot read it there | Accepted |
| [0039](0039-a-jdk-is-told-about-the-authority-inside-its-own-cacerts.md) | A JDK is told about the authority inside its own `cacerts` | Accepted |
| [0041](0041-mixengine-stops-nothing-a-person-did-not-ask-it-to.md) | MixEngine stops nothing a person did not ask it to stop | Accepted |
| [0042](0042-mixlab-starts-at-login-when-a-person-asks-it-to.md) | MixLab starts at login when a person asks it to, and separately from the daemon | Accepted |
| [0043](0043-documentation-lives-under-docs.md) | Documentation for people lives under `docs/`; `.claude/` holds only agent configuration | Accepted |
| [0044](0044-mixlab-is-the-product-and-mixengine-is-the-engine.md) | MixLab is the product; MixEngine is the engine inside it and the headless distribution | Accepted; decision 6 superseded by 0046, decision 5 by 0049 |
| [0045](0045-mixlab-has-an-account-and-mixengine-does-not.md) | MixLab has an end-to-end encrypted account; MixEngine has none, and its server is its own repository | Accepted; decision 4 superseded by 0046 |
| [0046](0046-the-sync-server-lives-beside-the-client-it-serves.md) | The sync server lives in this repository, under `server/`, so one CI run proves both halves agree | Accepted |
| [0047](0047-the-url-scheme-is-mixlab.md) | The URL scheme is `mixlab://`; `mixdb://` is no longer answered | Accepted |
| [0048](0048-a-file-a-package-manager-placed-leaves-with-the-package.md) | A file a package manager placed leaves with the package; `mix uninstall` keeps a packaged helper on Linux | Accepted |
| [0049](0049-a-download-is-named-after-what-it-installs.md) | A download is named after what it installs: `mixlab-…` with the window, `mixengine-…-headless` without | Accepted; its list of downloads amended by 0053 |
| [0050](0050-a-copy-the-pkg-installed-is-updated-by-the-pkg.md) | A copy the `.pkg` installed is updated by the `.pkg`, through Installer.app | Accepted; extended to the Linux packages by 0053 |
| [0051](0051-an-uninstall-ends-what-it-undoes.md) | An uninstall ends what it undoes, and is the uninstaller's rather than the window's | Accepted |
| [0052](0052-a-build-that-is-not-a-release-keeps-its-own-credentials.md) | A build that is not a release keeps its credentials in its home, not in the OS store | Accepted |
| [0053](0053-the-helper-has-its-own-version-and-follows-the-product.md) | The helper has its own version and follows the product through the daemon | Accepted |
| [0054](0054-an-install-completes-itself-from-its-own-payload.md) | An install completes itself from its own payload; an update still adds nothing | Accepted |
| [0055](0055-the-daemons-credentials-are-one-keychain-item-per-home-on-macos.md) | The daemon's credentials are one Keychain item per home on macOS | Accepted |
| [0056](0056-mixlab-stands-without-mixengine.md) | MixLab stands without MixEngine, and updates itself; MixEngine never updates unasked | Accepted |
| [0057](0057-a-runtime-s-commands-are-in-bin-only-while-it-is-installed.md) | A runtime's commands are in `bin/` only while it is installed | Accepted |
| [0058](0058-the-tray-is-mixlabs-and-a-module-lends-it-a-section.md) | The tray is MixLab's, and a module lends it a section | Accepted |

### Desktop (recorded in MixDB)

Four decisions the desktop application took before it came to this repository
([ADR 0027](0027-the-desktop-client-lives-in-this-repository.md)). They keep their dated names:
renumbering them would make them look like decisions this repository took.

| Date | Decision |
| --- | --- |
| [2026-08-10](desktop/2026-08-10-codemirror-for-the-query-editor.md) | CodeMirror 6 for the Query tab's editor |
| [2026-08-11](desktop/2026-08-11-two-checkers-and-when-they-keep-quiet.md) | Two checkers for the Query tab, and when each keeps quiet |
| [2026-08-31](desktop/2026-08-31-gpl-and-signpath-for-free-code-signing.md) | GPL-3.0, so that code signing can be free |
| [2026-09-17](desktop/2026-09-17-mixlab-redesign-tokens-themes-densities.md) | MixLab's redesign: one token set, two themes, two densities, no glass |

## Template

```markdown
# NNNN. <Short title>

**Status**: Proposed | Accepted | Superseded by [NNNN](…) | Deprecated
**Date**: YYYY-MM-DD

## Context
What forces are at play? What did we know at the time?

## Decision
What we are doing, stated plainly.

## Consequences
What becomes easy, what becomes hard, what we accept as the cost.

## Alternatives considered
Each with the reason it lost.
```

Write an ADR when a choice is expensive to reverse, spans more than one crate, or will otherwise be
re-litigated in six months by someone (possibly you) who has forgotten the reasoning.
