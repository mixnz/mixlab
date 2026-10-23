---
status: implemented
date: 2026-10-23
task: T211j
---

# T211j — A resolver that programs never ask is not wired (design)

Found running M34 on 2026-10-22, on `code-server-n100`: Ubuntu 24.04.5, systemd 255, AdGuard Home
in front of the machine's DNS. Linux only; [Not here](#not-here) says why macOS and Windows are left
alone.

## Measured, not assumed

After `mix elevation grant` applied `resolver-apply`, the link was up and `systemd-resolved` routed
the managed TLDs to MixEngine's server, and still no ordinary program on the machine reached it:

| Question | Answer |
| --- | --- |
| `resolvectl query nothere.test` | `127.0.0.1` |
| `getent hosts anything.m34.test`, `foo.localhost`, `bar.internal` | nothing |
| `getent hosts m34.test` | `127.0.0.1`, from the hosts block |
| `/etc/systemd/resolved.conf.d/adguardhome.conf` | `DNS=127.0.0.1`, `DNSStubListener=no` |
| `/etc/resolv.conf` | a link to `/run/systemd/resolve/stub-resolv.conf`, which resolved itself links to `resolv.conf` when the stub is off: `nameserver 127.0.0.1`, `1.1.1.1`, `8.8.8.8` |
| `dig @127.0.0.53` | connection refused |
| `hosts:` in `/etc/nsswitch.conf` | `files dns`, so glibc reads `resolv.conf` and asks AdGuard directly |
| `libnss-resolve` | not installed; candidate 255.4-1ubuntu8.18 |

Every surface said the opposite. `mix status` read *"DNS on 127.0.0.1:53535, wildcards for *.test,
*.localhost, *.internal"*, `mix domain status m34.test` read `wildcard 127.0.0.1`, `mix doctor` had
nothing to say about the resolver, and the daemon queued *"hosts-apply: remove MixEngine's block from
the hosts file"*. Granted, that row would have taken `m34.test` off the machine.

**The cause is one reading.** `ResolverConfig::probe` on Linux answers from
`10-mixengine.network` alone (`linux/resolver.rs`'s `wired_in`), and that answer is
`Dns::wired()`: the daemon's mode, `daemon.status`' `wildcards`, `domain.status`' `wildcard`
column, `hosts::desired` (a wired TLD's names leave the hosts block) and the doctor's resolver
check all read it. The trait's own documentation already says the probe answers "is the
configuration in place" and not "does a name resolve". On this machine the configuration being in
place was not enough, because nothing on the machine ever consulted the configuration.

**Not rare on the machines M34 is about.** AdGuard Home, Pi-hole and a dnsmasq on port 53 each turn
the stub listener off to take that port, and each is a common thing to run on a headless server.

## D1 — wired means the files route a TLD and programs ask systemd-resolved

A program's lookup reaches `systemd-resolved`, and so its routing domains, in one of two ways:

1. **`resolve` on the `hosts:` line of `/etc/nsswitch.conf`, before `dns`**: glibc asks resolved
   over its socket through `nss-resolve`, whatever `resolv.conf` says. `libnss-resolve` puts it
   there.
2. **The first `nameserver` in `/etc/resolv.conf` is `127.0.0.53`**, resolved's stub. glibc asks
   the first server first, and a later one only after the first fails.

When neither holds, programs bypass resolved, and the link routes nothing anybody sends. The probe
then reports **no TLD wired**, whatever the `.network` file declares.

Both files are world-readable, so the probe stays unprivileged and cheap enough to run at every
start and after every grant, as T45's D8 requires.

`127.0.0.54`, resolved's proxy stub, is not counted. Whether it applies per-link routing has not
been measured, and a reading that guesses wrong here is the defect this task is about.

**Rejected: a real lookup of a fresh name as the probe.** It answers the right question, and T46's
`domain.status` already asks it for a declared name. As the probe it has three costs. The trait
documents a window after the reload in which the link is not up yet, so a probe right after a grant
would read *not wired* on a machine where it is. It cannot say *why*, which is the sentence D4 owes a
person. And it would put a network wait on the daemon's start path.

**Rejected: `ResolverMethod::None` for a bypassed machine.** `None` has no `target`, so `mix
uninstall` could no longer revoke a link an earlier grant wrote, which is exactly this machine's
state today.

## D2 — `ResolverState` says it was bypassed, and asks for nothing

`ResolverState` gains `bypassed: Option<String>`, the reason in words. Linux fills it as in D1;
macOS, Windows and the mock always leave it `None`.

When it is `Some`:

- `wired` is empty, and `missing` holds the same reason;
- `plan` returns `None`. A grant that writes files already on disk cannot change what programs ask,
  so asking for one would spend a prompt to change nothing, forever;
- `target` is unchanged, so `mix uninstall` still removes the link and its files.

`ResolverState` also gains `declared`, the TLDs the configuration routes whether or not programs
reach it: `wired` everywhere except a bypassed Linux, where it is what `10-mixengine.network`
declares. D6 found the need by measuring. The uninstall inventory planned a revoke only when `wired`
was non-empty, so the first measured uninstall left the link and both files on disk. It reads
`declared` now.

## D3 — the daemon needs no new branch, only the reason

With `wired` empty, everything that reads it already does the right thing:

- `Dns::mode` is `HostsOnly`, so `hosts::desired` keeps every site name in the block.
- `require_resolver` gets no plan and calls `no_longer_needed("resolver")`, which drops a queued
  `resolver-apply`.
- `daemon.status` reports no wildcards, and `domain.status` reports none per name.

What changes in the daemon is the reason. `Dns` keeps the probe's `bypassed` beside `wired`, and
`Dns::because` returns it in place of *"nothing on this machine routes a managed TLD to … yet"*.
That *"yet"* would promise a grant that cannot help.

**And a start recomputes a queued hosts row.** D6 found this one by measuring. A row is the block
as it should have been when the row was queued. `require_hosts` runs after a site changes, after a
grant and under `--repair`, but not at start. So a *"remove MixEngine's block"* row queued while
programs reached the resolver was still waiting after a restart on which they no longer did, and
granting it would have taken the home's names off the machine. A start now runs `require_hosts`
after `require_resolver`, **only when a hosts row of this home is queued**, which withdraws the row
or replaces it with what the block should hold now. A start with no such row still asks for
nothing: putting back a block somebody changed stays `--repair`'s job.

## D4 — `mix doctor` names the file and the way out, as a note

The resolver check reports a bypassed machine as a `Note`, not a `Problem`. Running on the hosts
file is a supported mode (T46a), and the setting in the way is the person's own, made on purpose
for another program. The wording, its last sentence kept because D6 showed it true:

> names under .test, .localhost and .internal come from the hosts file, because programs on this
> machine do not ask systemd-resolved: /etc/resolv.conf names 127.0.0.1 rather than 127.0.0.53,
> and the hosts line of /etc/nsswitch.conf has no `resolve` before `dns`. Installing
> libnss-resolve (`sudo apt install libnss-resolve` on Debian and Ubuntu) sends them through
> systemd-resolved and keeps the DNS server you use

`mix status`'s `names` line carries D3's reason in shorter words: *"hosts file, no wildcards
(programs on this machine do not ask systemd-resolved)"*.

## D5 — what is parsed, and how strictly

- `/etc/nsswitch.conf`: the first line whose first field is `hosts:`; comments dropped. A
  `[...]` action is skipped as a token. `resolve` counts when it appears before `dns`, or when
  `dns` does not appear at all.
- `/etc/resolv.conf`: the first line whose first field is `nameserver`; its second field compared
  as an address with `127.0.0.53`.
- A file that cannot be read counts as *not* reaching resolved for that file, and the other file
  can still decide. When both are unreadable, the reason says so in its own words.

Pure functions over the two files' contents, unit-tested with the measured files above, an Ubuntu
desktop's (`files mdns4_minimal [NOTFOUND=return] dns` with the stub), and one with
`libnss-resolve`.

## D6 — measured on the same machine before it ships

1. With this branch's package installed: `mix status` names the reason; the hosts block keeps
   `m34.test`; the queue holds no `resolver-apply` and no *"remove block"*; `mix doctor` shows the
   note; `mix uninstall` still removes a link an earlier grant left.
2. **With the owner's permission**, since the machine serves other things: `sudo apt install
   libnss-resolve`, restart the daemon, and see `getent hosts anything.m34.test` answer `127.0.0.1`
   while AdGuard stays the upstream. Then `sudo apt remove libnss-resolve` to put the machine back.
   If this step is not allowed, D4 ships without its last sentence.

## Not here

- **macOS and Windows.** `/etc/resolver` is read by mDNSResponder and NRPT by the DNS Client, which
  is every program's path on those systems. Nothing comparable has been found bypassing them, and
  this task does not go looking.
- **Changing the person's DNS setup.** MixEngine does not install `libnss-resolve`, turn the stub
  back on, or edit `nsswitch.conf`. Each is a choice about the whole machine's DNS, and
  `mixengine-elevate` does not get a new operation for it.
- **A real lookup in the doctor.** `mix domain status` already does one per name, and it keeps that
  job.

## MixLab

No new method and no new screen. Settings → MixEngine's *General* section reads
`status.dns.wildcards`, which becomes empty, so it shows its existing no-wildcards line. Its
*Diagnostics* section lists `daemon.doctor`'s checks unchanged, so D4's note appears there. The
elevation dialog stops showing a `resolver-apply` row it could never apply. Nothing in
`apps/desktop` changes.
