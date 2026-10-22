# Phase 34 — A headless host

*Goal: a release on a Linux machine with no desktop runs databases, keeps their passwords across
restarts, and comes back on its own after a reboot — the host that MixLab on another machine, and
later a phone, will control.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-30-a-headless-home-keeps-its-credentials-in-a-file-design.md](../specs/2026-09-30-a-headless-home-keeps-its-credentials-in-a-file-design.md)
(T194).

---

- [x] **T194a** The ADR: a Linux release may keep a home's credentials in a file, superseding ADR
      0052's "a release refuses `home`" and T33's "it does not fall back to a file".
- [x] **T194b** `settings.credential_store`, its precedence under the flag and over the build's
      default, and `credentials::choose` accepting `home` on a Linux release only. **(P)**
- [x] **T194c** The absent-store hint names `mix daemon credential-store home` ahead of the D-Bus
      workarounds; nothing switches on its own.
- [x] **T194d** `daemon.set_credential_store`, refused while the current store holds anything for
      this home, effective at the next start; `mix daemon credential-store`, and MixLab's
      *Credentials* section in Settings → MixEngine.
- [x] **T194e** `credentials` on `daemon.status` and in `mix status`, the doctor note, the startup
      log, the `mix database` wording; bindings.
- [x] **T194f** `security-model.md`, `platform-abstraction.md`'s `Keyring` row, the changelog lines,
      and the design flipped to `implemented`.
- [x] **T211** The ADR: on a machine with no graphical agent, `mix` in a terminal raises the
      prompt itself, through `sudo` on the helper — amending ADR 0005. Found on an Ubuntu 24.04
      server over SSH (2026-10-21): `polkitd` is installed and running, but `pkexec` is not — a
      separate package since Ubuntu 24.04 — and with no polkit agent over SSH it could not prompt
      anyway, so `mix elevation grant` and the privileged half of `mix uninstall` never run there,
      and nothing names a way to do it by hand. The error itself is wrong too: it says *"pkexec is
      not on PATH, so polkit is not installed"*, which T211c corrects. Linux and macOS alike: a Mac
      over SSH has the same gap. ADR 0065;
      design: [2026-10-21-t211-elevation-from-a-terminal-design.md](../specs/2026-10-21-t211-elevation-from-a-terminal-design.md).
- [x] **T211a** The daemon writes the request and hands its path to a client that has a terminal,
      instead of refusing in `elevation/require.rs` before the request exists;
      `mixengine-elevate` still validates every request itself. **(P)**
- [x] **T211b** `mix elevation grant` and `mix uninstall` run `sudo <helper> <request>` in the
      person's own terminal, and the daemon reads the report as it does after `pkexec`. **(P)**
- [x] **T211c** Ask whether this machine can raise a prompt before `continue? [y/N]`, not after
      the answer; where it cannot, the error names the command a person can run by hand.
- [x] **T211d** `docs/guide/en/uninstalling.md` names `mixengine-headless` beside `mixlab` for
      `apt remove` and `dnf remove`, `mix uninstall`'s last line names the package command, and
      `--keep-home`'s help stops saying the daemon keeps running (ADR 0051).
      **Verified** over SSH on 2026-10-21, on Ubuntu 24.04 and on macOS 15: a decline, a grant run
      by hand from a second session, a grant in the same terminal, `mix doctor --repair --yes` and
      `mix uninstall`; on the Mac also Ctrl-C at `sudo`'s password and three wrong passwords. Two
      gaps the Mac found are T211e and T211f, measured there again the same day and passing; the
      helper's console check (T211e, D5) has unit tests and was not tried on a Mac.
- [x] **T211e** On macOS a terminal cannot add MixEngine's authority to the System keychain:
      `add-trusted-cert -d` raises SecurityAgent on the Mac's own screen even under `sudo`. A grant
      raised from a terminal leaves such rows queued, says so before `continue?`, and
      `elevation.status` marks them; the helper refuses at once when the console is not the
      caller's. **(P)**
      Design: [2026-10-21-t211e-what-a-terminal-cannot-grant-design.md](../specs/2026-10-21-t211e-what-a-terminal-cannot-grant-design.md).
- [x] **T211f** MixLab's elevation dialog shows a terminal's waiting command whatever `can_prompt`
      says, and while it follows such a grant says it waits for a terminal rather than for a system
      prompt. Same design as T211e.
- [x] **T211i** Found re-measuring T211e/f on the Mac: a daemon `mix` starts over SSH lives in
      SSH's session, and osascript from there fails with -60007 (*wrong password*) showing nothing,
      for every later dialog grant at the screen. The daemon reads `launchctl managername` once and
      answers `can_prompt: false` outside `Aqua`, with the way out (`mix daemon stop` at the Mac),
      and -60007 reads as that. `ElevationGrant`/`UninstallQuery` carry `over_ssh`, and only a grant
      over SSH leaves what needs the screen: `sudo` at the screen reaches its dialog. `mix` refuses
      over SSH before `continue?` when every row needs the screen; rows read *not over SSH*;
      MixLab's lead and the wrapped command fit a terminal's grant. **(P)** Left open: starting the
      daemon from SSH into `gui/<uid>` through launchd, which needs measuring on a Mac first.
      **Verified** on macOS 15 on 2026-10-21, over SSH and at the screen, MixLab included. Two
      sentences that measurement found pointed the wrong way, and no longer do: the refusal over SSH
      offers MixLab only when the daemon can raise a dialog, and the dialog's *cannot prompt* line
      (`mix daemon stop`, which would cancel it) stays hidden while a terminal's grant waits.
- [x] **T211g** One home takes from the machine what other homes on it still use, found on a Mac
      with two homes on 2026-10-21 — and every machine that develops MixEngine has two (ADR 0024).
      The hosts file has one `# BEGIN MixEngine` block for every home, so one home's `hosts-apply`
      replaced another's `blog.test`, and a home whose resolver got wired empties it for the others.
      `ResolverRevoke` removes every `/etc/resolver/*` file MixEngine marked, whatever port it
      names. `mix uninstall` removes the helper and the audit log another home still uses. One block
      per home, tagged by its path; a revoke that names the port; a ledger of homes the helper keeps,
      so it keeps itself and the log while another home is listed. **(P)** ADR 0066, amending 0051
      and 0005.
      Design: [2026-10-21-t211g-t211h-one-home-undoes-only-its-own-design.md](../specs/2026-10-21-t211g-t211h-one-home-undoes-only-its-own-design.md).
      **Verified** on macOS 15 on 2026-10-22, at the screen, with three homes of this branch over
      the helper it builds: each wrote a block of its own and the untagged block drained; the second
      home, on its own DNS port, left the first one's block, a resolver file naming another port,
      the helper and the log in place on uninstall, and its plan said so before `continue?`; a third
      home deleted by hand was named by `mix doctor` and emptied by `--repair`, after which the
      ledger dropped the second; the last home's uninstall took the helper, the log and the ledger.
      One sentence that run found reads otherwise now: emptying a gone home's block said
      *"remove MixEngine's block from the hosts file"*, like emptying one's own.
- [x] **T211h** Display slips older than T211, found on the same Mac: `helper … replaces it at the
      next permission prompt` printed when the daemon skipped the replacement; a succeeded job still
      showing 40% or 70% and its last step; `mix doctor --repair` printing "the managed hosts block"
      twice; MixLab's `hosts-apply` detail printing `127.0.0.1` twice with no name. And two in the
      elevation dialog, found re-measuring T211i: *"Everything below will be changed in one
      prompt"* where the daemon cannot raise one, and *"Anything marked below waits for this
      screen"* where nothing is marked.
- [x] **T211j** A resolver that glibc never asks is read as wired, found running M34 on 2026-10-22.
      On an Ubuntu 24.04 server with AdGuard Home, `resolved.conf.d/adguardhome.conf` sets
      `DNSStubListener=no` and `/etc/resolv.conf` names `127.0.0.1`, `1.1.1.1` and `8.8.8.8`, so
      after `resolver-apply` the `mixengine0` link answers `resolvectl query nothere.test` and
      `getent hosts anything.m34.test`, `foo.localhost` and `bar.internal` answer nothing. `mix
      status` still says *"wildcards for *.test, *.localhost, *.internal"*, `mix domain status`
      says `wildcard 127.0.0.1`, `mix doctor` says nothing about it, and the daemon queues
      *"remove MixEngine's block from the hosts file"* — granted, it would take `m34.test` off this
      machine. Wired is to mean that the system's own lookup reaches MixEngine's server: read
      whether `/etc/resolv.conf` is resolved's stub before calling the link wired, keep the hosts
      block while it is not, and have `mix doctor` name the file and the setting in the way.
      Design: [2026-10-23-t211j-a-resolver-programs-never-ask-design.md](../specs/2026-10-23-t211j-a-resolver-programs-never-ask-design.md).
      **Verified** on the same machine on 2026-10-23 with this branch's package: a fresh home asked
      for no `resolver-apply`, `mix status` gave the reason, the block kept `m34.test` and `mix
      doctor` named both files and `libnss-resolve`; with `libnss-resolve` installed the grant wired
      the link and `getent` answered `anything.m34.test`, `foo.localhost` and `bar.internal` while
      AdGuard stayed upstream; with it removed again, the next start withdrew the queued *"remove
      MixEngine's block"*; and `mix uninstall` took a link programs never reach. The measurement
      found two gaps the design then closed: a queued hosts row outlived a restart that changed the
      reading, and the uninstall planned a revoke only from `wired`. It also found T211m.
- [x] **T211k** `resolver-revoke` on Linux removes `10-mixengine.netdev` and `10-mixengine.network`
      and reloads systemd-networkd, which leaves the `mixengine0` dummy link up until the next boot;
      found by the same run, where `mix uninstall` printed *"removed … the DNS routing"* and `ip
      link` still listed `mixengine0`, unmanaged, with no address and no DNS. Delete the link
      (`networkctl delete mixengine0`) after removing its files, and say so in the detail.
      **Verified** on the same machine on 2026-10-22 with the helper rebuilt: a grant brought
      `mixengine0` up, and after `mix uninstall` `ip link` no longer had it. A link left with no
      files by an earlier helper is deleted too.
- [x] **T211l** Display slips the same run found: `mix doctor` reads a hosts block that holds names
      this home no longer wants as *"the hosts file does not hold the names this home's sites
      need"* (`doctor.rs`, one sentence for a block that differs either way); `mix autostart
      status` heads an at-boot entry with *"starts when you log in"*; `mix database client` says
      *"the headless archive has none"*, a tarball T182b replaced with `mixengine-headless`; and
      `mix uninstall` on a `mixengine-headless` install keeps *"the MixLab application"*, says it
      forgets passwords from *"this user's credential store"* when the store is the home's file,
      and names `apt remove mixlab` first after its own plan named `dpkg:mixengine-headless`.
      Done: the doctor names the entries missing and the ones left over; an entry that starts at
      boot is headed so; a headless install is called one; the package row names the window only
      where `locate_window` finds it, as `database.client` does, since a Mac's two packages share a
      receipt; the password rows say *"forgot 1"*, true of either store; and the last line names
      the package the plan's row named, both only where no row did.
- [ ] **T211m** An uninstall that has something to revoke on a machine whose log an earlier
      uninstall removed leaves the log it just made, and so keeps the home. Found verifying T211j
      on 2026-10-23: a fresh home on a machine still holding `mixengine0` granted `resolver-revoke`
      and `audit-log-remove`, and the helper answered the second *"this machine has no record yet of
      which other homes use it"* (`homes.rs`, ADR 0066), because the ledger did not exist before the
      batch. It did not: this batch made it, and the log with it, so no other home can be using
      either. The run printed *"LEFT … this was not here before the uninstall and it is here now"*
      and kept the home; a second `mix uninstall` finished. Keep the log only when the ledger or
      the log was there before the batch.
- [x] **T195** Start at boot without a login: a systemd user unit and linger, turned on as the
      account and never through `mixengine-elevate` — polkit lets an account linger itself, and a
      headless host has no prompt to show.
      Design: [2026-10-20-t195-start-at-boot-without-a-login-design.md](../specs/2026-10-20-t195-start-at-boot-without-a-login-design.md).

**Milestone M34**: on an Ubuntu 24.04 server with no desktop, reached only over SSH, a release
chooses the file store, first-runs a MariaDB, keeps its password across a daemon restart, and after
a reboot with nobody logged in the daemon is back with the database answering; over the same SSH
session, `mix elevation grant` applies its queue and `mix uninstall` finishes.

**Reached on 2026-10-22**, on `code-server-n100`: Ubuntu 24.04.5, x86_64, systemd 255, no display
manager and no desktop package, reached over SSH, with no `pkexec` and no Secret Service on the
session bus. The release was this branch at `c388a161`, built on that machine by
`packaging/linux/build-deb.sh` with `[profile.release]` as written and `MIXENGINE_RELEASE`, and
installed as `mixengine-headless_0.0.19-1_amd64.deb` (helper 0.1.8) over a 0.0.19 install whose
home was deleted first. Two ways it is not a tag's artifact: the four binaries were built on the
host's glibc 2.39 rather than in `manylinux_2_28`, and `MIX_WINDOW_GLIBC` was raised to 2.39 for
that one build and put back, since the window built on 24.04 needs 2.39 and the headless package
does not carry it.

1. `mix daemon credential-store home` answered *"from the next start"*; after `mix daemon stop`,
   `mix status` read *"a file in this home, readable by your account only"*, and the log named
   `credentials.json`.
2. `mix package install mariadb 11.4.13` and `mix service create mariadb@main` first-ran it on 3309,
   saying 3306 was taken (a MySQL in Docker holds it). The root password sat in `credentials.json`,
   mode 0600; with it the client wrote a row, and with a wrong one it was refused.
3. `mix daemon stop` stopped the service with the daemon; the next daemon read the file store, the
   password hashed the same, and the row read back.
4. With `mix service autostart mariadb@main --on` and `mix autostart enable --at-boot` (linger
   `yes`, unit enabled), the machine rebooted at 03:51. systemd started the unit at 03:51:25 and
   MariaDB was up a second later; nobody logged in until 23:17, and then the same password read
   the same row.
5. A site at `m34.test` queued `hosts-apply` beside `resolver-apply` and `trust-ca-install`.
   `mix elevation grant` in that SSH session said before `continue?` that `sudo` would ask in this
   terminal, ran `sudo -- …/mixengine-elevate …/request.json`, and applied all three: `/etc/hosts`
   gained `# BEGIN MixEngine 186baeea2e723b7c`, and `/var/log/mixengine/` held `elevate.log` and a
   `homes.json` listing this one home. `sudo` there is `NOPASSWD`, so its password prompt was not
   exercised, and the terminal was a pty under `script` driven from the same SSH login.
6. `mix uninstall` removed the hosts block (the file read as before it), the networkd files, the
   authority, `/var/log/mixengine/` with the log and the ledger, the unit, the linger and the home;
   it kept the helper and named the package that removes it, as T88d and T211d have it.

What the run found is T211j, T211k and T211l, which keep the phase open: wildcard names never
reached glibc on that machine while every surface said they did, the `mixengine0` link outlived
`mix uninstall` until the next boot, and six sentences said something other than what happened.
