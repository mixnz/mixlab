# MixEngine build plan

Phases are ordered. Work top to bottom — each phase depends on the ones above it. Tick items as they
land; when new work appears, insert it **where it belongs in the order**, not at the end.

Each phase lives in its own file; this page is the index. Task numbers (`T1`…`T110`) are global and
never reused, so a task keeps its number wherever it is cited — which is why phase 6 is a gap rather
than a renumbering, and why T56 and T64 keep their numbers in the phases they moved to. Phase 6's
gap is now the shape of a decision reversed rather than a task abandoned: the desktop application
it planned is arriving as phases 11–13, from the repository that built it in the meantime
([ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md)).

Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** = has a platform-layer component and
needs verification on Windows + macOS + Linux.

**Done** is how many `[x]` the phase's own file holds and **Tasks** is every entry in it, `[~]` and
`[ ]` counted too. Both are *derived*, so read them off the files rather than remembering them —
five rows of this table had drifted by 2026-09-13, each one a follow-up filed into a phase by
somebody who did not then come back here:

```bash
for f in docs/roadmap/phase-*.md; do
  echo "$f  $(grep -c '^- \[x\]' "$f") / $(grep -c '^- \[.\]' "$f")"
done
```

---

## Phases

| Phase | Goal | Tasks | Done | Milestone |
| --- | --- | --- | --- | --- |
| [0 — Foundations](phase-0-foundations.md) | Daemon starts, CLI talks to it, state persists | T1–T11 | 16 / 16 | **M0** `mix status` prints a healthy daemon on all three OSes in CI |
| [1 — Process supervision](phase-1-process-supervision.md) | Run and babysit arbitrary programs correctly | T12–T19c | 15 / 15 | **M1** the daemon adopts what survived a kill and cleans what did not |
| [2 — Runtimes](phase-2-runtimes.md) | Multiple PHP/Node/Python/Ruby/Go/Java versions, selectable, and Composer | T20–T29 | 16 / 16 | **M2** `php -v` differs between two directories, no shell hook |
| [3 — Services](phase-3-services.md) | Web server, databases and caches with generated config | T30–T38, T99 | 18 / 18 | **M3** caddy + mariadb + redis healthy in under 10 s warm |
| [4 — Sites & elevation](phase-4-sites-and-elevation.md) | `http://blog.test` works, creating a site prompts for nothing | T39–T47b, T64, T93 | 18 / 18 | **M4** a site opens with zero prompts after first-run setup |
| [5 — HTTPS](phase-5-https.md) | Green padlock, automatically, forever | T48–T54, T98 | 9 / 9 | **M5** `https://blog.test` trusted in every browser |
| ~~6 — Desktop GUI~~ | **Withdrawn** — a GUI is a client in its own repository, see [ADR 0011](../decisions/0011-no-gui-in-this-repository.md) | — | — | ~~M6~~ |
| [7 — Efficiency](phase-7-efficiency.md) | Deliver the promise that idle costs nothing | T68–T73 | 10 / 10 | **M7** 30 idle minutes leaves only the daemon and the web server — **met**, both halves measured by `bench` |
| [8 — Differentiators](phase-8-differentiators.md) | LAN sharing, blueprints, extensions, the standalone client | T74–T84, T77b, T125–T125a | 25 / 25 | **M8** capture, apply, open in the standalone client, test from a phone |
| [9 — Ship](phase-9-ship.md) | Installers, updates, docs, beta | T56, T85–T92, T94–T95 | 20 / 21 | **M9 — v0.0.1** |
| [10 — Client surface](phase-10-client-surface.md) | What `client-surface.md` claims about itself is true | T96–T97, T183 | 3 / 3 | **M10** the standalone client's Dashboard and Settings draw whole, with no business logic in the client — **met** |
| [11 — The desktop app comes home](phase-11-the-desktop-app-comes-home.md) | The standalone client's application builds and tests from this repository, unchanged | T100–T103 | 4 / 4 | **M11** the window builds green in this repo's CI on three OSes and behaves as the standalone client 0.0.33 |
| [12 — One product](phase-12-one-product.md) | One installer, one updater, a standalone-client user's data comes across | T104–T107, T111, T165 | 7 / 7 | **M12** one download installs five binaries and either updater replaces all five; `mixnz/<old>` archived |
| [13 — Profiles](phase-13-profiles.md) | A person who never wanted a database client never sees one | T108–T110 | 3 / 3 | **M13** first-run picks a profile; *MixEngine* hides the toolbox, Settings brings it back |
| [14 — A window a new user can start from](phase-14-a-window-a-new-user-can-start-from.md) | One button makes a working site, a reboot keeps it, the menu can be read | T112–T129 | 22 / 22 | **M14** one button and one prompt on a fresh install open a working `https://<name>.test`, and a restart leaves it serving |
| [15 — What a terminal inherits](phase-15-what-a-terminal-inherits.md) | A terminal can open its databases, run its global tools, and reach its own HTTPS sites | T130–T134 | 5 / 5 | **M15** `mysqldump` is a command, `npm install -g yarn` makes `yarn` one, and a Node program fetches `https://<site>.test` |
| [16 — One site, many backends](phase-16-one-site-many-backends.md) | A site forwards path prefixes to several backends, rewriting the prefix on the way out | T135–T142 | 8 / 8 | **M16** one site answers `/` from disk, `/api` from a port, `/abc` from another as `/xyz`, on both front ends — **met**, measured through Caddy 2.11.4 and nginx 1.31.3 |
| [17 — A disk somebody chose](phase-17-a-disk-somebody-chose.md) | The four directories that grow can be put on another disk, from the window or the command line, while the choice is still free | T143–T147, T166 | 6 / 6 | **M17** a fresh install offers a disk before anything is installed, a runtime and a service land on it, and an elevation prompt still succeeds — **met**, measured on macOS with `data/` on an external volume and nothing left waiting for permission |
| [18 — What a machine lacks](phase-18-what-a-machine-lacks.md) | What an artifact requires is read before it downloads: what can be installed is, once somebody agrees, and what cannot names the version that runs | T148–T152 | 5 / 5 | **M18** on a Windows machine with no Visual C++ runtime, choosing PHP 8.3 and agreeing once produces one approval dialog naming Microsoft Corporation and ends with `php -v` answering |
| [19 — MongoDB](phase-19-mongodb.md) | The published MongoDB releases install and run as a service, what they ask of the processor is judged, and MixLab opens one | T153–T156, T164 | 5 / 5 | **M19** `mix service create mongodb@main 8.3.11` keeps a document across a restart, and MixLab's Open lands in a Mongo tab connected to it |
| [20 — MixLab redesigned](phase-20-mixlab-redesigned.md) | Every screen drawn from one token set, in two themes, at the density its content needs | T157–T163 | 7 / 7 | **M20** every screen renders in both themes with no colour literal outside `App.css`, and the connection editor and Dashboard match their artboards |
| [21 — A site that stays up](phase-21-a-site-that-stays-up.md) | A site that was up stays up until a person stops it | T167a–T167g | 7 / 7 | **M21** a daemon restart leaves every site answering without a click, and with Save battery off nothing is idle-stopped |
| [22 — MixEngine in the tray](phase-22-mixengine-in-the-tray.md) | An icon on all three systems that starts and stops services, stops MixEngine and opens MixLab | T168a–T168g | 7 / 7 | **M22** after a login with the switch on, the icon alone stops MariaDB, stops everything and shuts MixEngine down |
| [23 — One home for the documentation](phase-23-one-home-for-the-documentation.md) | Every document for people under `docs/`, every spec with its status, links checked in CI | T169a–T169d | 4 / 4 | **M23** the `lint` job runs `node scripts/check-docs.mjs` on `master` and it passes |
| [24 — A test job that scales](phase-24-a-test-job-that-scales.md) | A branch run is waited for 15 minutes or less; a new suite adds a parallel leg, not serial minutes | T170a–T170l, T171a–T171c, T184 | 16 / 16 | **M24** in a warm branch run every leg finishes in 15 minutes or less, `build` in 20, and none passes 30 — **met**, measured by run 35481399561: 34 jobs green in 18.4 minutes, longest leg 14.9 |
| [25 — A workflow a person can read](phase-25-a-workflow-a-person-can-read.md) | A person reads the CI job they change without the other thirteen | T172a–T172e | 5 / 5 | **M25** no workflow file passes 500 lines and a full run matches the one before in jobs, test counts and artifacts |
| [26 — A window that builds in under ten](phase-26-a-window-that-builds-in-under-ten.md) | The longest leg of `build` finishes in ten minutes, with no cache and no change to what a tag builds | T173a–T173c | 3 / 3 | **M26** `window (windows-latest)` finishes in 10 minutes or less and `build`'s artifacts and probes are unchanged — **met**, measured by run 35489039746: 9.1 minutes, 34 jobs green, fifteen artifact names identical |
| [27 — A rehearsal of the release build](phase-27-a-rehearsal-of-the-release-build.md) | What a release is built with is built every week, so a tag is never the first time | T174a–T174d | 4 / 4 | **M27** a `release-exact` run on `master` is green inside every timeout, happens weekly unasked, and the checklist names it before tagging — **two halves met** by run 35491317573 (LTO links; the widest leg uses half its timeout), the weekly one waits for the first Monday after this lands |
| [28 — Dependencies that earn their place](phase-28-dependencies-that-earn-their-place.md) | Nothing is compiled, shipped or audited because a plugin's default feature asked for it | T175a–T175c | 3 / 3 | **M28** no `image` and no `moxcms` in the window's `--timings`, terminal paste still works, and both the bson and the db-crate questions are answered with a number — **met**, 898 units against 908, and two measured refusals |
| [29 — One name to find it by](phase-29-one-name-to-find-it-by.md) | One name for the product, and the engine keeps its own where that is still the right word | T176a–T176g | 7 / 7 | **M29** searching for MixLab reaches the repository, the handbook and the download page, the *do I need both* question is answered in one sentence, and no identifier moved — **met**, and the redirects were measured rather than predicted |
| [30 — A copy only you can read](phase-30-a-copy-only-you-can-read.md) | A person's second machine has what they ticked, and the server that carried it cannot read it | T177a–T178d | 19 / 19 | **M30** two machines agree on exactly what was ticked, a revoked device stops syncing, and the server's database yields no plaintext — **the CI half met** by run 35646590878, conformance green against both servers; **the two-machine half met** 2026-09-22 by hand: rows not ticked stayed home, edits and deletions crossed, a removed machine was signed out, a move worked, and the self-hosted server's SQLite held no plaintext beyond the email and device name D1 allows |
| [31 — MixLab updates itself](phase-31-mixlab-updates-itself.md) | A MixLab user is told about a release and installs it from MixLab; MixEngine never looks or installs unasked | T187a–T187g | 7 / 7 | **M31** on Windows, a MixLab whose MixEngine never started comes back on the next release from Settings with no `mixengined` at any point, one with MixEngine running keeps its services, and an idle `mixengined` makes no request to the feed |
| [32 — The tray is MixLab's](phase-32-the-tray-is-mixlabs.md) | MixLab runs in the background on every preset; a module lends the tray a section | T192a–T192e | 5 / 5 | **M32** on *Database tools*, a terminal session survives the close button and the icon brings it back; with MixEngine visible the panel works as M22 says |
| [33 — A line shows its newest](phase-33-a-line-shows-its-newest.md) | The available list is one row per line, and an installed version updates to its line's newest patch on a click | T193a–T193d | 4 / 4 | **M33** PHP 8.4.24 serving a site updates to 8.4.25 from MixLab with its extensions kept and 8.4.24 gone; a MariaDB 11.4 instance updates within its line with its data intact |
| [34 — A headless host](phase-34-a-headless-host.md) | A Linux release with no desktop runs databases, keeps their passwords, and comes back after a reboot on its own | T194a–T195 | 6 / 7 | **M34** on an Ubuntu 24.04 server reached only over SSH, a MariaDB first-runs on the file store, survives a daemon restart, and answers after an unattended reboot |
| [35 — An index that stays small](phase-35-an-index-that-stays-small.md) | The package index costs one small request when nothing was published, and only the kinds a home uses when something was | T196a–T196e | 5 / 5 | **M35** an idle daemon asks for one signature every six hours, a fresh home listing runtimes fetches six kind files and not eighteen, and an upgraded home cannot be walked backwards |
| [36 — One command installs MixLab](phase-36-one-command-installs-mixlab.md) | One line in a terminal picks, checks and installs the right installer | T197 | 1 / 1 | **M36** `curl … \| sh` and `irm … \| iex` install the headless programs on a clean Linux, macOS and Windows runner, and `mix --version` names the release |
| [37 — The window closes its known gaps](phase-37-the-window-closes-its-known-gaps.md) | Every daemon method a person in the window needs has its button; the rest say why not | T199–T199a | 2 / 2 | **M37** `check-client-surface` reports 0 known gaps |
| [38 — An add-on works the moment it is installed](phase-38-an-add-on-works-the-moment-it-is-installed.md) | Installing an add-on shows progress, ends in one row, and a web-app opens at once | T200–T200c | 4 / 4 | **M38** Adminer opens with no daemon restart |
| [40 — What the removal walk found](phase-40-what-the-removal-walk-found.md) | A blueprint routes around a taken account name, a failed start says why and names its repair, and the tests and `watch-ci.sh` say what they found | T202–T202c | 0 / 4 | **M40** `laravel-1` applies on a server with a foreign `laravel-1` account, and names the account it used |

[Parked](parked.md) — revisit deliberately, do not start early.

**The release line is `0.0.x`, and it always was.** M9 shipped as **v0.0.1**, not the `v0.1.0` this
table named until 2026-09-06; the literal survived here because `66695d0`'s sweep deliberately
spared `docs/roadmap/`, reading the number as a milestone still ahead rather than as the stale
half of a rename — which is exactly the reading a version that never shipped invites.

## Where we are

**Phase 37 is built — 2 of 2, and M37 is met: `node scripts/check-client-surface.mjs` reports 0
known gaps.** Nine daemon methods had sat in `knownGaps` since T182i: things `mix` could do and a
person in MixLab could not. Six are buttons now — Start, Stop and Delete in a site's row menu,
Cancel on a running job and on a blueprint being applied, *Check served* on Domains, *Write
manifest* on Projects — and three (`job.list`, `cert.ca_rotate`, `cert.ca_uninstall`) are `cliOnly`
with their reasons written down. Deleting a shared site withdraws the share on the road
`site.unshare` takes (T199a), so the window's guard against it went with it. The exceptions file's
`knownGaps` is empty, and that list only shrinks.
Design: [2026-10-06-t199-the-window-closes-its-known-gaps-design.md](../specs/2026-10-06-t199-the-window-closes-its-known-gaps-design.md).

**Phase 36 is built — 1 of 1, and M36 is met by CI against the newest release.** One line in a
terminal installs MixLab on a new machine: `install.sh` on macOS and Linux, `install.ps1` on
Windows, both kept on `master` and served by the handbook's site, so a fix to either needs no
release. The script picks the file for the system, the architecture and the package family, checks
the `.sha256` and then the `.minisig` — fetching a pinned minisign for the run on a machine that has
none — and refuses a validly signed file served under another name. CI run 37427456570 on
2026-10-06 dry-ran both scripts on the three runners and installed the headless package for real on
Ubuntu, in Fedora and openSUSE containers, on macOS and on Windows, with `mix --version` naming the
release each time.
Design: [2026-10-05-t197-one-command-installs-mixlab-design.md](../specs/2026-10-05-t197-one-command-installs-mixlab-design.md).

**Phase 35 is built: 5 of 5, and M35 is met by tests against a signed registry.** The package
index is read as `mixengine-packages`' schema 2: a signed root of about two kilobytes and one file
per kind, named by hash. A daemon whose cache is past six hours asks for a 308-byte signature and
stops there when nothing was published; a release of one kind costs that kind's file; a kind file
that does not match costs that kind and is named in the lists. The published set was read through
the new client on 2026-09-30 and decodes to exactly what `index.json` says.
Design: [2026-09-30-t196-mixengine-reads-index-schema-2-design.md](../specs/2026-09-30-t196-mixengine-reads-index-schema-2-design.md).

**Phase 33 is built: 4 of 4, and M33 is met on Windows.** The available lists show one row per
version line, and an installed runtime or server updates to the newest patch of its line, carrying
its sites, pools, instances, extension choices and pins, with the old version removed unless
something still needs it. The `#[ignore]`d real-server suites prove the milestone: a PHP site still
served through Caddy after its PHP is updated, and a MariaDB instance keeping its databases.
Design: [2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md](../specs/2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md).

**Phase 32 is built — 5 of 5, and M32 waits on CI's Linux and macOS legs.** The tray belonged to
MixEngine: without the `mixengine` module there was no icon, and the close button quit MixLab and
every terminal session with it. Now the icon, close-to-tray, the login switch and the panel's frame
are MixLab's, on every preset; the `mixengine` module lends the panel a section, and without one a
click on the icon brings the window back
([ADR 0058](../decisions/0058-the-tray-is-mixlabs-and-a-module-lends-it-a-section.md)). A development
build was checked by hand on Windows.
Design: [2026-09-29-t192-the-tray-is-mixlabs-design.md](../specs/2026-09-29-t192-the-tray-is-mixlabs-design.md).

**Phase 30 is built — 19 of 19, and M30 is met.** MixLab has an account, so a second machine
has the saved connections, REST collections, snippets and, one switch each, the saved passwords the
first one has. Everything is encrypted on the machine before it leaves; the server holds ciphertext
and opaque ids, and nothing syncs until a person ticks it. A forgotten password is recovered with
the recovery key and loses nothing, an account moves between servers unchanged, and a server that is
closing says so in the window a month ahead. The server is `server/` in this repository, two
implementations of one protocol
([ADR 0046](../decisions/0046-the-sync-server-lives-beside-the-client-it-serves.md)), and one CI run
proves they agree: run 35646590878 passed the conformance suite against both. Anyone can run their
own, on Cloudflare Workers or from the container image, and the handbook says how. The other half
of M30 was checked by hand on two machines on 2026-09-22.
Design: [2026-09-20-t177-a-copy-only-you-can-read-design.md](../specs/2026-09-20-t177-a-copy-only-you-can-read-design.md).

**Phase 29 is built — 5 of 5, and M29 is met.** One product with two names that contain neither each
other nor a hierarchy was what people could not search for and what made them ask whether they
needed to install two things. MixLab is now the name of the product; MixEngine keeps the daemon, the
CLI and its vocabulary, the crates, `MIXENGINE_HOME` and the headless distribution, because there it
is still the right word
([ADR 0044](../decisions/0044-mixlab-is-the-product-and-mixengine-is-the-engine.md)). The one thing
lost is the handbook's old address; the release feed's redirect survives, which was measured rather
than assumed. It ran before phase 30 because phase 30 prints a name on a sign-in screen, a
verification email and a second repository.

**Phase 22 is built — 7 of 7, and M22 waits on a person at three desktops.** MixEngine has an icon
in the tray or the menu bar: a webview panel on macOS and Windows, and the same panel behind a
three-item menu on Linux, which tells an application nothing about clicks on its icon. MixLab can
start there at login, a switch beside the daemon's
([ADR 0042](../decisions/0042-mixlab-starts-at-login-when-a-person-asks-it-to.md)).
Design: [2026-09-19-t168-mixengine-in-the-tray-design.md](../specs/2026-09-19-t168-mixengine-in-the-tray-design.md).

**Phase 21 is built — 7 of 7, and M21 waits on a measurement.** A site that worked this morning and
does not open now is the one sentence that ends MixLab's use. On a default home two defaults caused it: idle shutdown was on, and the web
server did not start with the daemon. [ADR 0041](../decisions/0041-mixengine-stops-nothing-a-person-did-not-ask-it-to.md)
turns both around.
Design: [2026-09-19-t167-a-site-that-was-up-stays-up-design.md](../specs/2026-09-19-t167-a-site-that-was-up-stays-up-design.md).

**Phase 19 is built — 4 of 4, and M19 is half measured.** `mixengine-packages` published MongoDB 6.0
to 8.3 on 2026-09-15, and nothing here could run one. Now a `mongodb` recipe does, with no accounts
and two locks in their place — loopback only, and 27017 never opened to a network — and the AVX
every release states is judged before anything downloads. MixLab opens it in a Mongo tab. A real
8.3.11 keeps a document across a restart on Windows; the window's click waits for a person.
Design: [2026-09-17-t153-mongodb-is-a-service-design.md](../specs/2026-09-17-t153-mongodb-is-a-service-design.md).

**Phase 18 is built — 5 of 5, and M18 is half measured.** In Windows Sandbox, with no Visual C++
runtime, `mix runtime install php 8.3.33 --yes` installed Microsoft's redistributable and then PHP,
and `php -v` answered. The same run found MixEngine itself could not start on that machine — its
binaries imported `vcruntime140.dll` — so every Windows release is now linked with `+crt-static`.
What the Sandbox cannot show is Windows' approval dialog (it runs with UAC off) and the MixLab click;
those wait for a machine with UAC on. The phase comes from a Windows machine with no Visual C++ runtime, where
installing PHP downloads it, unpacks it, and ends at a loader error the index predicted before the
first byte moved: every Windows PHP from 7.0.33 to 8.5.9 states a `requires.vcredist`, and T92
recorded that nothing reads it. The phase reads it. **What can be installed is** — one Microsoft
redistributable satisfies 21 of the 22 Windows artifacts that name one — after a person has agreed,
from a fixed address, and only once its signature says Microsoft; **what cannot is routed around**,
because a glibc or a macOS version is the operating system itself, and the newest release that does
run is named instead. `SmokeTest` stays in front of every install. The one exception this makes to
the security model is ADR 0037's to state, and the installer's real behaviour is measured before a
line of it is written.
Design: [2026-09-16-t148-what-a-machine-lacks-is-installed-not-reported-design.md](../specs/2026-09-16-t148-what-a-machine-lacks-is-installed-not-reported-design.md).

**Phase 17 is done — 5 of 5, and M17 is met.** It comes from a machine with a small internal disk
and an external SSD, where pointing `MIXENGINE_HOME` at that disk produced an elevation prompt that
took a password and then failed: macOS gates a removable volume behind TCC, and the elevated helper
— spawned through `osascript` and `authtrampoline` — arrives with no responsible process to inherit
a grant from, so it could not read its own request. Two things fall out. **The home stays where each
OS puts it**, because `run/` is the helper's whole contact surface and `Paths::new` already refuses
to move it — which is what makes this phase need no Full Disk Access from anybody. And `[paths]`,
which has moved the four directories that grow since the beginning, becomes reachable from the
window, from `mix`, and from the flags a daemon is started with — refused at the one moment it stops
being safe, which is the first row that records an absolute path. Measured on macOS with
`runtimes/`, `packages/` and `data/` on an external `noowners` volume: a runtime installed there and
ran from there, and the elevation queue granted to nothing waiting, with no Full Disk Access given
to anything. **The measurement found two bugs of its own**, both now fixed: `helper-install` blamed
the directory it was writing to when what had failed was reading its own binary off that volume, and
a grant that did nothing reported only counts — so the same prompt was answered eight times against
a sentence nobody had been shown. The second was older and wider. No PHP 7.0 or 7.1 this product
ever installed loaded any of its shared modules, on any system, because the generated ini named them
without the suffix PHP hands the loader verbatim before 7.2 — and the suite written to catch exactly
that was pinned to 8.3, the branch whose fallback makes a wrong name work. Module names are judged
against every PHP in `MIXENGINE_PHP_RUNTIMES` now, 7.0.33 among them.
Design: [2026-09-15-t143-a-disk-somebody-chose-design.md](../specs/2026-09-15-t143-a-disk-somebody-chose-design.md).

**Phase 16's tasks are done — 8 of 8, and M16 is met.** One more complaint from somebody using the
finished product — a site can only forward to one place — and reading for it found two holes beside
it, both now closed: a path in a `reverse-proxy` upstream was accepted by the daemon and refused by
Caddy, which cost *every* site on the machine its new configuration, and an upstream reached a
Caddyfile with no check for a newline. A site is now a kind plus an ordered set of path routes
([ADR 0035](../decisions/0035-a-site-is-a-kind-and-a-set-of-routes.md)). What is still asserted
rather than served is one rendering: the nginx php-fpm route's nested PHP handler, which needs a
suite with a PHP to hand.

**Phase 15's tasks are done — 5 of 5**, and what M15 wants next is the same clean-machine smoke M14
is waiting on, with three more things typed into the terminal it opens: `mysqldump`, then
`npm install -g yarn && yarn --version`, then a Node program fetching one of that machine's own
HTTPS sites. It is written against three more complaints from somebody using the finished product,
and none of the three was a bug in what existed — each is a place where `<root>/bin` or the
environment a shim hands over had never said anything. Two decisions came out of it:
[ADR 0033](../decisions/0033-bin-is-a-projection-of-what-is-installed.md) and
[ADR 0034](../decisions/0034-mixengines-authority-reaches-a-runtime-through-a-generated-bundle.md).

**Phase 14's tasks are done, and what M14 still wants is the clean-machine smoke on each
OS**: a fresh install, one button, one prompt, a browser on a working `https://<name>.test`, and
then a restart that leaves it serving. Everything below that line is automated and green.
The restart half once wanted a decision as well as a run — since T129 nothing ticked the front
end's `autostart`, so a restart served nothing until somebody ticked it once.
[ADR 0041](../decisions/0041-mixengine-stops-nothing-a-person-did-not-ask-it-to.md) took that
decision in phase 21: a front end is created with `autostart` on unless asked otherwise, and a
one-time migration turned it on for the rows that already existed. The clean-machine smoke is the
one thing M14 still waits on.

**Phase 13 is done — 3 of 3.** The window has a profile (T108), a first tab
that follows it (T109) and a bridge for a handoff to a module somebody turned off (T110). What
phase 14 is about is the first ten minutes after an install rather than what the window *is*:
[phase 14](phase-14-a-window-a-new-user-can-start-from.md) is written against four complaints from
somebody using the finished product, and only one of them — a service's `autostart`, claimed by
[client-surface.md](../features/client-surface.md) §4 and never read by anything — is a missing
method rather than a missing affordance.
Design: [2026-09-11-a-window-a-new-user-can-start-from-design.md](../specs/2026-09-11-a-window-a-new-user-can-start-from-design.md).

**Phase 11 is done — 4 of 4 — and M11 is reached; phase 12 is next.** Phases 0 to 10 are done and
v0.0.1 shipped. [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md)
reverses ADR 0011, and the standalone client — already a complete client of this API — now lives under
`apps/desktop/`, builds and tests in this repository's CI on three operating systems, and opens
against a running daemon as the standalone client 0.0.33 did ([phase 11](phase-11-the-desktop-app-comes-home.md)).
What comes now makes it MixEngine's window, named MixLab (phase 12), and makes its database client
optional to look at (phase 13). The design for all three is one document,
[2026-09-08-the-desktop-client-in-this-repository-design.md](../specs/2026-09-08-the-desktop-client-in-this-repository-design.md).
**Phase 12's tasks are done; what M12 still wants is the clean-machine smoke on each OS.** T104 gave
the application MixLab's name, identity and this workspace's version, and brings a standalone-client user's data
across once; T105 put it in all six installers and added a **headless** archive per OS/arch for the
machine that has no display — one download now installs five binaries. T105a closed the one line
T105 left open: the AppImage does **not** carry WebKitGTK
([ADR 0028](../decisions/0028-the-appimage-does-not-carry-webkitgtk.md)), the window's floor is the
distribution's and is measured off the binary on every Linux leg, and `AppRun` names which floor a
machine missed. T106 left one updater — the standalone client's plugin, key and feed are gone, the payload carries
the window, and it relaunches itself after its own executable is swapped. T107 made *where
MixEngine is* and *where its window is* one answer each, in `mixengine-platform`, held to the
packaging scripts that write them: `mix database open` from a terminal now lands in a tab in the
running MixLab, on an install that has no `<old>` extension at all.
Of the two debts phase 11 left where they were found, one is closed: `[daemon] ipc_path` in
`config.toml`, parsed and used by nothing, is gone from the template and retired in `Config` the way
`[bin]` was (ADR 0057) — still read, so a home that uncommented it still starts, and the daemon logs
that it does nothing. The other stands: the standalone client's `tool-downloads.yml` is not wired
into this CI.

**Phase 0 is done**, and **M0 is reached**: `mix status` starts a daemon if there is none, talks to
it over the local endpoint and prints what it says, in both renderings, proved end to end by
`crates/mixengine-cli/tests/status.rs` — green on all three runners, not only the one it was written
on. The Windows third of that runs as an administrator (T2b), which changes nothing about what it
proves: `status.rs` asserts nothing a token decides. T9a closed it last, and late on purpose: a
daemon can now be *asked* to stop rather than found and killed, it stops its services in reverse
dependency order first, and the whole of that is bounded by one budget — `config.toml`'s over the
API, and whatever Windows's console clock allows when the OS is the one asking.

**Phase 1 is done.** The vocabulary, the state machine, the supervision mechanisms, the log
capture, the dependency graph, the runner, the registry, the `service.*` surface, the CLI over it and
crash recovery are in: a declared service can be started, watched, restarted and stopped through a
real socket, every move is persisted and announced from one value, and a daemon that is killed no
longer takes the truth with it — the next one adopts what survived, stops what it cannot supervise
and clears the rest, before it serves a client. Every check a `ServiceSpec` can name is now one the
supervisor can make, and a service that needs a command of its own to shut down cleanly gets one
(T15a) — which is what Phase 3 was waiting for. A service's output now reaches a person as well: on
`GET /logs/{id}` and under `mix service logs`, on a stream of its own rather than as an event
([ADR 0009](../decisions/0009-logs-travel-on-their-own-stream.md), T16b) — and reaches them
*whole*, since T16c: a subscription begins when it is made, so a service's first lines used to reach
`current.log` and never the ring, and the capture now hands over what it is already holding together
with the subscription rather than only the second of the two. Each task's decisions — and
the four ADRs the work forced — are written up in
[phase-1-process-supervision.md](phase-1-process-supervision.md). **This page does not repeat them.**

**Phase 2 is done, and M2 is reached.** **T20a unblocked it**: PHP 8.3.33 exists
for Windows x86_64, macOS aarch64 and Linux on both architectures, each one run from a directory it
was moved to and made to load an extension there, described by a minisign-signed index at a permanent
URL. The pipeline that produced it is its own repository,
[`mixengine-packages`](https://github.com/mixnz/mixengine-packages), built on GitHub runners
because this project has no macOS or Linux of its own and an artifact nobody can reproduce is one
nobody can audit. **T20 reads that index** — signature checked before the JSON is parsed, cached for
six hours, served stale rather than not at all when the network is gone, and refused when a server
offers a document older than the one already held. **T21 installs what it names**, as one transaction
whose commit is a rename: resumable download, checksum, unpack into a staging directory beside the
destination, a run of the binary itself, and only then the move into place. **T22 is the job system**
— `jobs` rows, the two events, `job.list|status|wait|cancel`, cooperative cancellation, and a boot
that closes what a stopped daemon left running.

Each of those three shipped with nothing able to reach it, deliberately, and **T23 is the method in
front of each**: `runtime.install|uninstall|list_installed|list_available|set_default`, with
`mix runtime` and `mix job` over them. `runtime.install` is the job system's first and only producer
— the call answers a `JobSummary` the moment the row exists, the download reports through the
`Watcher` T21 shaped after `JobHandle`, and what the finished job carries is the same
`RuntimeSummary` a listing is made of. Proved end to end on a real socket against a signed index and
a real archive, in `crates/mixengine-daemon/tests/runtimes.rs` and
`crates/mixengine-cli/tests/runtime.rs`: a version is offered, installed, listed, chosen and removed,
and the directory on disk agrees at every step.

**T24 answers the question the rest of the phase was deferring to**: which version a directory uses.
`core::resolve` walks the four sources in order — a flag or `MIXENGINE_PHP`, the nearest
`mixengine.toml` *that names the language*, a registered project, the kind's default — and answers
with the installed runtime **and the source that decided it**, because "which PHP is this?" is asked
precisely when the answer is surprising. The grammar it needed went to `mixengine-proto` beside the
identifier it is about: `VersionConstraint` (a prefix or a caret) and `RuntimeVersion::cmp_precedence`,
which is a different order from the derived one and the one anything choosing a version wants.
`runtime.resolve` and `mix runtime resolve` are over it.

**T25 is that answer's first caller with no daemon to ask**, and the first binary here that is not a
client of one: `mixengine-shim` reads the name it was invoked by, resolves in its own process against
the database opened read-only, and then *becomes* the program — `exec` on Unix, a child in a Job
Object with the console interrupts swallowed on Windows, the program's own exit code either way.

**T26 gave that binary somewhere to be, and gave the directory it lives in a way onto the PATH**, and
with it **M2 is reached**: `crates/mixengine-shim/tests/shim.rs` runs the real shim out of a `bin/`
that `core::shims::refresh` filled, from two directories, and gets two different PHPs with no daemon
running and no shell hook installed. The two halves keep opposite policies about being done unasked —
`bin/` is a projection of a compiled-in table and is refreshed on every start, while the PATH is a
file in the user's home or a value in their registry hive and is written only by `path.install`. That
split, and the fact that `PathIntegrationApply` came *off* the privileged-operation list rather than
being implemented, are [phase 2](phase-2-runtimes.md)'s to keep.

**T27 is done, and all four languages are in the index**: twenty-five packages and one hundred and
eighteen artifacts, Node.js on five lines, Python on five, Ruby on four. What it cost *here* is three
tests and documentation — the kind enum, the command table, the smoke test and `resolve` were about
four languages rather than about PHP from the start, so every recipe lives in `mixengine-packages`.
Windows on ARM is a runtime target for three of the four now, where `windows.php.net` has never
published one at all. Ruby turned out to be two answers rather than one: RubyInstaller covers Windows
on both architectures, while macOS and Linux were the last cell in the whole table that nothing could
be borrowed for.

**[T27b](phase-2-runtimes.md) closed that cell and audited the packing code doing it.** Ruby is
compiled from ruby-lang.org's own source on all four Unix targets with `--enable-load-relative`, YJIT
on, and — the question the task was carved out to answer — **its own OpenSSL, taught to resolve its
default certificate paths against the loaded `libcrypto`'s location** rather than against the
distribution that built it, which is the same idea as the shim and as `--enable-load-relative`,
applied one library further down. Four rounds of CI and not one of them was Ruby: every failure was
in `relocate.py` or in what a check was asking, which is what a *second* build pipeline is for.

**T29 put a number on the promise the shim is built around, and a `bench` job in CI to keep it.** The
budget belongs to the *resolution* — that is where
[runtime-versions.md](../features/runtime-versions.md) puts it — and the resolution takes 0.58 ms on
macOS, 0.74 ms on Linux and 1.71 ms on Windows against a home with five runtimes in it, nine to
twenty-five times inside its 15 ms. What a person waits for is a different number and is reported
rather than gated, because it is process creation nearly all of it: the shim adds 2.19 ms on Linux
and 4.52 ms on macOS, where it `exec`s, and **15.03 ms on Windows**, where it cannot and starts a
second process instead. **T28 closed the phase.** What it was waiting for had
arrived with [T32](phase-3-services.md) — a pool to reload per PHP version, and a measured
`PHP_INI_SCAN_DIR` on all three systems — and what it found was that "prebuilt extension artifacts"
were already inside the archive: the index publishes what each build ships loadable, so the task owed
a switch rather than a second download path. An installed PHP now carries a generated
`etc/php/<version>/conf.d/` that both its pool and the `php` on a terminal read, and
`mix runtime ext enable xdebug` moves one line in it and says what that did to the pool.

**M3 is reached**, and what phase 3 still holds open is T33b, which the milestone does not ask
for. The number the milestone asks for exists, is held
to, and was taken on all three systems: `crates/mixengine-cli/tests/warm_start.rs` installs a real
Caddy, MariaDB and Redis into one home and times a single `mix service start`, in the `bench` job,
gating the **median** of five warm rounds at ten seconds. 875 ms on macOS, 2133 ms on Windows,
3189 ms on Linux. Two findings travelled with it, both kept in
[phase 3](phase-3-services.md): the promise was two different runs in one sentence — *fresh install*
and *warm cache* — which [../features/services.md](../features/services.md) now separates; and the
median passes while the **tail** does not, two Linux rounds at 11.8 s and 15.1 s. That tail is one
service rather than the sequential walker everybody would suspect — Caddy and Redis are 300 ms of it
and MariaDB is the rest — which is why the suite now prints the daemon's own account of any round
that goes over.

What the phase established, in one sentence each. **A `services` row is a rendered configuration and
a runnable spec** (T30), from a `Recipe` compiled into the daemon rather than published by the
package index — so a template travels on MixEngine's release schedule and never on the packaging
pipeline's. **Eight of them exist now**: two front ends (T31, T37), a pool that comes out of a
runtime rather than a package (T32), three databases (T33, T34, T34c), two caches (T35). **A user
reaches all of it through shipped methods** — `package.*` and `service.create|delete` (T31a) — which
is why every supervision fixture is now a row the real method wrote. **A port is allocated when a
row is written** (T34c), free means free on the machine, and a port lost to somebody's XAMPP is
reported with that program's name (T38). **Two instances of one server run side by side** (T36),
because every earlier decision keyed itself by service id rather than by package.

**The three refusals the phase added are what it is really made of**, because each one exists only
because the task before it made the mistake possible: a data directory two rows both name (T36), a
runtime uninstalled under a running pool (T32), and a second front end (T37). Each is refused where
it is written down rather than discovered where the files are opened.

**[T37](phase-3-services.md) closed the phase, and its own finding is that "exactly one front end"
had never been a rule anything could break.** With two front-end recipes it is: `Instancing` is
about a package, both of them answer `Single`, and a home obeying both still gets a Caddy and an
nginx rendered against the same 80 and 443. `Recipe::role` is the one distinction that closes it —
`FrontEnd` or `Other`, defaulted to the second — and the refusal reads `core::services::front_end`,
which answers by role so that neither program is the one the code happens to know about. The recipe
itself is Caddy's shape answered by a server with none of Caddy's mechanisms: **nginx has no admin
endpoint, so the template renders one**, because a TCP accept cannot tell a serving nginx from one
whose workers have all died. And the parity the task owed is literal — the arc both front ends walk
is one file, `crates/mixengine-cli/tests/harness/frontend.rs`, driven twice, with each suite reduced
to four constants over it.

**M1 is reached**: a daemon is killed mid-run, and the next one adopts the process that outlived it
and clears the row of the one that did not — `crates/mixengine-daemon/tests/lifecycle.rs`, with the
registry's own tests under it, green on ubuntu, windows and macos rather than on the machine it was
written on. That mattered more here than it did for M0: the reading the whole task rests on is per-OS
(`GetProcessTimes`, `proc_pidinfo`, `/proc/<pid>/stat`), and CI is what found the stop that reached
a process group nobody was leading — right on Windows, silently forgiven on both others.

Stated no louder than that. What the test proves is the *recovery*, on every system: it makes its own
survivors, because what a killed daemon leaves behind is a different thing on each of the three, and
that half is [ADR 0007](../decisions/0007-supervised-child-owns-a-process-group.md)'s own tests to
keep.

### What is open, and what each one blocks

**One row below says it gates a release that has since been tagged.** **T86a** is still `[~]` and
v0.0.1 shipped anyway, so "blocks the release" describes the intention of 2026-08-23 and not what
happened. The row is left as written rather than quietly softened, because deciding what it gates
*now* — the next release, or nothing — is a call to make deliberately and not while correcting a
version literal. **T41a**, which said the same thing, was answered on 2026-09-22; it shipped with the
question open, and the answer was yes.

| Debt | Blocks | Where |
| --- | --- | --- |
| ~~**T41a** does an unsigned binary load under Smart App Control, and does the hosts write survive Defender~~ — **answered 2026-09-22**, on a Hyper-V machine with SAC enforcing and Defender's current definitions: nothing was refused, the elevated hosts write drew no detection, and `mix doctor` named the policy. The readings and their two limits are in [../features/updates.md](../features/updates.md) | nothing | [phase 4](phase-4-sites-and-elevation.md) |
| **T45's fixed link-local address** — `169.254.53.53/32` is not negotiated and nothing detects a machine already using it | nothing; the whole-state shape makes the fix additive | [phase 4](phase-4-sites-and-elevation.md) |
| ~~**M3's tail** — the warm median is inside ten seconds on all three, and two Linux rounds of five were 11.8 s and 15.1 s~~ — **closed by T99**: the tail was MariaDB 11.4 generating a 4096-bit RSA key at every start for a TLS nobody configured, not cold I/O and not the sequential walker; with a leaf of this home's authority the ubuntu warm median is 592 ms, the five rounds within 3 ms of each other | nothing | [phase 3](phase-3-services.md) |
| **T69's idle shutdown ships switched off** — no recipe offers a default, so nothing is ever stopped unless somebody asks per service | nothing, and it is a choice rather than an omission: a stopped pool has nothing to start it again until **T70**. Turning it on is four `None`s in four recipes | [phase 7](phase-7-efficiency.md) |
| **Keep-warm reaches a project's PHP pool and not its database** — `kept_warm` joins on `sites.php_service_id` alone | nothing while idle shutdown is off. **Widening it needs no new feature**: `site_service_links` has held the edge since `0006`, which T77 established while reading it for capture — the row used to say the widening waited on T77 | [phase 7](phase-7-efficiency.md) |
| ~~**No disk-usage-by-category or cleanup method exists**~~ — **closed by T96**: `daemon.disk_usage` and `daemon.cleanup`, reachable as `mix disk` and `mix cleanup` | nothing | [phase 10](phase-10-client-surface.md) |
| ~~**Which web server is active has no readable/writable state**~~ — **closed by T97**, on [ADR 0026](../decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md): the reading is `ServiceSummary::role` and the switch is `service.set_front_end`, reachable as `mix service front-end` and `mix service set-front-end` | nothing | [phase 10](phase-10-client-surface.md) |

**One of the three gaps the standalone client's Phase 4 spec found is closed by reading it rather than by building
anything.** `ServiceSummary` carries no CPU or RSS, and it should not: those live in
`MetricsSample`, keyed by `MetricsSubject::Service(id)`, and putting them on the summary would break
the invariant T71 exists to hold. The fast cadence is 1 Hz **only while somebody is subscribed to
`GET /metrics`** — the connection *is* the subscription, which is what stops a crashed client
leaving a laptop being polled. A `cpu_percent` on `service.list` is a way to take that cadence
without opening the stream; the alternative is answering with a reading up to a minute old from a
struct that has nowhere to say *when*, and `MetricsFrame.at` exists because a figure without its
moment is a figure that lies. So the Dashboard's "in one read" is **one list plus one stream**, not
two reads per frame: it holds the frame continuously and calls `service.list` again when the event
stream says something changed. Uptime is not missing either — `last_started_at` and `state` are it,
and daemon and client share a clock because they share a machine. [client-surface.md](../features/client-surface.md)
screen 1 said "in one read" and now says which two things the join is between.

**The scaffolding that carried an expiry date has half met it.** `mixengine_testkit::declare` no
longer writes a `services` row: **T31a**'s `service.create` does, over a real socket, so the row every
supervision suite runs against is the one the shipped method writes. What is left of it is the
`packages` row for `fakeservice`, which no index will ever publish and which therefore has no method
to replace it. Its sibling `MIXENGINE_DEV_SPECS` is gone: T30 made a row into a real declaration, and
what a test needs beyond that is a *recipe* for the fixture — one a debug build carries and a release
build does not, and that runs one program rather than whatever a variable named.

**Phase 4 has started, and its elevation half is now built from the bottom to the surface.** The
site model and its four kinds are in (**T39a**), the one-shot helper and its file protocol are in
(**T40**), and **T40a** gave the daemon the one thing that turns a request lying on disk into an
elevated process reading it: an `Elevation` capability on `Host`, with UAC, osascript and pkexec
behind it. **T40b** is the half that decides *when* a prompt is worth spending — a durable queue
whose unique key is the operation itself, one grant slot, `ElevationRequired`, and the degraded mode
a decline leaves behind — and **T64** is the surface over it: `mix elevation grant` prints every
operation and what each will literally change, and only then asks. It refuses to raise anything it
cannot be answered about, which is what a cron job and a CI step look like.

**That stack now has a producer, and MixEngine writes outside `MIXENGINE_HOME` for the first
time.** **T41** made `site.create|update|delete` ask for the machine's hosts file to say what this
home's sites say it should — the whole managed block, never a delta, and only when the disk and the
database actually disagree. The criterion it was written around is one regression test: splice a
block in, replace it, take it out, and the file is byte-identical to the one it started as. The
fixture that used to fill the queue is deleted; both elevation suites now create a site and find an
operation waiting.

**And the machine will now let an unprivileged front end answer on 80 and 443** (**T42**). One
read-only capability says which mechanism this system uses, which port a program must actually bind
to answer 80, and whether the grant is already there; the write is two `PrivilegedOp` variants the
helper validates itself. Windows grants nothing because it reserves nothing, Linux gets
`cap_net_bind_service` written straight as the `security.capability` attribute, and macOS gets a
packet-filter redirect plus the boot-time job that enables pf — the one standing thing MixEngine
installs, argued in [ADR 0012](../decisions/0012-a-boot-time-job-enables-the-packet-filter-on-macos.md). The producer is the daemon's own start-up probe,
which is also the re-probe **T88b** asked for and is what closed it. **T43** then put a front end
behind it: a site file belongs to the front end's own document set, `sites/` is a directory the
recipe declares swept so a removal counts as a change, and a php-fpm site whose pool is gone is left
out rather than failing the render.

**And a home now resolves through its own DNS server** (**T44** built it, **T45** made something send
it a name, and T46a closed with the first). A `hickory-server` on loopback answers `A` for every name
under a managed TLD at any depth, whether or not a site was declared for it — the wildcard that makes
`site.create` cost nothing — and **REFUSES everything else**: no forwarder, no cache, no recursion.
The port is **53535**, not the 5353 three documents named, because 5353 is mDNS's and is held on
every ordinary desktop; `AAAA` is NODATA with an `SOA` rather than `::1`, T41's reasoning applied a
second time.

**T45 is the half that makes the mode real, and every Linux mechanism the documents named was
unusable.** Six rounds of measurement on real runners: a `resolved.conf.d` drop-in with a global
routing domain redirects the **whole machine**, `resolvectl dns lo` is refused by name, a real link
has its servers *replaced*, and NetworkManager is not installed on a stock Ubuntu server. What works
is a `systemd-networkd` dummy link of our own **carrying an address** — without one it is configured,
reports its servers back, and never gets a DNS scope, which is the worst of the four because it reads
as applied. macOS is a marked file per TLD, Windows is one NRPT rule written as registry values
rather than through PowerShell. **The helper is never told where to point**: `127.0.0.1`, the link
and the registry key are compiled into it, and the operation carries only which TLDs and which port.

Two shapes changed with it, and both are corrections rather than costs. **The hosts block is computed
per TLD**, because every mechanism there is scopes to one and `.local` is deliberately never wired —
so a home with `blog.test` and `shop.local` needs a block with exactly one line. And **the wiring is
asked for at daemon start, before any site exists**, which is what makes M4's "zero prompts" true:
ask after the first site and emptying its hosts line is a second operation and therefore a second
prompt. `.internal` joined the managed table on the way, while it was still cheap — the helper is
excluded from auto-update, so a TLD added after a release is refused by every installed copy.

**And a person can now ask what actually happens to one name** (**T46**). `http://blog.test` failing
has four independent causes that look identical from a browser — the name is not declared, no hosts
line was written, no resolver routes the TLD, the server is not answering — so `domain.dns_status`
reports four facts and refuses to collapse them into a verdict, which is what `DnsStatus::wildcards`
had to stop doing one task earlier. The lookup is **`getaddrinfo`, never `nslookup`**, on T45's
measurement that `nslookup` bypasses the NRPT and would report a correctly wired Windows machine as
broken — and it includes the OS cache on purpose, the opposite of what T45's own test needed, because
this asks what a browser sees now rather than whether a mechanism works. `domain.add` and
`domain.remove` came with it: `site.update` could already replace the whole list, but composing one
addition from it is a read-modify-write in a client, and removing a site's primary or its last domain
is refused by name.

**And a person can now ask what is wrong with the machine itself** (**T47a**, the read half of T47).
Nine checks, each read from the subsystem that already owns the answer — the hosts block from T41's
own comparison, the resolver from T45's probe, the domains from T46's report rather than a second
opinion. Two shapes in it are worth more than the checks. **`Note` is not `Problem`**: what MixEngine
can promise about a killed daemon's descendants is total on Windows, the immediate child on Linux and
nothing on macOS, and reporting the macOS answer as a fault would report the operating system as
broken while reporting it as nothing at all is the exact failure [ADR
0007](../decisions/0007-supervised-child-owns-a-process-group.md) exists to prevent — the same
distinction that keeps `hosts_only` a supported mode rather than a permanent fault. And **a `Problem`
carries a closed id, never advice**, so T47b's repairs cannot drift from this build's findings —
which T47b then spent: its dispatch is an exhaustive `match` with no wildcard arm, so a
condition added later stops the repair compiling until somebody decides what fixing it means.
T47b also found the one thing its own design had backwards: a repair may not enqueue and flush
in one call, because T64's promise is that a person reads the batch **before** it is allowed,
and there is no moment to show it in.
**The check that earns its keep is Windows' reserved port ranges**: a bind into one fails with an
access error, so it reads as a permission problem and sends a person to elevation, UAC and the
firewall, none of which is the answer. It also settled `icacls`, which T3a left open for want of a
caller: the whole of what the caller needs is "is inheritance still severed", `icacls` answers that,
and the 150 lines of `unsafe` FFI buy a trustee comparison nothing asks for.

**One recorded debt from it, and T45 widened it:** two accounts on one machine share a single
`# BEGIN MixEngine` block, and now a single resolver wiring as well — so the second home's desired
state replaces the first's. On Windows the two cannot even be told apart by port, NRPT having no
field for one. A machine-wide lock stops them interleaving
a write, and nothing stops that. **T41a** asks the other open question — whether an unsigned build is
allowed to make this write at all, under Smart App Control and Defender's `HostsFileHijack`
heuristic — and it is now owed against the first release rather than against this phase, because
answering it needs a clean VM, which does not exist yet. The certificate that used to travel with
that question no longer does: **T94** took it to phase 9 and **answered it on 2026-09-04** — no
certificate this project can buy repairs Smart App Control, because the images deciding the outcome
are the borrowed runtimes rather than ours
([ADR 0017](../decisions/0017-smart-app-control-is-an-unsupported-configuration.md)). T42 adds the same
debt in its own shape: on macOS the two homes share one anchor with one pair of redirect targets, so
the second front end will want 8080 too and will fail to bind it.

**Phase 7 is done, and the first two of its tasks are about honesty rather than about saving memory.** **T68** put `ResourceLimits` behind a per-*field* answer about what this machine will
really do with each one, so no client can offer a memory cap that does nothing on macOS. **T69** is
the mechanism the whole phase is named for — a service nothing is using is stopped — and it ships
**switched off**, because stopping a pool is only safe once something starts it again on the next
request and that is T70. What it added instead is everything that has to be right before a default
can be turned on: a `ConnectionCount` capability on all three systems, a probe reading beside `ready`
and `health` in the supervisor, a sweeper that spends a policy in *observations* rather than in
elapsed time — a suspended laptop counts none of the night — and a rule every layer repeats, that a
service which could not be measured is never stopped. `services.idle_minutes` therefore has three
states today and not two, so that T70's default can reach the home that never touched the setting
without reaching the one whose owner switched it off. The rest, including the query counter the
roadmap asked for and `ServiceSpec` cannot carry, is
[phase 7](phase-7-efficiency.md)'s to keep. **T70** and **T70a** are the something that starts a
service again — the request or the connection that found it stopped — which is what let idle
shutdown be turned on at all. **T71** is the measuring: two sampling rates in one loop, because
*"sampled only while watched"* and *"a history that says what was eating my battery last night"*
cannot both be true, and the night is the half nobody is watching.

**Both published numbers are now measured rather than promised**, which is what closes M7. **T72**
gates `mixengined` idle under 42 MB — 36 until 2026-09-07, raised when a week of features had put
five to ten megabytes on it on every system, with **T72b** owing the reason — and reports the 60 MB
total beside it, because two thirds of that total is a Go program this project neither wrote nor
tunes. **T72a** gates the cold path at 1.5 s — met at 108 ms on Linux, 129 ms on macOS and 574 ms on
Windows — and the task it took to get there is the phase's own lesson twice over: the roadmap entry
described work T70 had already done, while the thing actually missing was a pool on a socket having
no way to be *asked* whether anybody was using it. Two defects surfaced only once a real request
went through: a counter rule that was reading the daemon's own health checks as traffic, and an
activator that was bound at boot and therefore never for a pool installed afterwards. Neither was
visible from reading the code. **T73** closed the phase, and it is the one task here that changed no
mechanism at all: the three database templates were rendering the values their servers would have
used with no configuration file, under a feature document that said they were tuned. What it refused
is worth as much as what it changed — `max_connections` saves nothing at idle and buys a new way for
a busy afternoon to fail, and an idle php-fpm pool is already stopped, so a smaller one would only
slow down the machine while somebody is using it.

**Both promises are kept.** `runtime.uninstall` refuses over a running php-fpm pool (**T32**) and
over a registered project whose pin the removal would leave with no answer (**T39**), and `--force`
crosses the second and never the first — a broken pin is a statement about the next `cd`, a running
pool is a process serving requests now.

## Working on this file

- Tick a task in **its phase file**, not here; update the `Done` column when a phase moves.
- New work goes into the phase file where it belongs in the order. Give it the next free suffix on
  the task it follows (`T40a`, `T40b`) rather than renumbering anything after it. A task may be
  lettered after the one it is ordered *before* — T19c and T20a both are — as long as it says so.
- A phase file carries its own goal, legend and milestone so it reads on its own.
- **A milestone names the release it shipped as, and nothing here names the current one.** The
  version this tree is at is `Cargo.toml`'s to say; it moves, and a copy of it in prose is a copy
  that goes stale at the next bump. Work that has not shipped is "the next release", never a number
  — a number written for one that has not happened is what let `v0.1.0` survive a rename here and
  read as a milestone still ahead.
- **One note, one place.** A decision that is *in* the code — why this type, why this order, why not
  the obvious alternative — belongs in the doc comment beside it. One that crosses crates belongs in
  an [ADR](../decisions/). What a phase file carries is only what neither can: what a task
  deliberately did **not** do, and which later task is expected to. A note that would still be true
  with the code deleted is a note the phase file should keep; one that merely describes the code is
  one the code should be carrying instead.
- **"Where we are" is the current phase and the open debts, and nothing else.** Not a changelog. A
  finished task is described by its phase file and by the code it landed, and a third telling is two
  more places for the story to go stale in. Keep this section under a screen; when a phase closes,
  its paragraphs go, they do not accumulate.
