# Phase 38 — An add-on works the moment it is installed

*Goal: installing an add-on from MixLab shows its progress, ends in one row, and a `web-app` opens
without restarting MixEngine.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-07-t200-an-add-on-works-the-moment-it-is-installed-design.md](../specs/2026-10-07-t200-an-add-on-works-the-moment-it-is-installed-design.md);
T200a and T200b: [2026-10-07-t200a-t200b-an-add-ons-page-and-why-a-site-is-down-design.md](../specs/2026-10-07-t200a-t200b-an-add-ons-page-and-why-a-site-is-down-design.md).

---

- [x] **T200** `extension.install` gives a `web-app`'s pool its activator before the job ends, as
      `runtime.install` does since T72a; an activator is held per address and released when its
      service is uninstalled, so a reinstall serves too; `ExtensionSummary` carries its
      description; the Add-ons screen follows the install job on its row, lists an add-on once,
      drives a `web-app` through its site (Open, Turn on/off, state), shows only the action that
      applies, and disables an offer this machine has no artifact for. **(P)**
- [x] **T200a** A manifest may name the port its web interface is on, and a `service` row gets
      *Open* from it — Mailpit's UI. A format change here and a publish in `mixengine-packages`.
- [x] **T200b** The starting page's *"Open MixLab to see why"* has somewhere to point: a wake the
      activator refused is shown beside the site it belongs to.
- [ ] **T200c** Publish Mailpit's `[ui]` from `mixengine-packages`, after the release that carries
      T200a: every earlier build reads an entry with `[ui]` as one it cannot read, so the reader
      ships first.

**M38** Adminer installed from the window opens to its login page on all three systems with no
daemon restart.
