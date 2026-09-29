# Bundled services: web servers, databases, caches

**Goal**: working Caddy/Nginx, MariaDB/MySQL/PostgreSQL, Redis and Memcached immediately after
install, with sane defaults and no hand-edited config files.

## Catalogue

| Service | Default version line | Default bind | Notes |
| --- | --- | --- | --- |
| Caddy | 2.x | `127.0.0.1:80/443` | **default web server** — see ADR 0004 |
| Nginx | 1.27 stable | `127.0.0.1:80/443` | alternative front end, one active at a time |
| php-fpm | one per installed PHP, named by the full version (`php-fpm@8.3.33`) | unix socket / `127.0.0.1:9xxx` on Windows | created by `runtime.install`, removed by `runtime.uninstall`, never by `service.create` |
| MariaDB | 11.4 LTS | `127.0.0.1:3306` | random root password in OS keyring |
| MySQL | 8.4 LTS | `127.0.0.1:3306` | **a different product from MariaDB**, not a version of it: its own package, its own recipe, its own rows. Only one of the two holds 3306; the other is given a port of its own |
| PostgreSQL | 16 | `127.0.0.1:5432` | initdb on first start |
| Redis | 7.x | `127.0.0.1:6379` | **a cache, and it keeps nothing**: `save ""`, `appendonly no`, and stopped with `SHUTDOWN NOSAVE` so it does not write one on the way out either |
| Memcached | 1.6 | `127.0.0.1:11211` | 64 MB default |
| MongoDB | 8.x | `127.0.0.1:27017` | **no accounts** (T154): a bind address off loopback is refused, 27017 is never opened to a network, and WiredTiger's cache defaults to 256 MB |

**"Default" is which one this project picks, not one that arrives by itself.** Nothing installs a
front end. `service.create` refuses a *second* one by `Role::FrontEnd`, and
[ADR 0004](../decisions/0004-caddy-as-default-web-server.md) settles which of the two that role
should be when there is a choice — but a home with neither has neither until somebody runs
`mix package install caddy`, or `nginx`, which is a first-class alternative and not a lesser one. A
first run that offers to do it for them is not built and has no task of its own yet.

**Which one a home is on is answerable and changeable** — **T97**, on
[ADR 0026](../decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md). The
`services` table is the only record: the front end is the row whose recipe answers `Role::FrontEnd`,
reported as `ServiceSummary::role` and changed by `service.set_front_end`, which stops the old one,
swaps the row, renders every site for the new one and starts it. **No field is added to
`config.toml`**, because two statements of one fact would have no arbiter. The switch can end with
the home still on the server it had — on Linux the port-80 grant is written into the binary, so the
new front end needs a grant of its own — and that outcome is described rather than left as a home
whose sites are rendered for a server that cannot answer. `mix service front-end` and
`mix service set-front-end <caddy|nginx>`.

Multiple instances of the same service are supported (`mariadb@main`, `mariadb@legacy`, and
`mysql@main` beside `mysql@legacy` on the same terms) with independent ports, data dirs and
versions. Instance name is part of the `ServiceId`, and the name after the `@` is the user's: it is
what tells two of them apart, and nothing in MixEngine knows the words `main` or `legacy`. It cannot
be changed afterwards — the id is also the generated config directory, the log directory, the socket
file and the keyring address — so renaming one is creating the other and deleting this one, which
keeps the data directory.
What tells a person which program an instance is, is `ServiceSummary::version` (T183), reported
beside the id: the name is a label, the version is a fact.

**An instance moves to a newer patch of its line with `package.upgrade`** — T193c. One instance at
a time: stopped, pointed at the new version, rendered and started again, and put back on the old
version if it does not start. On Linux a front end's new binary is asked for the port grant before
anything stops. The old version is removed once every instance has moved. MySQL 8.0 cannot go back
once a newer 8.0 patch has opened its data, and the plan says so. `mix package upgrade`, and
*Update* in MixLab. Design: [T193](../specs/2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md).

**A data directory belongs to one service.** Two servers over one set of files corrupt them, and the
cost lands on the data rather than on a start that fails, so `service.create` refuses a `data_dir`
another row already holds and names who holds it. Only an explicit `--data-dir` can reach that
refusal: the derived layout is `data/<package>/<instance>`, where two instances cannot collide. The
comparison resolves a relative path against the working directory and stops there — a symlink, a
bind mount, or one directory reached through two cases on a filesystem that ignores case are the
server's own lock file to catch, not MixEngine's.

## Ports, and who gets 3306

MariaDB and MySQL name the same default, and so do two instances of either — which is one problem
and not two. A port is **allocated once, when the row is written, and never computed again**:

- **The number in the table above is a recipe's preferred port, not a reservation.** Which port a
  service would like is a fact about the service, so it is declared by the recipe beside its binary
  and its template rather than decided in `service.create`. A caller naming a port explicitly is
  taken at its word and gets no allocation at all.
- **First created, first served.** The first database to ask for 3306 is given it; the next is given
  the first free port above — `mysql@main` beside `mariadb@main` lands on 3307 by the same rule
  that puts `mariadb@legacy` there, which is the point of writing one rule rather than a special
  case for two products. The daemon reports the port it chose, because a port a person did not pick
  is one they have to be told.
- **Free means free on the machine, not free in the table.** 3306 on a developer's machine is
  routinely held by an XAMPP, by Windows' own `MySQL80` service or by a published container, none of
  which has a `services` row. So the test is a bind and not a query, and a preferred port lost to a
  program MixEngine does not manage is reported with as much of that program's identity as the OS
  will give up (T38) rather than as a silent renumbering. The search is bounded — running out of ports is an error, not a longer loop.
- **An allocated port belongs to its row for as long as the row lives.** Deleting whoever holds 3306
  does not promote anybody into it: the port is in a project's `.env` and in a colleague's shell
  history, and a service that quietly moved would break both. Moving one is a person's decision and
  a regeneration — `mix service set`, which does not exist yet and is the same missing task the
  reload waits for.
- **Allocating and writing the row are one critical section**, or two concurrent `service.create`
  calls are each handed the same next-free port and the second server fails to bind at start.

## Config generation

Every service's runtime config is **generated** into `etc/<service-id>/` from a template
(`minijinja`) plus the user's overrides stored in `services.config_overrides_json` — one directory
per `ServiceId`, which is why an instance's name is in the directory rather than under it:

```
etc/
  caddy/Caddyfile                  ← global block + one imported file per site, and one per extension
  caddy/sites/blog.test.caddy
  caddy/extensions/mailpit.caddy   ← a [[recipe.front_end]] an installed extension declared (T81c)
  nginx/nginx.conf + sites/ + extensions/
  php-fpm@8.3.33/php-fpm.conf      ← one pool per installed PHP, shared by every site on it
  mariadb@main/my.cnf
  mysql@main/my.cnf                ← a MySQL template, not MariaDB's
  postgres@main/postgresql.conf + pg_hba.conf + pg_ident.conf
  redis@main/redis.conf
  mongodb@main/mongod.conf
```

**Memcached is not in that list, and never will be.** It has no configuration file format — not one
it declines to use, one that does not exist: every setting is a command-line flag, and what
distributions call `/etc/memcached.conf` is a list of flags their init script pastes onto the command
line. So its typed overrides become the process's arguments and it is the one service with no
`etc/<service-id>/` directory at all. Rendering a file nothing reads, so that this list looks
uniform, would put a document in front of the user that changes nothing when they edit it — which is
what the next rule exists to prevent.

**One pool per PHP version and not one per site**, which is a decision T32 made rather than a
simplification: a pool per site is Unix-only vocabulary, and Windows has one master with one set of
children and no `[pool]` sections at all. Choosing it would have created exactly the split the rest
of this design avoids, in the layer Phase 4 builds on. **No `pool.d/` yet**: php-fpm reads a glob
whose directory is missing as a hard error rather than as a pattern matching nothing, and that
directory cannot exist for the first `php-fpm --test` — `include` names the installed path while
validation runs over the staged one. Phase 4 brings the directory and the `include` together.

What each service *is* — which binary, which templates, which overrides it understands, how to tell
it is up — is a **recipe** compiled into the daemon (`mixengine_core::generate::Recipe`), found by
`packages.name`. Two instances of one server are two rows against one recipe. The package index
publishes downloads and says nothing about any of this: a template changes with a MixEngine release,
not with a repackaged upstream.

Rules:

- Users edit **overrides** (typed key/value, or a free-form `extra` blob per service), never the
  generated file. The rendered result is readable back for display only, with its path, so a client
  can show it and reveal it in a folder.
- An override naming a setting the recipe does not have is **refused**, with the ones that exist in
  the message. A silently ignored key is a setting the user believes is in effect.
- Regeneration is atomic and diffed: if the rendered output is byte-identical, skip the reload.
- Reload beats restart: Caddy `caddy reload`, Nginx `nginx -s reload`, php-fpm `SIGUSR2`. Only fall
  back to restart when the change requires it (port, user, data dir). **Windows has no signal a
  daemon can send** (ADR 0008), so a pool there keeps its old configuration until somebody restarts
  it — the daemon says so in `daemon.log` rather than restarting a thing nobody asked it to restart.
- A generated config that fails validation (`caddy validate`, `nginx -t`, `postgres --check`) is
  **not** installed; the previous config stays live and the error is surfaced with the offending
  override highlighted.

## First-start initialisation

- **MariaDB**: two programs, in this order (T33). `mariadb-install-db` bootstraps
  `data/mariadb/<instance>` — a shell script on Unix and a different C++ program of the same name on
  Windows, sharing almost none of their options. It does **not** set the password: a second
  `mariadbd --bootstrap`, fed its SQL on standard input, writes the generated password into
  `mysql.global_priv`, drops the anonymous accounts and removes the test database. Bootstrap mode
  listens on no port and no socket, so there is no window in which a password-less root is
  reachable — and it implies `--skip-grant-tables`, which is why the row is written directly rather
  than with `SET PASSWORD`.
  Both programs, and the server they leave behind, keep their temporary tables in
  `data/mariadb/<instance>.tmp` rather than in the machine's `/tmp` (T33c): every MySQL-family
  server deletes every `#sql*` file in its temporary directory at start, whoever made it, so two
  sharing `/tmp` can delete each other's mid-load. The same directory, under `data/mysql/`, serves
  MySQL on all three of its routes.
  The password is generated by the daemon and stored in the OS keyring **before** either program
  runs, so a machine with no credential store fails with nothing created. There is no fallback to a
  file: it would be a plaintext credential on disk, which is the thing this arrangement exists to
  avoid (ADR 0006). A build that is not a release is the exception, and a deliberate one: it keeps
  every credential in `<root>/credentials.json` instead of the OS store (ADR 0052). A release never
  does, and refuses to be told to.
  The server's certificate is not the ritual's: it is issued at every render, from this home's
  authority, so a rotated authority reaches a running home at its next start — see
  [tls.md](tls.md#services) and T99.
- **MySQL**: the same job as MariaDB's and none of the same programs, which is why it is a recipe of
  its own (T34c). **Three bootstrap routes, chosen by version and platform** rather than by a version
  test: 5.7 and newer use `mysqld --initialize-insecure`; 5.6 on Unix uses `scripts/mysql_install_db`,
  which does not quote `$basedir` and so has to be reached through a path with no spaces; 5.6 on
  Windows has neither, and upstream's zip ships a `data/` directory with the system tables already
  built. The generated password goes on afterwards, out of the keyring, on the same terms as
  MariaDB's — stored before anything is created, no fallback to a file.
  **How it goes on is a second split, and not the same one**: 8.0 and newer take `ALTER USER` from an
  `--init-file` on a server bound to nothing, which then stops itself; 5.7 and 5.6 write the grant
  tables on standard input to `mysqld --bootstrap`, which opens no listener and exits. 5.7 is on that
  side because its `--init-file` server never stops — it runs the file, reports itself ready for
  connections and stays, so the first run spent its whole patience on a server nobody asked for.
  `--bootstrap` was deprecated at 5.7.6 and only removed at 8.0.0; reading the first date as the
  second is what had put 5.7 on the other side.
  **`--initialize-insecure` creates only `root@localhost`**, where MariaDB's installer also creates
  `root@127.0.0.1`: the `skip-name-resolve` in MariaDB's template would leave every client here
  refused by a server whose own log says it is ready for connections. Two `my.cnf` files that look
  alike are not one template.
- **PostgreSQL**: `initdb` with UTF-8 + the user's locale, `pg_hba.conf` trusting local connections
  only, create a superuser named after the OS user.
- **Redis/Memcached**: nothing at all — no bootstrap, no credential, no ritual. What they do need is
  a data directory that exists, which `Generator::render` creates for every service beside the log
  directory: Redis names its own `dir` in its own configuration and refuses the whole file when it is
  missing, and memcached, whose working directory it is, never reaches its first line.
- **MongoDB**: nothing but that same directory — `mongod` initialises an empty one itself on its
  first start (T154). **It has no accounts**, which is MongoDB's own default and Redis's
  arrangement: turning access control on means authenticating every probe and handoff through a
  client the artifact does not ship. Two locks stand in for the password, and neither depends on the
  other — the recipe refuses a bind address that is not loopback, and `mixengine-elevate` never
  opens 27017. It is ready when it prints `Waiting for connections` rather than when its port
  accepts, and on Windows a stop is a kill (ADR 0008) that WiredTiger recovers from at the next
  start. Design:
  [docs/specs/2026-09-17-t153-mongodb-is-a-service-design.md](../specs/2026-09-17-t153-mongodb-is-a-service-design.md).

Init runs inside a job with progress, and is idempotent — a half-finished data dir is detected and
cleaned rather than reused. **Two markers, and the second one is what keeps that sentence honest**:
one written before the first step and one after the last, so a directory carrying the first alone is
ours to clear and a directory with contents and *neither* is somebody else's database. The second is
refused and left exactly as it was. The started marker sits beside the data directory rather than
inside it, because Windows' `mariadb-install-db` refuses any datadir that is not empty.

### Re-setting a credential a home cannot produce any more

A database keeps its own copy of its superuser password inside its data directory and the OS
credential store holds the other. The first run writes both, and once they come apart nothing can
log in to bring them back together: every way of changing the copy inside the directory needs the
password that was lost. `mix service reset-credential <service>` — `service.reset_credential` —
is the way out, and T127 is where it was added.

**It is the ritual's own password step and nothing else.** A ritual is *create the directory, then
set the password through a server that listens on nothing*; a repair is the second half, against a
directory that is already full. Each recipe declares it beside its ritual, because the mechanisms
are genuinely different — MariaDB writes the grant tables in `--bootstrap`, MySQL runs `ALTER USER`
through an `--init-file` on a server bound to nothing from 8.0 and writes the grant tables in
`--bootstrap` before it, PostgreSQL uses `postgres --single`.

**The two operations refuse each other's cases**, and that is what keeps a repair from becoming a
data-loss report. A first run passes over a `Ready` directory without touching it, which is why
restarting a service never repairs a drifted password. A reset refuses `Empty` and `Unfinished` by
naming the command that performs a first run, refuses a `Foreign` directory exactly as a first run
does, and never clears anything — deleting a data directory happens on one path only.

**It is never automatic.** *A keyring entry is the deed of ownership* is wrong in precisely this
case: the entry this home holds may be the one another home overwrote. So the repair is irreversible
work done on the strength of a claim known to be unreliable here, and it happens only when somebody
types the command.

**What proves it worked is the service starting**, not an exit code. `postgres --single` exits 0 on
a syntax error; the readiness check of two of these recipes is an authenticated query, so the service
reaching `running` is the password being accepted. MariaDB's is `mariadb-admin ping`, which answers
before authentication: there the restart proves the server starts on the directory the repair wrote
into, and the credential itself is proved by the next statement run against it. Either way the repair
ends with the named service running and the dependents the stop took down started again. Where a step
fails, nothing is started: a database whose credential is half re-set is not one to put back in front
of an application.

**And a start that meets the same drift now says so** — roadmap task **T127a**. That asymmetry has a
second consequence. A MariaDB instance whose password has drifted reaches `running` and meets the
refusal at its first statement, where it is explained; a PostgreSQL one never leaves `starting`, and
was reported as a ready check that timed out — two minutes of waiting and then a hint pointing at a
log. A failed start now reads the lines the service printed before it decides what to call the
failure, so a refusal naming that instance's own superuser is `superuser_refused`, carrying the
server's own line and naming this repair, rather than `ready_timeout`. It is read only for a service
that has a database vocabulary, and only after the start has already failed: a reader that acted on a
log line while a start was still running could take down a cluster that an application was merely
polling with a stale password of its own.

## Data an earlier install left

A data directory under `data/` with no service row — an uninstall that kept the folders `[paths]`
moved, then a fresh install pointed at them — is listed by `service.found` (`mix service found`,
MixLab's Dashboard) and brought back by `service.adopt` (`mix service adopt`, **Adopt**). Adopting is
`service.create` with the same id, whose derived data directory is the one found, and for a database
`service.reset_credential`, which sets a new admin password. The service is left stopped; the
databases and accounts inside are kept, and an application's own account keeps the password the
application holds. Which installed version may open the data is read from `.mixengine-ready`. Design:
[T182f–h](../specs/2026-09-27-t182f-a-reinstall-finds-what-the-last-one-kept-design.md), D4.

## Web server integration

- Exactly one of Caddy/Nginx is the active front end (owns 80/443). Switching regenerates all site
  configs and hands the ports over.
- Sites map to a generated per-site config file; there is no shared file that all sites append to,
  so one site's configuration is a file somebody can read on its own. **The whole set is judged
  together, and a refusal installs nothing** — T43, D3. `SiteState` has two words on purpose, and a
  site carries no free text today: its domains are normalised, its doc root is refused if it resolves
  outside the project, its upstream is checked, and its pool must exist. A rendered site file the
  front end's own checker refuses is therefore a bug in this repository's template, not a mistake a
  user made — and skipping it would hide the bug while serving eleven sites out of twelve. So the
  front end goes on reading the configuration that worked and the error names the file the checker
  complained about. A `Degraded` site becomes worth having when a site can carry a snippet somebody
  wrote, which is the extension surface's ([extensions.md](extensions.md)) to introduce.
- **A front end sweeps two directories, not one** — `sites/` and `extensions/` — because their
  contents follow two different tables and a sweep that could not tell them apart would be one table
  deleting the other's files (**T81c**). An extension's fragment is text MixEngine did not write, so
  unlike a site file a refusal there *is* a mistake a user can make: it is judged by the front end's
  own checker when the extension is installed, before anything is downloaded, and refused there.
- The front end **answers** on 80 and 443 on every system, and **binds** 80 and 443 on Windows and
  Linux and 8080 and 8443 on macOS. Which of those a program must listen on is not a `#[cfg]`
  anywhere above the platform layer: it is what `Host::port_access().probe(…)` returns, one
  `PortBinding` per port, and **T43** is what renders it into a front end's configuration.
- Neither is bound by an elevated process. Windows reserves nothing below 1024; Linux is granted
  `cap_net_bind_service` on the front end's binary; macOS gets a packet-filter redirect plus the
  boot-time job that enables pf ([ADR 0012](../decisions/0012-a-boot-time-job-enables-the-packet-filter-on-macos.md)) — and the grant runs that job's
  command itself before it returns, because a machine granted the redirect today has not rebooted
  and three files on disk leave the running pf exactly as it was. All of it is arranged by a
  one-time `PortAccessGrant` — see
  [../decisions/0005-on-demand-elevation.md](../decisions/0005-on-demand-elevation.md). If a port is taken by another
  program, report `port_in_use` **with the owning process name** — the platform layer's `PortOwner`
  is what answers that, and how much of an answer there is depends on the OS: the program's name
  where this account may read it, the pid alone where the OS refuses the name, and "another program
  on this machine" where a socket cannot be traced to a process at all. All three are better than
  the symptom, and none of them is the same answer as *nothing is listening*.

## Database management scope

MixEngine manages *lifecycle* (install, start, stop, config, credentials, data dir, backup/restore
snapshots of the data dir). Browsing and querying data is **out of scope** — that is
MixLab's `db` module ([extensions.md](extensions.md)).

**Making a database is part of that lifecycle, and "credentials" is what makes it one.**
`database.create` — `mix database create mariadb@main --name blog` — creates a database and an
account that reaches it on a running instance, generating the account's password and storing it in
the OS keyring at `<service-id>/<user>`. Nothing prints it or puts it on the wire *by default*: what
a caller is told is the address, and handing a credential to a program that needs one is
`database.open` — `mix database open mariadb@main --user blog` — which starts MixLab
with the password in that process's environment alone (T83, [extensions.md](extensions.md)). The
headless archive has no window, and says so (T165).

**A person can still read it, and can still choose it** — T77b. Neither of the two calls above
reaches the one case they leave uncovered: a project's own `.env`. `mix database credentials
mariadb@main --user blog` prints the password MixEngine holds, and `mix database create mariadb@main
--name blog --user blog --password` lets a person choose it instead of letting MixEngine generate
one — through the same ownership rule the paragraph below states, so a correct password for an
account MixEngine holds no keyring entry for is still refused: knowing a password is not the deed.
Design, and the rule for when a credential is allowed on the wire at all:
[docs/specs/2026-09-06-t77b-a-password-a-person-can-read-and-choose-design.md](../specs/2026-09-06-t77b-a-password-a-person-can-read-and-choose-design.md),
[ADR 0025](../decisions/0025-a-credential-is-answered-only-by-a-method-that-exists-to-answer-it.md).

Two rules make it repeatable and safe to run twice. **A keyring entry is the deed of ownership**: an
account already on the server that MixEngine holds no credential for is refused by name rather than
having its password reset, because "make sure this account exists" must not mean taking over
somebody else's — and a password a person supplies that happens to be correct does not change that
refusal. And the last statement **logs in as the account just made** and creates a table with it, so
the call cannot report a success the account cannot use — on PostgreSQL that account *owns* its
database, since `GRANT ALL ON DATABASE` has not carried `CREATE` on `public` since version 15.

There is no `database.drop`. Removing a database destroys data, and nothing has asked for it — see
[blueprints.md](blueprints.md) for what a blueprint rollback does instead.

## The commands a package brings with it

**An installed database puts its clients on the PATH** — roadmap task **T130**,
[ADR 0033](../decisions/0033-bin-is-a-projection-of-what-is-installed.md). `<root>/bin` fronts
`mysqldump`, `psql`, `redis-cli` and their siblings for whatever this home has installed, so a
terminal can open a database without knowing where MixEngine unpacked it.

Each recipe **declares** which of its programs are commands rather than the list being derived from
the artifact's `provides`, and the difference matters twice: nginx's map holds `mime.types` beside
`nginx.exe`, and every supervised server — `mariadbd`, `postgres`, `redis-server` — plus every
bootstrapper — `initdb`, `pg_ctl`, `mariadb-install-db` — stays out. A shim in front of one of those
would be a second way to start or overwrite something nothing is watching. They stay reachable at
their full path.

**A client belongs to an instance, not to a version.** A runtime resolves per directory; a database
cannot, because it has instances with their own versions, data directories and ports. So the
instance decides both which install the program comes out of and, through the client family's own
variable, where it connects:

```
mariadb / mysql   MYSQL_HOST   MYSQL_TCP_PORT
postgres          PGHOST       PGPORT
redis, memcached  (those clients read no environment; use -p)
```

That second half is not a nicety. *Ports, and who gets 3306* above gives 3306 to whichever of
MariaDB and MySQL was created first and the next free port to the other, so on a home with both, a
bare `mysql` told nothing would open a session on the other product's server and report success. A
variable the person already exported is never overwritten, and `MIXENGINE_MARIADB=mariadb@legacy`
names an instance outright.

**MariaDB also answers to `mysql`, `mysqladmin` and `mysqldump`**, because its own archive publishes
`mariadb-dump` while every tutorial and script says `mysqldump` — and it drops all three the moment
the MySQL package is installed beside it. A real name always beats a spelling; where two packages
publish the same real name, the one with an instance wins, then the one on the product's documented
port, then the package name ascending. `mix path status` and `mix doctor` both say who won.

A home with the package installed and **no instance** still gets the clients, out of the newest
version installed and told nothing about an endpoint: `mysqldump -h db.example.com` is a real use of
one.

## Acceptance criteria

- `mix service start caddy mariadb redis` → all three healthy in under 10 s, **warm**: installed,
  bootstrapped, and started at least once before. Measured by
  `crates/mixengine-cli/tests/warm_start.rs` in the `bench` job, on all three systems.

  **Warm and *fresh install* are two different runs**, and this line used to say both. A fresh
  install has an empty data directory, so its first start is MariaDB's first-run ritual —
  `mariadb-install-db` building a system schema, a generated root password reaching the credential
  store — which is tens of seconds of work by design. That number is measured and reported beside
  this one and is held to nothing: nobody has said what it should be, and a budget nobody argued
  for is a budget that gets turned off rather than met.
- Breaking an override produces a clear validation error and does **not** interrupt running traffic.
- Two MariaDB instances of different versions run simultaneously with separate data dirs.
- MariaDB and MySQL run side by side, each bootstrapped by its own programs, neither reading the
  other's generated `my.cnf`.
