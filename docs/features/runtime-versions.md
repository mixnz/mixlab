# Multi-version runtime management

**Goal**: install many versions of PHP, Node.js, Python, Ruby, Go and Java side by side, switch
instantly, and have each project use the right one without the user thinking about it.

## Model

- A **runtime install** is an immutable directory `runtimes/<kind>/<version>/`. Installing never
  mutates an existing version. **One file is the exception**, and only on a JDK: the daemon writes
  this home's authority into `lib/security/cacerts` with that JDK's own `keytool` — T27e,
  [ADR 0039](../decisions/0039-a-jdk-is-told-about-the-authority-inside-its-own-cacerts.md).
- A **runtime kind** has one *global default* and any number of *project pins*.
- PHP is special: each installed PHP version also owns a long-running `php-fpm@<version>` service
  ([services.md](services.md)). Node/Python/Ruby/Go/Java are invoked per-command, not supervised.
- **Go** — T27d. Upstream's whole tree under `runtimes/go/<version>/`, fronted by `go` and `gofmt`,
  smoke-tested with `go version`. `GOROOT` is wherever the tree is, derived by `go` itself, and
  `GOPATH`, `GOMODCACHE`, `GOCACHE` and `GOBIN` stay Go's own defaults. What `go install` writes into
  `GOBIN` is not fronted: that directory is outside every install and shared by all of them. Design:
  [docs/specs/2026-09-17-t27d-go-runtime-design.md](../specs/2026-09-17-t27d-go-runtime-design.md).
- **Java** — T27e. A Microsoft Build of OpenJDK under `runtimes/java/<version>/` (the LTS lines 11,
  17, 21 and 25), fronted by `java`, `javac`, `jar`, `jshell`, `keytool` and `jlink`, smoke-tested
  with `java --version` — run without `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS`, `JDK_JAVA_OPTIONS`,
  `CLASSPATH` or `JAVA_HOME`, so a daemon started from a session carrying one of them does not refuse
  every JDK for a reason that has nothing to do with the JDK. No globals directory: Maven and Gradle
  keep their programs outside every install. Design:
  [docs/specs/2026-09-18-t27e-java-runtime-design.md](../specs/2026-09-18-t27e-java-runtime-design.md).
- **Composer is a kind, not a language** — T27c. It installs, pins, lists and defaults like the
  languages above, into `runtimes/composer/<version>/composer.phar`, and the `composer` shim runs
  that file under the PHP the same directory resolves to. Composer 2.3+ needs PHP 7.2.5 or newer; a
  project on an older PHP pins `composer = "2.2"`. No service, no smoke test at install (nothing
  starts on its own), no `conf.d`. Design:
  [docs/specs/2026-09-08-t27c-composer-through-the-runtime-pipeline-design.md](../specs/2026-09-08-t27c-composer-through-the-runtime-pipeline-design.md).

## Version resolution

One function, `core::resolve::runtime`, used by the shims, the daemon and every client — and
`runtime.resolve` over the API for the clients already talking to a daemon. Order:

1. Explicit flag / env (`MIXENGINE_PHP=8.1`), read by the process the user invoked and passed in —
   never read by the daemon, whose environment is whatever started it
2. `mixengine.toml` found by walking up from cwd. The first one **that names this language** wins: a
   manifest silent about PHP is not an answer about PHP, so an outer pin still applies
3. Project record in SQLite matching the cwd or a directory above it (a registered project root)
4. Global default
5. Error `dependency_missing` with a hint naming the exact `mix runtime install` command, or — for a
   range, whose satisfying version is not knowable from here — `mix runtime available`

Constraint strings accept a **prefix** (`8`, `8.3`, `8.3.12` — as many segments as are written have
to agree, and one nobody wrote is a zero) and a **caret** (`^8.3`, up to the leftmost non-zero
segment: `^0.12` stops at `0.13`). A constraint with no pre-release in it never selects one — `8.5`
and `^8.5` both pass over `8.5.0RC1`, and naming it (`8.5.0RC1`) is how it is asked for. Everything
is resolved against installed versions — **never** silently against downloadable ones.

## Shims

`<root>/bin/` contains a small shim binary per exposed command of each **installed** kind (`php`,
`php-config`, `pecl`, `composer`, `node`, `npm`, `npx`, `python`, `pip`, `ruby`, `gem`, `bundle`,
`go`, `gofmt`, `java`, `javac`, `jar`, `jshell`, `keytool`, `jlink`) — T185b, ADR 0057. No version of a
kind installed means no command of it in `bin/`, a tool installed into a runtime included. The shim:

1. Reads its own file name to know which command was invoked.
2. Calls `resolve` (in-process, reading SQLite read-only + walking for `mixengine.toml`) — **no IPC**,
   so it stays fast even when the daemon is down. Target: **< 15 ms** overhead, enforced by a bench.
3. `exec`s the real binary with the correct `PATH`, `PHPRC`, `GEM_HOME`, etc. prepended.
   On Windows there is no `exec`, and what is in `bin/` is not the shim but a **trampoline**
   (`mixengine-trampoline`, T185): it runs `mixengine-shim` — found through
   `bin/mixengine-shim.path` — for the resolution only, then starts the program itself in a Job
   Object, proxying the exit code and console signals. The shim's resolution is unchanged; only the
   few hundred KB that outlive it are copied per name.
4. **A row may name a `via` kind** (T27c): `composer` resolves a Composer for the file and a PHP for
   the program — each under its own override variable — and hands the PHP `composer.phar` as its
   first argument, with the PHP's own environment.

Only `<root>/bin` goes on the user's PATH — one entry, never per-version directories. The directory
is filled by the daemon at every start — a hard link to
the shim binary wherever the filesystem gives one file a second name, and a copy of its bytes where
it does not, which on Windows is always: a shim there outlives the program it starts, so a link would
let a running `php -S` hold the shim binary itself open against the next upgrade. On Windows those
copies are of the trampoline (~360 KB) rather than the shim (~5.6 MB), which took a 38-command
`bin/` from about 223 MB to about 14 MB (T185); an install that updated itself onto that release
gets the trampoline at its first start (T185a). Either way the file in `bin/` dispatches on the name
it was invoked by. Putting the directory on the PATH is
`path.install`, which is asked for rather than assumed, and
`path.uninstall` reverses it.

**`bin/` is a projection of what is installed** — [ADR 0033](../decisions/0033-bin-is-a-projection-of-what-is-installed.md),
roadmap tasks **T130** and **T131**. Three sources compose it, and the compiled table stays first:

1. `core::shims::COMMANDS`, which is a constant and does **not** depend on what is installed — a
   `node` shim on a machine with no Node.js resolves nothing and says which command to type, which
   is a better answer than `node: command not found` from a tool whose job is managing versions of
   Node.
2. The **client commands of installed service packages** — `mysqldump`, `psql`, `redis-cli`. See
   [services.md](services.md).
3. The **tools somebody installed into a runtime**, below.

`path.status` says which of the three put each name there, and `mix doctor` names a discovered
command that comes before a program already on the PATH.

### A tool installed into a runtime

`npm install -g yarn`, `pip install poetry`, `gem install rails`. Each package manager writes into a
**bindir inside the runtime's own install** — measured rather than assumed: `npm config get prefix`
inside a MixEngine Node answers the install directory itself on Windows and its `bin` on Unix — and
no part of that is ever on anybody's PATH.

So the daemon looks for them and fronts what it finds. A scan of each installed runtime's bindir
records the name and **which language it belongs to** (`bin_commands`), and `bin/` gains a shim for
each. What it refuses: a name `COMMANDS` already holds, a program the artifact itself publishes,
MixEngine's own binaries, and a file this system does not execute.

**A discovered tool follows the version, exactly as `npm` does.** `yarn` is resolved for the
directory it was typed in and looked for inside *that* version's bindir — so a project pinned to
another Node gets that Node's Yarn, or a sentence naming the version and `npm install -g yarn`. A
PATH entry pointing at one install could never have done that, which is why there is not one.

The daemon watches each installed runtime's bindir and runs the pass when one gains or loses an
entry, so nothing runs on an idle machine. A start, and every runtime installed, removed, adopted
or restored, runs it before answering. `mix path rescan` runs it now. Composer's own global
bindir is out of scope: it is `~/.composer/vendor/bin`, outside every install directory.

### What a shim tells the program it becomes

`PATH` with the runtime's own directory first, the generated ini set (`PHP_INI_SCAN_DIR`, T28), and
**where this machine's certificate authorities are** — roadmap task **T133**,
[ADR 0034](../decisions/0034-mixengines-authority-reaches-a-runtime-through-a-generated-bundle.md):

| kind | variable | file |
| --- | --- | --- |
| Node | `NODE_EXTRA_CA_CERTS` | `certs/ca/root.crt` |
| Python | `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE` | `etc/ca/bundle.pem` |
| Ruby | `SSL_CERT_FILE` | `etc/ca/bundle.pem` |
| PHP, Composer | — | the generated ini set says it instead |
| Go | — | Go already reads the store this home's authority is installed into |
| Java | — | its own `cacerts`, written by the daemon with that JDK's `keytool` (T27e, ADR 0039) |

Node is handed the authority and the others the merged bundle, because `NODE_EXTRA_CA_CERTS` *adds*
to what Node trusts and every other mechanism **replaces** a trust store. See [tls.md](tls.md).
Nothing is exported for a file that is not there, and a variable the person already set is never
overwritten. Go needs no row: its `crypto/x509` verifies through CryptoAPI on Windows and
`SecTrustEvaluateWithError` on macOS, and on Linux reads `/etc/ssl/certs/ca-certificates.crt` or
`/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem` — the two bundles `update-ca-certificates` and
`update-ca-trust` regenerate after MixEngine's anchor lands.

**And a `go` is kept to the release it resolved to** — T27d. The archive's `go.env` says
`GOTOOLCHAIN=auto`, which lets a `go.mod` asking for a newer Go download that Go and run it instead,
so the shim hands a `go` **`GOTOOLCHAIN=local`**: a module that needs more than the pinned release
fails with Go's own sentence naming the setting. A non-empty `GOTOOLCHAIN` the session exported is
left alone, an empty one is treated as unset (Go does the same), and one written with `go env -w`
loses on purpose — the environment beats that file, and it is usually a machine-wide choice made
before MixEngine was installed. `mix doctor` notes a daemon whose own environment carries a
`GOTOOLCHAIN` other than `local`, or a `GOROOT`, since its children inherit either.

**And a Java command is told its own `JAVA_HOME`** — T27e. The shim sets it to two directories above
`provides.java` — `Contents/Home` on macOS — **over** whatever the session carries. That is the
opposite of the rule above, for T27d's `go env -w` reason: `JAVA_HOME` is usually a machine-wide
value an installer wrote before MixEngine was here, and a `java` whose own children are told about
another JDK is half a pin. The limit is stated rather than hidden: a shim reaches only what it
starts, so `mvn` or `./gradlew` typed in a terminal still reads the session's `JAVA_HOME`. `mix
doctor` notes a daemon environment carrying one outside `runtimes/java/`, or a `JAVA_TOOL_OPTIONS`,
`_JAVA_OPTIONS` or `JDK_JAVA_OPTIONS` naming `javax.net.ssl.trustStore` — that store replaces the
`cacerts` the authority was written into.

**What a Linux JDK links is warned about and never refused** — T27e. The index's optional
`requires.libraries` names the sonames a build links and does not ship: every Linux JDK's `libz`,
`freetype`, X11 and ALSA. On Linux the daemon reads `ldconfig -p` for this architecture, and each
soname it does not list becomes a `Need::SharedLibrary` whose remedy is `InstallFromDistribution` —
it blocks nothing, needs no consent, and never hides a release from `ChooseVersion`, because every
Linux release of a line links the same set. `mix` prints a `warning:`, the install job says the same
in its progress, and MixLab shows a notice and installs. An `ldconfig` that cannot be run, or a cache
that lists nothing, is no answer rather than everything missing: a headless server runs a JDK without
X11 or sound, and `libz` — the one nothing starts without — is what the smoke test catches anyway.

## Install flow

`runtime.install { kind, version }` returns a job:

1. Look up the artifact in the signed package index for `(kind, version, os, arch)`.
2. Download to `<root>/cache/downloads` with resume support, verify SHA-256, verify index signature.
   Not `run/`, which is scratch belonging to the daemon currently running: a partial download's whole
   value is surviving a restart, and it is named after the artifact's hash so the same file offered
   by a mirror and by the default host resumes one download rather than starting two.
3. Extract to a staging dir, write `.mixengine-install.json` into it (the row about to be recorded,
   T182f), then atomic-rename into `runtimes/<kind>/<version>/`.
4. Post-install hook (per kind): PHP — write the base `php.ini` from our template and create the
   `php-fpm@<version>` service record. **The service half landed with T32 and is written differently
   from what this step implies**: it is not a PHP-shaped branch in the installer but a walk over the
   recipes whose `Recipe::source` names a runtime, and it is *idempotent and also run at boot* — so a
   PHP installed by an earlier build gets its pool with no data migration, and a home whose row was
   deleted by hand repairs itself. The `php.ini` half landed with T28 and
   became a `conf.d` instead — there is no generated `php.ini` at all, see *PHP extensions* below. **Node, Python and Ruby — nothing**, which is not what this
   step originally said. It reserved *ensure `pip`* and *ensure `bundler`*, and T27 found that both
   belong in the recipe rather than here: the only artifact missing a runnable entry point is the
   Windows CPython, and generating one at install time bakes the install directory into a launcher
   that stops working the moment `<root>` moves. `tools/python.py` writes a wrapper that computes the
   interpreter from its own location instead. **A path computed at run time beats a path written at
   install time** — the same rule the shim itself is an instance of — so the hook that would have
   fixed this would have been the bug.
5. Record in `runtime_installs`, emit events, **refresh `bin/`** — T185b, ADR 0057: a runtime's
   commands are in `bin/` only while one of its versions is installed, so the first install of a
   kind adds its names and the last uninstall removes them.

Failures roll back the staging directory. A half-extracted version must never appear in `list`.

**A directory without a row** — T182f. A home that lost its database (an uninstall that kept the
folders `[paths]` moved elsewhere, then a fresh install pointed at them) finds versions on disk it
has no row for. Each daemon start records them: by the marker every install leaves, and for an
install older than the marker by the package index, once every file the build provides is there and
the smoke test passes. An install that finds its directory already there records it instead of
refusing when the marker names that build. What cannot be checked is named by `mix doctor` and left
alone; `mix runtime adopt` and `mix package adopt` record one by hand. Design:
[T182f](../specs/2026-09-27-t182f-a-reinstall-finds-what-the-last-one-kept-design.md).

**What the machine lacks is read first** — phase 18. `runtime.requirements` answers what a version
lacks here and what can be done about each: the Microsoft Visual C++ Redistributable can be installed
(`install_prerequisites`, after a person agrees — ADR 0037); a glibc or macOS that is too old names
the newest release that runs. `runtime.install` refuses before its job exists when the machine
certainly lacks something and nothing was agreed; `ignore_requirements` skips the judgement, and the
smoke test still runs. `package.*` has the same two methods and flags.

See [operations/runtime-packaging.md](../operations/runtime-packaging.md) for where the binaries come
from on each OS.

## Lines and updates

**A line** is the part of a version a person chooses by: `8.4` for PHP, `22` for Node.js and Java,
major.minor for Python, Ruby, Go and Composer — T193a. `mixengine_core::lines` is the only place it
is decided, and it is never wider than a database's data series. `runtime.list_available` marks
each release's `line` and whether it is the `newest_in_line`, and lists `updates`: each installed
version whose line has a newer release here. `mix runtime available` prints one row per line
(`--all`, `--line`), and MixLab one row per line with the rest behind *N more*.

**An update stays within a line** — T193b. `runtime.upgrade_plan` says what would move;
`runtime.upgrade` installs the new release, carries the extension choices and the pool's
settings, starts the new pool if the old one ran, moves sites, `web-app` pools whose `requires`
it satisfies, the default and exact SQLite pins in one transaction, renders the sites (reversing
the move if the front end refuses them), stops the old pool and removes the old version, unless
`keep`, a `mixengine.toml` pin, a tool installed into it or a `web-app` still needs it. Moving
between lines is refused: that is a switch. `mix runtime upgrade`, and *Update* on the installed
row in MixLab. Design: [T193](../specs/2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md).

## What a release lacks, and Ruby's devkit on Windows

**An index entry may say what its cell cannot do that the other cells of the same version can** —
roadmap task **T206** ([design](../specs/2026-10-08-t206-ruby-on-windows-builds-native-gems-design.md)).
The packaging repository writes it as `lacks`, a map from a capability to the reason, and the only
release carrying one today is Ruby on Windows: RubyInstaller's build has no YJIT and no compiler, so
`gem install` of a gem with a C extension answers `MSYS2 could not be found`. MixEngine reads the
field and shows it, and judges none of it: `runtime.list_available` carries it on each release,
`mix runtime available` draws a `LACKS` column when a row has one, `mix runtime install` ends with a
line when the release cannot build native gems, and MixLab's Languages tab marks the row.

**The compiler is a package of its own, `msys2`** (T206a): MSYS2 with the toolchain RubyInstaller's
`ridk install 3` would add, built and smoke-tested by the packaging repository for Windows x86_64
(UCRT gcc) and Windows ARM64 (clang). It is a *toolchain*: `package.install` installs it, lists it
and removes it, and `service.create` refuses it, because nothing in it runs. **The shim points every
Ruby command at the newest installed one through `MSYS2_PATH`**, the first place RubyInstaller looks
(`ruby_installer/runtime/msys2_installation.rb`), so one install serves every Ruby version and no Ruby
directory is written to. A session that already exports `MSYS2_PATH` keeps its own value, on
`GOTOOLCHAIN`'s rule. Nothing is set for any other language, or where no `msys2` is installed.

**What a gem's MSYS2 packages bring** (T206b, re-measured on 2026-10-09). A gem that declares
`msys2_mingw_dependencies`, such as Rails 8's `ruby-vips`, makes RubyInstaller run `pacman -S` inside
the devkit. pacman installs the package and its dependencies and none of its optional ones, so
libvips loads without its heif, jxl and magick modules until they are added with `usr/bin/pacman -S`
from the devkit's folder. The devkit's package database is the one of its build date, and that
costs nothing measurable: every file a 2026-10-08 database named still downloaded a day later, from
`mirror.msys2.org` and from `repo.msys2.org`. If a database ever grows old enough for its files to be
gone, the answer is a newer `msys2` from `mix package available`, never a `pacman -Sy` inside this
one, which is the partial upgrade MSYS2 does not support. See
[msys2.md](https://github.com/mixnz/mixengine-packages/blob/master/docs/packages/msys2.md).

## An install whose folder is gone

**A runtime or package whose folder was deleted by hand reads as `missing`** — roadmap task
**T206d** ([design](../specs/2026-10-09-t206b-t206d-installs-whose-folder-is-gone-design.md)). Nothing
is stored for it: every read of `runtime_installs` and `packages` asks the disk, and only "not found"
counts, because a folder out of reach (no access, a drive not mounted) is not a folder gone. The row
stays until `mix runtime uninstall` or `mix package uninstall` forgets it.

- **What runs skips it.** Under a constraint, the newest version that is there answers; when only a
  missing version matches, or the default is missing, `php` and every other shim fail with
  `RuntimeMissing`, naming the install that puts it back and the uninstall that forgets it. A
  default is never replaced by another version behind a project's back. The Ruby shim points
  `MSYS2_PATH` only at a devkit that is there.
- **The catalogue offers it again.** `list_available` marks that version `installed: false` and
  bases no update on it. **Installing the same version restores it in place**: the row keeps its id,
  so a runtime stays the default and a package keeps its services.
- `mix runtime list` and `mix package list` read `missing` in the `INSTALLED` column, `mix doctor`
  reports `InstallMissing` (left alone by `mix doctor --repair`), and MixLab offers *Reinstall*.
- **Uninstalling a version removes its kind's folder once it is empty** (T206c), so the last
  `msys2` leaves no `packages/msys2/` behind.

## PHP extensions

Per-version, since that is how PHP works. **Landed with T28**, and three things about it are written
differently from what this section originally said:

- `mix runtime ext list|enable|disable <name> --php 8.3`, and **not** `mix php ext …`. A per-language
  command family for one language is a noun this CLI would then owe every other runtime; `runtime` is
  where the version already lives.
- **No `install <name>`.** What can be switched on is what the archive already ships — which is 31
  modules on the Windows build and everything the Unix build compiles in. An extension from anywhere
  else is a `mixengine-packages` task before it is one here, and the state model does not change when
  one arrives: it becomes another name in the artifact's `shared` list.
- Enabling writes `etc/php/<version>/conf.d/<NN>-<name>.ini` and reloads only that php-fpm pool.
  **Under `etc/` and not inside the install**: an install is a rename of a staging directory over the
  destination, so a generated `conf.d` living inside it is destroyed by reinstalling the same version
  — and generated configuration is disposable by the project's own rule. Both consumers find it
  through `PHP_INI_SCAN_DIR`, set by the pool's spec and by the shim, so `php -m` on a terminal and
  `phpinfo()` in a browser answer the same thing.
- Extensions are per-version toggles, with the "requires restart" state made obvious —
  `runtime.set_extension` answers `reloaded`, `restart_required` or `pool_not_running`, so no client
  has to guess it from the operating system it happens to be running on.

## Uninstall

Refuses if a project pins it or a site uses its php-fpm service, listing what blocks it, unless
`--force`. Removes the directory, service record, and any orphaned pool config.

**Both refusals are in**: the running php-fpm pool as of **T32**, by name and with
`mix service stop <pool>` in the hint; the project pin as of **T39**, naming each project and the
constraint it asks for. An uninstall that is allowed removes the `services` row before the
directory.

`--force` crosses **the pin and nothing else**. A broken pin is a statement about the future — the
next `cd` into that directory fails with a message naming the install that fixes it — and somebody
who has been shown the projects is entitled to decide; a running pool is a process serving requests
now, and no flag buys a live process with no files under it.

The pin is read in **effective** order, so a row the project's `mixengine.toml` overrides refuses
nothing, and a pin nothing already satisfies refuses nothing either: what earns the refusal is the
transition from *answered* to *unanswerable*. Still open: removing an orphaned `etc/<pool>/`, which
is the same orphan-removal question T43 owns for site files.

A *site* using the pool is T39a's half of the sentence above, and is not checked yet.

## Acceptance criteria

- Two PHP versions serving two sites simultaneously, verified by `phpinfo()` in an integration test.
- `cd project-a && php -v` and `cd project-b && php -v` disagree, with no shell hook installed.
- Uninstalling the default version leaves the system in a coherent state (new default chosen or
  cleared, with a warning), never a dangling shim.
