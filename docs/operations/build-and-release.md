# Build, CI and release

## Local development

```bash
cargo check --workspace --all-targets        # fastest loop
cargo clippy --workspace -- -D warnings
cargo test --workspace                        # unit + component + integration
cargo run -p mixengine-daemon -- --log-level debug   # foreground; --detach backgrounds it
cargo run -p mixengine-cli -- status
bash scripts/clean-targets.sh                 # both target/ directories — `cargo clean` reaches one
```

Rust at the root; the one Node toolchain is `apps/desktop/`, the desktop application, whose Cargo
workspace is excluded from this one
([ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md)). Its own loop is
`npm ci && npm run build && npm test && npm run lint` there, and
`cargo clippy --locked --all-targets -- -D warnings` in `apps/desktop/src-tauri`.

**`npm run dev:app` builds the daemon too** — roadmap task **T111**. The window looks for
`mixengined` beside itself first ([T107](../specs/2026-09-09-t107-where-the-daemon-and-the-window-are-design.md)),
and `tauri dev` starts it out of `apps/desktop/src-tauri/target/debug/`, where nothing else ever put
a daemon — so the MixEngine tab either showed *not installed*, or, on a machine with a release
installed, found **that** daemon at the second step and started it against the release home while
the window kept dialling `MixEngine-dev`. `apps/desktop/scripts/stage-daemon.mjs` builds the four
headless crates at the root, copies them there, and then starts `tauri dev` itself with
`MIXENGINE_DEV_HOME` pointed at `.mixengine-home` — the same suggestion `.cargo/config.toml` gives
`cargo run -p mixengine-daemon`, so a terminal and the window see one daemon. It is a suggestion and
not a home: the binaries take it unless an elevated helper could not read it there, and on macOS a
checkout under `/Volumes/` gets `MixEngine-dev` instead
([ADR 0040](../decisions/0040-a-development-builds-home-follows-its-checkout.md)). It starts the window
rather than sitting in front of it behind `&&` because that environment has to reach it. The list it
stages is `packaging/common.sh`'s `MIX_BINARIES` minus the window; it carries none of its own. After a
build that succeeded it **always** runs `mix daemon stop` against the same home, quietly when nothing
is running: the daemon outlives its window, and macOS — unlike Windows and Linux — lets a running
executable be replaced, so a stop that waited for the copy to be refused never happened there and
the window went on talking to the previous build. A copy still refused after the stop (a daemon of
another home running the same binary) is reported as such, rather than as os error 5.

**A development build keeps its credentials in its home too**, in `credentials.json` instead of the
Keychain, Credential Manager or Secret Service
([ADR 0052](../decisions/0052-a-build-that-is-not-a-release-keeps-its-own-credentials.md)). An unsigned
daemon is a stranger to the Keychain after every rebuild, and a file never has to ask anybody. The
cost is the hand-off: MixLab reads a managed database's password from the
machine's store, so to work on that path start the daemon with `--credential-store os`
(`MIXENGINE_CREDENTIAL_STORE=os`).

Environment knobs: `MIXENGINE_HOME` (isolated sandbox root — always set this when experimenting),
`MIXENGINE_LOG_FORMAT=json`, `MIXENGINE_SYSTEM_TESTS=1`, and the pair `MIXENGINE_INDEX_URL` +
`MIXENGINE_INDEX_KEY` (`--index-url` / `--index-key`), which point `mixengined` at another package
index. Only together: the signature requirement stays, and nobody but us can sign with the key
compiled in — so a URL that moved while the key did not would be a setting that can only ever fail.

**`MIXENGINE_ALLOW_MISSING_PACKAGES=1` is for one script and one situation.**
`.github/scripts/test-no-network.sh` refuses to run at all when a real-server package it expects is
not unpacked, because a suite that quietly does not run is a green tick over nothing. Set this to run
that script by hand without the archives, and the refusals go back to being warnings. Nothing in CI
sets it, and the day something does is the day the Linux leg stops meaning what it says.

### After changing a type in `mixengine-proto`

The published TypeScript contract is generated from that crate and **committed**, so a type added,
removed or reshaped there leaves `bindings/` behind — roadmap task **T56**. Regenerate it and commit
the result with the code:

```bash
bash packaging/bindings.sh
```

CI's `bindings` job regenerates into a temporary directory and diffs, so forgetting this is a red
job rather than a release that ships a contract nobody's client matches. Two things fail *before*
that and say more: `crates/mixengine-proto/tests/bindings.rs` names the type that is missing, and it
also refuses two types with the same name — the contract is one file per type, so a collision is a
file carrying both declarations in whatever order the exporter ran them.

`cargo test --workspace --all-features` regenerates it as a side effect, because the exporter is a
`#[test]`. That is the mechanism rather than an accident; the command above is the deliberate way to
do the same thing.

### After changing a `sqlx::query!`

`sqlx::query!` checks its SQL against a real database **while compiling**, which is what turns a
misspelled column into a build error instead of a bug found at runtime. Nobody building MixEngine
has such a database, so the answers are committed as `.sqlx/` and every build without a
`DATABASE_URL` reads those instead of connecting. Ordinary builds therefore need nothing.

Editing or adding a query means regenerating them, and committing the result with the code:

```bash
cargo install sqlx-cli --no-default-features --features sqlite,rustls   # once
export DATABASE_URL=sqlite:target/sqlx-dev.db                           # ignored by git, like all of target/
cargo sqlx database create
cargo sqlx migrate run --source crates/mixengine-core/migrations
cargo sqlx prepare --workspace -- --all-targets --all-features
```

Forgetting the last step is invisible on the machine that made the change — `DATABASE_URL` is still
set there — and breaks everyone else's build. That is the one failure `lint` runs
`cargo sqlx prepare --check` for.

**Do not put `DATABASE_URL` in a `.env` file.** sqlx reads one automatically, and a stale database
sitting where every build finds it silently replaces the committed answers with whatever that file
happens to contain.

### After changing anything the privileged helper is built from

The helper carries a version of its own, `HELPER_VERSION` in `mixengine-proto::privileged`, and an
installed MixLab replaces the helper on a machine only when that version moves (the T182b design,
D1). So a change to what goes into the helper has to move it, once per release:

```bash
git config core.hooksPath .githooks          # once per clone
rustup target add x86_64-unknown-linux-gnu aarch64-apple-darwin   # once, beside your host target
bash packaging/helper-lock.sh --check        # what the hook runs for you
bash packaging/helper-lock.sh --bump         # when --check says so; commit the result with the change
```

`crates/mixengine-elevate/helper.lock` holds the fingerprint of every source file the compiler
builds into the helper on all three systems, and of every external crate in its closure, as the
last release shipped them. The pre-commit hook runs the check whenever a staged file is one of those
or `Cargo.lock`, the `lint` job runs it on every branch, and `set-version.mjs` moves the baseline
when a release is cut.

## CI matrix

CI fires by itself on a `v*` **tag** and on nothing else — a workspace that compiles for three
operating systems is worth a runner when you are asking a question, and a push is not the same thing
as asking. Every ref asks for its own answer, `master` included: push it, then request a run on it.

```bash
git push origin HEAD
gh workflow run ci.yml --ref "$(git branch --show-current)"
gh run list --branch "$(git branch --show-current)" --limit 1
```

The run carries the branch that asked, so two questions in flight stay apart. A second request on
the same branch cancels the first, because by then you have stopped caring about that answer —
except on `master` and on a tag, which are never cancelled so that those refs keep a complete
history of what they were told.

`scripts/ask-ci.sh` is those two commands with the push in front, and `scripts/watch-ci.sh` waits
for the verdict and prints the failing steps rather than a URL — and closes with the failed jobs
counted and named (*2 of 45 jobs failed: lint / lint, …*), under `--once` too, so the last line of
its output cannot read as green when it is not (T202c).

**And `scripts/gate.sh` in front of the push.** rustfmt, clippy, rustdoc with `-D warnings`,
`helper-lock.sh --check` and `check-docs.mjs` answer in about a minute here, where a red CI run
costs twenty; a red gate pushes nothing. It is added because a run went red on a broken intra-doc
link that clippy had passed. It does not run the test suites, several of which answer for the
machine rather than for the code, and it checks only this system's `cfg`. `--no-gate` skips it,
for a push whose point is CI itself.

### Asking about one job

A full run is thirteen jobs across three operating systems, and there is a loop where eight of them have
nothing to say yet: one job is red, and what you want is that job again as soon as possible.

```bash
bash scripts/ask-ci.sh --jobs test     # `test`, `services` and `rustdoc`; every other job is skipped
bash scripts/ask-ci.sh                 # every job, which is the default and the thing to end on
```

The groups are the job names in the table below — `lint test system bench bindings docs desktop
build` — plus `all`. `ask-ci.sh` refuses an unknown one itself, because a `choice` input is rejected
by an API error that does not say which words are allowed.

**A narrowed answer is not an answer about this workspace**, and both halves say so: `ask-ci.sh`
prints it when it asks, and `watch-ci.sh` names the jobs that never ran when a narrowed run comes
back green. The exit status stays CI's, because the run did succeed — what it succeeded *at* is the
part a reader can otherwise miss. Ask for `all` before believing anything.

**A tag is never narrowed.** The `inputs` context is empty for a push, so every job's condition
falls through to true and a release runs the whole matrix whatever a dispatch once selected;
`preflight` and `release` additionally refuse to run at all unless the group is `all`, so a
narrowed dispatch on a tag ref cannot produce half a release.

| Job | Runner | Runs |
| --- | --- | --- |
| `lint` | ubuntu | `fmt`, `clippy -D warnings`, `cargo deny` (licences + advisories), `sqlx prepare --check`, `node scripts/check-docs.mjs` (documentation links, spec status) |
| `test` | windows / macos / ubuntu | unit + component + integration under `cargo nextest` (profile `ci` in `.config/nextest.toml`, T170f), network egress blocked, `cargo doc -D warnings` for the runner's own OS on macOS and Linux |
| `services` | windows ×2 (`web`, `sql`), macos, ubuntu | every `#[ignore]`d suite that needs a real program — Caddy, nginx, PHP, the SQL servers, the caches, MongoDB — plus the connection count against a socket that really is connected; asked for with `test` (T170e). **A new real-program suite joins a group here, never `test`**, and a leg past 15 minutes on a warm cache becomes a new matrix row |
| `rustdoc` | windows | `cargo doc -D warnings` for Windows, as a job of its own because the Windows `test` leg is the run's critical path (T170d); asked for with `test` |
| `system` | windows / macos / ubuntu, elevated | `#[ignore]`d system tests, and the only place `MIXENGINE_SYSTEM_TESTS=1` is set — on every run of the workflow |
| `bench` | windows ×2 (`budgets`, `footprint`), macos, ubuntu | performance budgets from [../standards/testing.md](../standards/testing.md), in a **release** build; Windows split in two because its measurements alone took 13.5 minutes (T170j) — `budgets` is the shim overhead and the cold path, `footprint` the idle and tuned footprints and M3 |
| `bindings` | ubuntu | regenerates ts-rs bindings and fails if the committed output differs |
| `docs` | ubuntu | builds the user handbook's site and fails if the committed command reference is not what `mix` prints |
| `desktop` | ubuntu-22.04 | the desktop application: `npm run build`, `npm test`, `npm run lint`, then its own workspace's `clippy -D warnings`, `cargo test` and `cargo audit` |
| `window` | windows, windows arm64, macos, ubuntu, ubuntu arm64 | the desktop application in release, built on the runner (never in the container) by `packaging/desktop.sh`, handed on as `window-<os>` (a tar, kept 14 days) — asked for with `build` (T171b) |
| `binaries` | the same five | the four headless binaries in release, by `packaging/stage.sh --build-only` (in the manylinux container on Linux), handed on as `binaries-<os>` (a tar, kept one day) — asked for with `build` (T171b) |
| `build` | the same five, after `window` and `binaries` of its own leg | installers for each OS (macOS ships one universal artifact), packaged from the two tars under `MIX_PREBUILT=1` and uploaded as `mixengine-<os>`; **fails if anything was compiled in it**, and reports the whole path's time against 18 minutes. All three jobs build **on a branch without LTO and with 16 codegen units, on a tag exactly as `[profile.release]` says** (T170h: a branch proves the packaging, which does not depend on LTO, and only a tag feeds `release`); the window has been placed by every installer since T105 |
| `release` | ubuntu | **on a `v*` tag only**: gathers the five legs' `mixengine-*` artifacts, packs the API contract, writes `latest.json`, signs each with the updater key, verifies what it published, and leaves a **draft** GitHub Release a person publishes |

**macOS builds by ref** (T171c). Each of the three build jobs chooses the same way:

| Ref | Release profile | macOS slices | macOS files |
| --- | --- | --- | --- |
| a `v*` tag | `[profile.release]` as written | x86_64 and aarch64 | `macos-universal` |
| `master` | no LTO, 16 codegen units | x86_64 and aarch64 | `macos-universal` |
| any other branch | no LTO, 16 codegen units | aarch64 only, x86_64 checked with `cargo check` | `macos-arm64` |

So a branch's macOS artifacts do not run on an Intel Mac. The `lipo` path and the x86_64 release
build are proved by every `master` run, before any tag.

**One workflow per job family since T172e.** `ci.yml` holds the triggers, the `jobs` choice,
concurrency and one entry per family; each family is a called workflow of its own —
`_lint.yml` (`lint`, `bindings`, `docs`, `desktop`), `_test.yml` (`test`, `rustdoc`),
`_services.yml`, `_system.yml`, `_bench.yml`, `_build.yml` (`window`, `binaries`, `build`) and
`_release.yml`. `preflight` stays in `ci.yml`, so that it still answers in seconds rather than
behind the build. A called workflow does not inherit the caller's `env:`, so each declares the same
four variables, and job names read `<family> / <job>` in the run and in the API.

**Three workflows are not in that table**, and none of them belongs in `ci.yml` — each follows
`master` on its own, which is the thing that file will not do.

`.github/workflows/gallery.yml` sends one `repository_dispatch` to `mixengine-packages` when a push
to `master` touches `crates/mixengine-core/src/blueprints/gallery/**` or `blueprints/trust.rs`, so
the check that compares the published signed gallery with this one runs against the commit that
changed it instead of on that repository's weekly cron. It checks nothing out and builds nothing —
what `ci.yml` is protecting is a three-OS compile, not a runner-minute — and it is the one job here
that cannot wait to be asked for, because being forgotten is the failure it exists to prevent. It
needs `PACKAGES_DISPATCH_TOKEN`, a secret with write access to the other repository, and fails loudly
when it is missing: see [../features/blueprints.md](../features/blueprints.md).

`.github/workflows/pages.yml` builds the handbook and
deploys it to GitHub Pages on every push to `master`. It is separate because deploying needs
`pages: write` and `id-token: write` and a `github-pages` environment, and `ci.yml` is
`contents: read` and stays that way. It carries no `paths:` filter — filtering to the corpus would
leave the site claiming the previous version after a release bumped `Cargo.toml`, silently — and it
follows `master` rather than a tag, because a handbook that only updated when a version was cut would
describe the previous release for as long as the next one took.

`.github/workflows/server.yml` builds and tests the sync server — `server/worker/`,
`server/native/`, and the conformance suite both of them answer to (T177b, T177h). It fires on
`server/**` and on nothing else, so a change to MixLab or to the engine never spends a runner on a
server nobody touched, and a change under `server/` never drags the three-OS matrix behind it. It
is not a job family in `ci.yml` because the reason every ref there asks for its run is the cost of
compiling the workspace for three operating systems, and this is one Ubuntu runner. It follows
`master` for a reason of its own: Workers Builds deploys `server/worker/` from that branch without
being asked, so there a server that is red and unrun is a server that is deployed red. **The
separation is of triggers, not of repositories** — both implementations are jobs in this one
workflow, so one run still proves they agree, which is what
[ADR 0046](../decisions/0046-the-sync-server-lives-beside-the-client-it-serves.md) was decided to
buy.

**Two settings are a person's, once.** GitHub Pages must be enabled for the repository with the
source set to GitHub Actions. `actions/configure-pages` is asked to enable it through the API, and
where the token may not, the job fails saying so — deliberately, because a deploy that skipped itself
quietly would leave a green tick over a site nobody published. And `PACKAGES_DISPATCH_TOKEN` must
hold a token with write access to `mixnz/mixengine-packages`; nothing can create it from inside a
run, because the whole point is reaching a repository this one's `GITHUB_TOKEN` has no claim on.
Both fail loudly for one reason: the failure they guard against is silence.

**All eight exist since T103**: `lint`, `test`, `bench`, `system` — which arrived with T40, the first
`#[ignore]`d system test — `build`, which arrived with T85, the task that produced something to
install, `bindings`, which arrived with T56, the task that produced a contract to check, `docs`,
which arrived with T90, the task that produced a site to build, and `desktop`, which arrived with
T103, the task that brought the desktop application here. Until `bindings` existed, a `ts-rs`
type whose committed output had drifted was caught by a person or by nobody. `bindings` also gates
`release`: a tag whose committed contract had drifted would otherwise publish the drift, signed.

**`docs` is narrow on purpose, and T89's rule is why.** The handbook's own invariants — the two
languages holding the same pages, every `./<slug>.md` link resolving, a translation revisited after
its source changed, prose wrapped — are `cargo test` with nothing to download and no privilege, so
`test` already runs them on all three operating systems and this job needed no step for them. What
is left over is the part that is not a test: building the site, which compiles a Markdown renderer,
and holding `docs/guide/en/cli.md` against what `mix docs --reference` prints.

### After changing a `clap` command, or any page of the handbook

The user handbook is `docs/guide/{en,vi}/` — roadmap task **T90**,
[design](../specs/2026-09-05-t90-the-documentation-site-design.md),
[ADR 0021](../decisions/0021-the-handbook-is-one-corpus-published-three-ways.md). Two of its files
are not written by hand and go stale silently:

```bash
bash packaging/docs.sh --reference   # after editing a clap command or its help text
bash packaging/docs.sh --restamp     # after translating an edited English page
bash packaging/docs.sh --check       # what CI runs
```

`--restamp` is run **after** translating a page, never instead of it: every Vietnamese page carries
the SHA-256 of the English page it was made from, so an English edit that nobody carried across is a
failing test rather than a discovery six months later. All the stamp records is that somebody
looked.

**T88 added one step to `release` and one artifact to `build`.** The step is `packaging/feed.sh`,
which writes `latest.json` into the distribution directory **between** gathering the legs and signing
them — it is written there rather than in a leg because no leg can see the other four, and before the
signing rather than after because being in that directory *is* how it gets signed. The artifact is
the update payload: a plain `mixlab-<version>-windows-<arch>.zip` of the release's binaries, which is
what `mix self-update` applies to a per-user Windows install. Every other installer needs root, and
is updated by the next one of its kind (T88f, T182b). `packaging/README.md` has the shape of both.

**The feed's notes come from `git` and not from GitHub**, and the ordering is why: `--generate-notes`
runs when the draft is created, which is after the signing is over, so notes GitHub wrote cannot be
inside a document that was already signed. Re-signing afterwards would put the private key on the
machine of whoever edits the draft, which is the one thing T86 arranged not to need. So `feed.sh`
writes the tag's own commit subjects and a `notes_url` pointing at the page a person may improve
afterwards.

**T86 added `release`, and a second job that is not in the table**: `preflight`, which answers in
thirty seconds the three questions that would otherwise fail an hour into a release — the tag matches
the workspace version, `packaging/updates.pub` matches the key this build pins, and both signing
secrets are set. Neither runs except on a `v*` tag, which is also the run that makes release-checklist
item 1 something CI asserts rather than a person.

**`test` downloads one thing, and it is a server.** `crates/mixengine-cli/tests/caddy.rs` (T31) is
the only suite in the workspace that judges a recipe against the program it configures, which cannot
be faked: whether Caddy accepts a generated Caddyfile — with a Windows path in it — is a question
only Caddy answers. So the job fetches a pinned Caddy from `mixengine-packages`' own release before
the network is taken away, points `MIXENGINE_CADDY_PACKAGE` at it, and runs that suite `--ignored`.
It is a **fixture and not an install**: nothing checks a signature or a hash there, because
`core::index` and `core::install` are what do that and both have suites of their own. Run it by hand
the same way:

```bash
MIXENGINE_CADDY_PACKAGE=/somewhere/caddy cargo test -p mixengine-cli --test caddy -- --ignored
```

It stays a step in `test` rather than becoming a job: it needs the same debug build every other
correctness answer needs, and a job of its own would compile the workspace a second time to run one
test. `#[ignore]` is what keeps it out of a run that has no Caddy — and what makes that visible,
since a skipped test is reported and a test that returned early is not.

`bench` is on all three runners rather than on ubuntu alone, which is what this table used to say.
The budget it gates is the same everywhere; what it stands in front of is not one mechanism, since
the shim `exec`s on Unix and starts a child inside a Job Object on Windows — and the wall clock it
reports beside the gate is the only place that difference is written down as a number. It is a job
of its own rather than a step in `test` because these tests are `#[ignore]`d and need a release
build, which is a second compilation no correctness answer should wait behind. Run one by hand the
way CI does, `--test-threads=1` included:

```bash
cargo build --release -p mixengine-testkit --bin fakeservice
cargo test --release -p mixengine-shim --test overhead -- --ignored --nocapture --test-threads=1
```

Both lines matter. Selecting one test target does not build `fakeservice`, so a release copy from an
earlier build is used as it is; and the two benchmarks each spend their whole time creating
processes, so run in parallel each measures the other.

**Four budgets since T72a**, each its own step so that a red job names what went red without anybody
opening a log: the shim's overhead, the **idle footprint**, the **cold path**, and M3's warm start.
The footprint step runs a daemon and a real Caddy with nothing else, reads them through
`mix metrics` thirty seconds after the last command, gates `mixengined` alone and prints the total
beside it. It needs only the Caddy the fetch step above already pulls, and no `dbus-run-session`
wrapper: nothing in it starts a MariaDB, so nothing in it has a password to store.

The cold-path step needs that Caddy and **three PHPs**, fetched into three directories of their own
and named in `MIXENGINE_PHP_RUNTIMES`. Three because a pool is only cold once, so three rounds need
three pools, which need three runtime installs — and three *versions* because two of them predate
`pm.status_listen`, which is what holds T72a's idle probe to working on every PHP this product
offers. The step spends about ninety seconds waiting for the idle sweeper before it measures
anything, and pays that wait once for all three rounds.

**Both run before M3 deliberately.** A failing step ends its job, and M3 starts three servers eight
times over and is bimodal on ubuntu — the first run of the footprint budget was skipped on that
runner for exactly that reason, which is a measurement lost to somebody else's bad minute. Cheap
independent measurements go first, and the cold path needs the rule more than the footprint does:
losing it costs a step that had already stood still for a minute and a half.

```bash
cargo build --release -p mixengine-daemon --bin mixengined
MIXENGINE_CADDY_PACKAGE=/path/to/unpacked/caddy \
  cargo test --release -p mixengine-cli --test idle_footprint -- --ignored --nocapture

MIXENGINE_CADDY_PACKAGE=/path/to/unpacked/caddy \
MIXENGINE_PHP_RUNTIMES=/path/to/php-7.0.33:/path/to/php-7.4.33:/path/to/php-8.3.33 \
  cargo test --release -p mixengine-cli --test cold_path -- --ignored --nocapture
```

The first line is not optional and the reason is the same shape as `fakeservice`'s: `cargo test -p
mixengine-cli` builds `mix` and **not** `mixengined`, so the suite drives whichever daemon was built
last — which, while this budget was being written, was one still carrying the bug it had been written
to find. **It cost the cold path an hour too**: a 502 that had already been fixed went on being
measured, because the suite was still starting the daemon from before the fix.

## Targets

| OS | Targets | Installer |
| --- | --- | --- |
| Windows | `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | NSIS per-user installer, the same without the window, and the zip the per-user update swaps from |
| macOS | `x86_64-apple-darwin`, `aarch64-apple-darwin` → universal binary | `.pkg` with the window + a headless `.pkg` |
| Linux | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, the four binaries against glibc 2.28 and the window against glibc 2.35 | `.deb` + `.rpm`, each with the window and headless |

**Every installer in that column places five binaries** since T105 — the four command-line programs
and MixLab, the window ([the design](../specs/2026-09-09-t105-the-window-in-every-installer-design.md)).
The **headless** installer beside each is the same release without the window: four binaries, no
WebKitGTK dependency, for the machine that has no display. The feed lists it as its own flavour, so
an update hands a machine the package it has.

**The window's floor is not the other four's, and T105a is why the row above writes them
separately.** The four command-line binaries are built in the `manylinux_2_28` container; the window
cannot be, so it is built on the leg's own `ubuntu-22.04` host and carries that machine's glibc 2.35,
with WebKitGTK 4.1 from the distribution, which the package declares as a dependency
([ADR 0028](../decisions/0028-the-appimage-does-not-carry-webkitgtk.md) settled this for the
AppImage, which T182b removed).
`packaging/linux/window-floor.sh` reads the floor off the binary on every Linux leg and fails the
build if it has risen past `MIX_WINDOW_GLIBC`, which is the number both install pages promise.

**What T85 built is the host architecture of each row, natively; T85a built the rest, also
natively.** GitHub's own arm64-hosted runners (`windows-11-arm`, `ubuntu-24.04-arm`) — free and GA for
this repository, which is public — turned the second `aarch64` row into another native leg rather than
a cross-compilation of the first, the same way macOS has always built both of its slices on Apple's
own toolchain. The one real toolchain question left was the glibc floor: both Linux legs compile
inside a pinned `manylinux_2_28` container, matching the floor `runtime-packaging.md` already measured
for PHP 7.0–8.0, so a `.deb` built today keeps running on the LTS distributions it is aimed at rather
than on whatever glibc the `build` job's runner happens to ship this month.

**macOS ships a `.pkg` and not the `.dmg` this table used to name.** A disk image is a carrier for
something you drag out of it, and the thing that used to be dragged was an application bundle
[ADR 0011](../decisions/0011-no-gui-in-this-repository.md) deleted; what was left to ship there was
four command-line binaries. A `.pkg` also runs as root, which is what lets it place the privileged
helper at install time — see [ADR 0015](../decisions/0015-the-helper-installs-itself.md). An
application bundle came back with [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md)
and T105, and none of that reasoning changed: `MixLab.app` is placed in `/Applications` by the
package rather than dragged out of an image.

## What the installer does

1. Places `mixengined`, `mix` and `mixengine-shim` (per-user location on Windows, so updates need
   no UAC; `/usr/local/bin` from the `.pkg` and `/usr/bin` from the `.deb` and the `.rpm`). **The
   shim goes beside `mixengined` and nowhere else**, because that is the only place
   `core::shims::source` looks — an install without it starts, reports itself healthy, and leaves
   `<root>/bin` empty, which is every runtime command the product exists to provide (**T85c**).
2. **Does not place `mixengine-elevate`. MixEngine does that itself** — the operation
   `PrivilegedOp::HelperInstall`, applied inside the elevation prompt first-run setup already costs
   ([ADR 0015](../decisions/0015-the-helper-installs-itself.md)). It goes to
   `%ProgramFiles%\MixEngine\`, `/Library/PrivilegedHelperTools/` or
   `/usr/local/libexec/mixengine/`, and it must not sit anywhere the user can write. The `.deb`, the
   `.rpm` and the `.pkg` ship it at that same path anyway, because they run as root and can — and
   the operation then answers `AlreadyDone`. **The two ways of installing that run entirely as the
   user** — the per-user Windows installer and a `cargo build` — are why this cannot be a
   packager's job.
3. **Does not register daemon autostart either. MixEngine does that too** — `autostart.enable`,
   reachable from `mix autostart enable`, and by nothing an installer runs
   ([ADR 0016](../decisions/0016-autostart-is-registered-by-mixengine.md)). Item 2's argument
   reversed: a logon task lives under one account's SID, a LaunchAgent in one user's
   `~/Library/LaunchAgents`, a systemd *user* unit in one user's `~/.config` — so the three formats
   that run as root are precisely the three that cannot know which account will use MixEngine, and
   the three that run as the user are not where a "may I start something at every login" question
   belongs. Nothing about it is elevated on any of the three systems, and `mix autostart disable`
   takes it away without stopping a running daemon.
4. Puts its own directory on this user's PATH, so `mix` is runnable. **Not `<root>/bin`**, which is
   the directory of runtime shims and belongs to `path.install`: the two therefore write different
   segments of one value and each removes only its own, which is what makes two authors safe. On
   Windows the edit carries a guard — NSIS's `ReadRegStr` truncates at `NSIS_MAX_STRLEN`, so a PATH
   at or above that length is left alone with a line on screen saying so.
5. **Does not** install the CA, resolver config, port grant, or any runtime — those happen on first
   use, batched into a single elevation prompt, so a fresh install changes as little as possible.

## Packaging

The scripts live in [`packaging/`](../../packaging/), one directory per OS, each run on that system;
there is no cross-packaging, which is why the `build` job is three legs.

```bash
bash packaging/desktop.sh               # MixLab, once per leg — see below
bash packaging/windows/build.sh         # the two per-user installers and the update zip
bash packaging/macos/build.sh           # one universal .pkg and a headless one
bash packaging/linux/build-deb.sh       # .deb, with the window and headless
bash packaging/linux/build-rpm.sh       # .rpm, with the window and headless
```

**The first line is optional and is there for speed.** `packaging/stage.sh` runs `desktop.sh` itself
when nothing has staged the window, or when the staged one was built from other sources than the
checkout holds now (its `fingerprint` file differs), so any one of the lines below works on its own — but the two
Linux scripts each call `stage.sh`, and running it once up front means one of them is not paying for
a ten-minute webview build inside a packaging run. The window's crate is a workspace of its own that
this one excludes (ADR 0027, rule 5), which is why `stage.sh` cannot build it with `cargo -p` like
the other four.

Everything lands in `target/packaging/dist/` with a `.sha256` beside it. **A checksum is not a
signature** and is not offered as one: it is what lets a person who downloaded twice tell whether
they got the same file. The signature is `packaging/sign.sh`, which T86 added and which the `release`
job runs over that same directory — `.sha256` files are not signed, because a signature over a
checksum is a weaker way of saying what the signature over the artifact already says.

Each script ends by opening the artifact it just made and asserting the five binaries are in it —
`unzip -l`, `7z l`, `pkgutil --payload-files`, `dpkg-deb -c` and `rpm -qlp`. A packaging script that silently produced an empty archive is the failure this
whole job exists to prevent, and it is not one CI notices by itself.

**A headless archive is checked for four, and for the absence of the fifth.** Counting would not
catch an archive that quietly grew a webview; only asking about the window by name does, which is
what `MIX_WINDOW` in `packaging/common.sh` is for. The `.deb` and the `.rpm` additionally assert
their own dependency field — `dpkg-deb -f … Depends`, `rpm -qp --requires` — because a control file
written by a heredoc and read by nothing else is one an editing mistake can quietly empty.

**One artifact in `dist/` is not a binary.** `packaging/bindings.sh --pack` archives the committed
TypeScript contract as `mixengine-api-<version>-typescript.tar.gz` — roadmap task **T56**,
[design](../specs/2026-09-05-t56-the-published-api-contract-design.md). It is
packed in the `release` job from the tree in `bindings/`, which is current because that job needs
`bindings`; `sign.sh` signs it with everything else, and `feed.sh` does not offer it to
`mix self-update`, because it matches a payload by the `mixlab-<version>-<os>-…` shape and this is
not one. What the contract states is what the daemon **writes** —
[ADR 0020](../decisions/0020-the-published-contract-is-the-shape-the-daemon-writes.md).

The version comes from `[workspace.package]` in the root `Cargo.toml`, read by every script, so
cutting a release is a version bump and nothing else.

The elevated helper creates its own audit log on first run — `%ProgramData%\MixEngine\elevate.log`,
`/Library/Logs/MixEngine/elevate.log`, `/var/log/mixengine/elevate.log` — which is the first thing
MixEngine leaves outside `MIXENGINE_HOME`. Removing it is itself a privileged operation, so
`mix uninstall` owes it one (**T87**, the complete uninstall path). T47's `mix doctor` reports it and
does not remove it — a diagnostic that deleted a root-owned audit trail would be deleting the record
of what it was diagnosing.

**Since T85 there are two such files, not one.** The helper the operation in item 2 installs is
root-owned and outside `MIXENGINE_HOME` for exactly the audit log's reason, and removing it needs the
same privileged operation of its own. T87 owes both.

Uninstall reverses all of it: stop services, remove the hosts block, resolver/NRPT rule, firewall
rules, port grant, CA from every store, autostart entries, PATH entry. It asks before deleting
`data/` and prints exactly what it kept.

## Signing

**MixEngine ships without OS code signing.** Two different signatures are involved and only one is
in use — see [../features/updates.md](../features/updates.md) for the full table and consequences.

- **Updater signature (minisign / Ed25519)** — free, and the thing that actually protects users from
  a tampered update. It was mandatory while the updater was Tauri's; now it is ours by choice, and
  the choice does not change ([ADR 0011](../decisions/0011-no-gui-in-this-repository.md)).
  **Built by T86**: the private half is this repository's `UPDATE_SECRET_KEY` and `UPDATE_PASSWORD`
  secrets, the public half is committed as `packaging/updates.pub` and pinned as
  `core::updates::PUBLIC_KEY`, and `packaging/sign.sh` verifies every signature it makes back against
  that pinned key — so a run signed by a secret that is not its pair fails before anything is
  uploaded. Rotating the key strands every installed copy; read
  [../features/updates.md](../features/updates.md) before doing it.
- **Authenticode / Apple Developer ID** — not purchased. Accepted costs: SmartScreen warnings on
  Windows that reset with every release, and a Gatekeeper rejection of the `.pkg` that since macOS 15
  requires System Settings → Privacy & Security → "Open Anyway" if it is opened in Finder.
  **T86a measured both, and both are narrower than that sentence.** On Windows the installer is the
  only file judged and nothing it writes is judged again; on macOS
  `sudo installer -pkg <file> -target /` installs the quarantined package from a terminal without any
  of that, which for a command-line product is the instruction to document first. See
  [../features/updates.md](../features/updates.md).
- Linux: detached minisign signatures published with the release.

Recommended sequencing: Linux and Windows first, macOS once a Developer ID is available. Revisit
[ADR 0005](../decisions/0005-on-demand-elevation.md) if that changes — signing and the elevation
design are linked decisions.

## Versioning and updates

- SemVer, single version across the workspace, tagged `v0.1.0`. Pre-1.0 the API may break between
  minors; each break is listed in the changelog.
- Auto-update via `mix self-update` against a `latest.json` published on GitHub Releases. Updates
  are **opt-in**, never silent, because an update restarts the user's running services. **Built by
  T88** — [design](../specs/2026-09-04-t88-self-update-design.md): the daemon
  checks at start and on a daily clock, both silent on failure; a release is downloaded, hashed
  against the signed feed, unpacked and *run once* before anything is replaced; and a copy of
  MixEngine that a `.deb`, an `.rpm` or a `.pkg` installed is updated by the next package of its
  kind rather than in place (T88f, T182b).
- **`mixengine-elevate` is excluded from auto-update** and is replaced only through its own explicit
  elevation prompt. This is a security boundary, not a convenience choice.
- The daemon and clients negotiate a protocol version on connect; so do the daemon and
  `mixengine-elevate`. An old elevate keeps serving the operations it knows while the app asks the
  user to upgrade it.

## Release checklist

1. `cargo deny` clean, all CI green on all three OSes — and **take the coverage reading**, which CI
   cannot because it has no network:

   ```bash
   cargo test -p mixengine-core --test index -- --ignored --nocapture
   ```

   It verifies the published index against the key compiled into this build and prints what each of
   the six targets can install. It fails on a cell nothing can be installed from and whose reason is
   not written down in `KNOWN_EMPTY` — roadmap task **T92**. A hole there is not a release-blocking
   bug in this repository; it is a target that should not be released for, or a reason that has to be
   added to that list on purpose.
2. Bump version, update `CHANGELOG.md`, and **capture an upgrade fixture at the schema being
   released** — `cargo run -p mixengine-core --example capture-upgrade-fixture -- <schema>`, with a
   seed beside it, committed. Verifying the path is CI's since **T89**:
   `crates/mixengine-core/tests/upgrade.rs` migrates every committed fixture with the real
   `Store::open` on all three operating systems and compares every row before and after, and
   `crates/mixengine-cli/tests/upgrade.rs` starts a real `mixengined` on one of them. What is still
   a person's is knowing **which** schema was shipped, because the tree only knows which one is
   current — so a release that skips this capture is a release whose successor has no fixture to
   upgrade from.
3. **Check the rehearsal, and rerun it if it is not recent** — roadmap task **T174**. Off a tag CI
   builds without LTO, at `opt-level = 0`, and on one macOS slice, so the tag run is the only run
   that compiles what a release ships. `release-rehearsal.yml` builds that configuration every
   Monday on `master`; what this step asks is that a **green** one exists **for the code being
   tagged**:

   ```bash
   gh run list --workflow release-rehearsal.yml --limit 3
   gh workflow run release-rehearsal.yml --ref master   # if the newest is old, red, or behind
   ```

   Read two things in it: that every `build` leg is green, and that none came close to its timeout —
   `window` allows 45 minutes, and `codegen-units = 1` with `opt-level = 3` is the slowest thing this
   product compiles. A rehearsal that fails here costs one run; the same failure at step 4 costs a
   half-uploaded draft. **A schedule can be quietly disabled** after 60 days without activity in this
   repository, so an empty list is a reason to dispatch one, not evidence that nothing changed.
4. Push the tag `v<version>`. CI runs everything, signs every artifact, and leaves a **draft** release
   carrying each artifact with a `.sha256` and a `.minisig` beside it. Nothing is notarised — that is
   the right-hand column of the signing table, and it is not purchased.
5. Smoke-test each installer *from that draft* on a clean VM: install → create site → HTTPS →
   uninstall → verify nothing left behind. Then edit the notes and publish the draft by hand.

   **Publishing starts `install.yml`** (T197a), which installs the headless package through
   `install.sh` and `install.ps1` on five systems — the first time anything installs the release
   just made public through the one-line command. Read it before announcing the release:
   `gh run list --workflow install.yml --limit 1`.

   **Say in the notes what the updater will and will not replace** — roadmap task **T106**. From the
   release that lands it, the payload carries MixLab and `mix self-update` replaces it — but only
   where it is installed beside the binaries. An install from before that release has no window at
   all, and a macOS `.pkg` install keeps its window in `/Applications`, where no update reaches it.
   Both cases are "run the installer to get the window", and the notes are the only place a person is
   told: the feed's own notes are the tag's commit subjects, written before the draft exists.

   **Say once, in the first release carrying T187, that it is the last one reached the old way** —
   [T187](../specs/2026-09-26-t187-mixlab-updates-itself-design.md), D11. A window from before it
   has no MixLab updater, so this release arrives through the MixEngine tab, `mix self-update` or
   the installer. From the next one, MixLab updates itself from Settings → Updates.

   **And take the two readings no machine can take** — roadmap task **T86a**, whose other half is
   measured by `packaging/*/probe.sh` on every run of the `build` job. Both need a *browser* download
   of the *published* asset, because that is what applies the mark the operating system reads; a file
   copied onto the VM by any other route is not the file a user gets. Record what happened in
   [../features/updates.md](../features/updates.md), beside the measured readings:

   1. **Windows.** Download `…-setup.exe` in Edge from the release page and double-click it in
      Explorer. Did "Windows protected your PC" appear? What did the publisher line say? Was "More
      info → Run anyway" needed to get past it? Then do the same with the portable `.zip`: extract it
      *in Explorer* and run `mix.exe` — the probe's W4 measures that Explorer marks all three
      binaries, so this is where the count of warnings a user sees is confirmed.
   2. **macOS 15+.** Download the `.pkg` in Safari and double-click it in Finder. Record the dialog,
      then the System Settings → Privacy & Security → "Open Anyway" path and how many steps it took.
      The probe's M4 already answers `installer(8)` from a terminal; what this adds is the flow a
      person who did not read the instructions will actually meet.

   **The SmartScreen half is inherently a two-release reading and is therefore taken twice**: once
   here, and again on the release after this one, to confirm the warning returned. It is expected to
   — reputation with no publisher identity accrues to a file hash, which is the probe's W1 — and the
   point of taking it is that the prediction is checked rather than assumed.
5. Publish the updated package index if runtimes changed
   ([runtime-packaging.md](runtime-packaging.md)).

## Why CI is shaped this way

The reasoning behind `.github/workflows/`, moved here from the workflow's own comments in T172d
so that a step carries a line or two and this page carries the argument. Grouped by job, in the
order the jobs are written; each entry is named by the step it explains, or stands for the job
itself when it has no name.

### The workflow as a whole

The ten jobs described in docs/operations/build-and-release.md: `lint`, `test`, `rustdoc`,
`services`, `system`, `bench`, `bindings`, `docs`, `desktop` and `build`.

`rustdoc` and `services` arrived with T170d and T170e, both split out of `test` (see each job).

`desktop` arrived with T103, the task that brought the desktop application into this repository
(ADR 0027).

`system` arrived with the first `#[ignore]`d system test (T40), `build` with T85, the task that
produced something to install, `bindings` with T56, the task that produced a contract to check, and
`docs` with T90, the task that produced a site to build. Each was "a green job that proves nothing"
until then, which is the rule this comment exists to record rather than the list it happens to
name — and it is why T90's *corpus* invariants added no step anywhere: they are `cargo test`, so
`test` already runs them on all three operating systems.

Publishing the site is not here at all. It needs `pages: write` and `id-token: write`, and this
workflow is `contents: read` and stays that way: see `.github/workflows/pages.yml`.

**Line tables only in debug builds (T170l).** A failing test's backtrace still names files and
lines; what goes is variable and type information nobody reads from a CI log. On Windows the MSVC
linker writes a PDB for each of ~150 test binaries, and a smaller PDB is much of what `Build
tests` spends there. Set once for the whole workflow and not per job: rust-cache hashes the
`CARGO_*` environment into its key, and `test` and `services` share one entry. Release builds are
unaffected, because the variable names the `dev` profile, and so are developers' machines.

### Triggers

**No branch fires on its own, `master` included.** The `test` job is the only thing in this
repository that compiles the workspace for all three operating systems, and two thirds of
`mixengine-platform` is behind a `#[path]` that a developer machine never builds — so a change that
breaks Linux or macOS is invisible until something compiles it there. That answer is worth a
runner when somebody is asking for it, and a push is not the same thing as asking: a merge, a
documentation fix and a work-in-progress save all look identical to a `push` trigger, and only one
of them wanted an answer.

So every ref asks for itself, `master` the same as any other. Push, then request a run on it:

    git push origin HEAD
    gh workflow run ci.yml --ref "$(git branch --show-current)"

The run is labelled with the ref that asked, so two questions in flight are told apart by their ref
instead of overwriting each other on one shared lane.

**A tag is the one exception**, and it is not a branch. Pushing `v*` fires the run that *is*
release-checklist item 1 — "all CI green on all three OSes" — and then `preflight` and `release` at
the bottom of this file: sign what `build` made, and assemble a draft somebody publishes. That one
has to be automatic, because a tag whose run was never requested is a release nobody checked.

The dispatch list is read from the default branch: a workflow file only becomes requestable once
this file is on `master`, and a branch that edits it runs its own version once selected.

Pull requests are not a trigger. A pull request's head is a branch you can request a run on like
any other, and leaving `pull_request` on would mean the same commit builds twice.

### The `jobs` input

**One group at a time, not a list**, because the reason to narrow a run is always one
question: *is `test` green yet.* A run that answered about three of nine jobs is not an
answer about this workspace, and `all` is the default so narrowing is something somebody
chose rather than something they inherited.

A group is a job's own name, so adding a job adds an option and nothing else has to move.

### `lint`

**The elevated helper builds without the rest of the platform.** D8 of the T40 design. `mixengine-elevate` runs as root, and its whole dependency closure is a
security decision — its own manifest says so. Cargo unifies features across a workspace build,
so `cargo build --workspace` compiles `mixengine-platform` with tokio and keyring and links
the helper against that; the lean tree is a property of building the helper **on its own**,
which is what both of these steps do and what the release pipeline does.

A change here is not a lint failure to be waved through: read the diff, and if the new
dependency belongs in a binary that runs as root, commit the updated list in the same change.

**One dbus-secret-service in the tree, or the Linux keyring reading is silently dead.** ADR 0013. `linux/secrets.rs` reads a machine with no secret service by downcasting `keyring`'s
boxed source to `dbus_secret_service::Error`, which works only while our direct dependency and
the one `keyring` resolves are the **same** package. Two versions in one tree is not a build
failure and not a test failure: the downcast simply answers `None` for ever, and every machine
without a keyring goes back to being told its store refused.

So it is counted rather than trusted. The day `keyring` moves to a version we do not follow,
this step is what says so — and the fix is to follow it in the same change, not to widen the
match here.

**`--color never` is load-bearing.** This job sets `CARGO_TERM_COLOR: always`, so a repeated
subtree's `(*)` marker arrives wrapped in escape sequences and the `sed` below stops matching
it — which counts one package as two and fails a tree that is fine. Locally the same command
passes, because cargo drops colour on its own when it is not writing to a terminal.

**Install sqlx-cli.** `sqlx::query!` checks its SQL against a real database at compile time. Nobody building
MixEngine has one — so the answers are committed as `.sqlx/`, and every other build reads
those instead of connecting (T14). The failure this step exists for is a query edited
without re-running `prepare`: the author's machine still builds, because DATABASE_URL is set
there, and everyone else's stops. `--check` regenerates the answers and fails if they differ
from what is committed.

Prebuilt binary rather than `cargo install sqlx-cli`, which is a four-minute compile of a
tool this job uses for six seconds.

Pinned to the `sqlx` version in Cargo.lock, and it has to stay in step with it: the contents
of `.sqlx/` are a version-specific format, so an unpinned CLI would start regenerating them
differently the day sqlx releases and fail `--check` on whichever unrelated PR ran next.

**Install minisign.** T86's D9. `packaging/sign.sh` drives an external tool, and the only other thing that would
ever run it is a release — so it is run here, on every CI run, against a throwaway key.

In `lint` rather than in `test` for two reasons: it is a check on this repository and the
tools it uses, which is what every step above it is; and `test` runs on three operating
systems with network egress blocked, where installing minisign would be three problems in
exchange for two answers nobody needs.

### `test`

**45 since phase 11, and it is the line past which waiting stops being the answer, not a
budget.** The Windows leg took 24.5 minutes warm on master (run 34149551423) and was cut off
at 30 on the first branch to change the toolchain pin (run 34235885258): a new pin is a new
cache key, `Build tests` went from 313 s to 602 s with nothing to restore, and every suite ran
a third slower on that runner. Nothing hung — the leg was killed installing PostgreSQL, the
ninth of eleven suites. The same reasoning as `system`'s 45.

**What this leg runs as (T2b).** T2b, answered: this leg holds `BUILTIN\Administrators` as an *enabled* group at High
Mandatory Level — a full token, not the UAC-filtered one where that group is present
deny-only and grants nothing. The account name in the cache paths, `runneradmin`, never
settled that; only `/groups` does.

It stays as an assertion rather than being deleted with the question, because
standards/testing.md now states the answer and derives a rule from it — write Windows
exclusion tests structurally, never by attempting an access this token would win — and a
runner image that quietly de-escalated would leave that page confidently wrong. Failing on
the good news is the point: the news is only good once somebody acts on it.

**Defender's state.** **Defender's state, recorded rather than assumed (T170k).** The runner image turns real-time
protection off and excludes C:\ and D:\ (`images/windows/scripts/build/Configure-WindowsDefender.ps1`
in actions/runner-images), which is why Windows being slower here is the linker and
CreateProcess, not a scanner. That is a promise about today's image; this makes the log say
which image a run had, and warns rather than fails, because a scanner makes a leg slower,
not wrong.

**Only `master` writes (T170a).** A branch or a tag restores `master`'s entry for this key —
or, when its `Cargo.lock` differs, the newest entry with the same prefix — and saves
nothing. Before this, one run wrote about a whole 10 GB quota, every branch run evicted
`master`'s entries, and every run built cold (run 35430523430). The arithmetic is in
docs/specs/2026-09-19-t170-a-test-job-that-scales-design.md. **No run fires on `master` by
itself**, so a run requested on `master` after a merge is what refreshes what every branch
starts from.

**Install the packages this leg's tests need.** The keyring capability talks to a D-Bus secret service on Linux, and a runner has a session
bus with no provider on it — which since T15b the tests read for what it is, a machine with
no credential store, and therefore **skip**. Supplying a real one is what turns those eight
tests back into coverage; the run that proves the skipping branch itself is the separate
"no secret service" step below, which takes the store away on purpose and asserts it is
gone. Installed as a job step rather than from the script: the suite
runs in a namespace with no route out, so nothing inside it can fetch a package. Starting
the daemon is the script's job, since it has to happen inside that namespace.

Windows and macOS need no equivalent: their stores are part of the OS.

**Bounded, because an unbounded one cost a whole run.** `apt-get` waits on a stalled mirror
for as long as the mirror likes: one run spent twenty-seven minutes inside `apt-get update`
and was killed by the job's own timeout, having proved nothing about anything. A per-request
deadline and three attempts is what a mirror having a bad minute deserves; a step timeout
well under the job's is what says *this* is what went wrong when it is having a bad hour.

**And `libnss3-tools`, which is T49b's whole starting measurement.** `certutil` is not
installed on a stock Ubuntu 24.04 and `libnss3` does not pull it in, so the round trip in
`crates/mixengine-platform/tests/browsers.rs` is `#[ignore]`d and this is the only leg that
runs it. About 2 MB, and in this step rather than one of its own so that the job holds one
`apt-get update` and not two.

**Test the answer a machine with no secret service gets.** T15b. Every other leg of this job hands the tests a store that works, on purpose — so the
answer a machine *without* one gets is a branch every green run steps past, which is how the
bug this step exists for lived long enough to be found by a stack trace on somebody's
console. This takes the store away in the three ways a Linux can lose it and asserts, rather
than skips: `MIXENGINE_TEST_NO_KEYRING=1` makes finding a store a failure, so a round whose
sabotage silently did not work says so instead of passing.

Outside the network namespace deliberately. The point here is which D-Bus the tests reach,
and the script builds a bus of its own to control exactly that.

**Test against a real certutil.** The suite that needs a real `certutil` — T49b. `#[ignore]`d rather than skipped, in the
shape every real-program suite in this job uses: a machine without `libnss3-tools` says the
test did not run instead of quietly reporting a pass, and this is the only leg that has the
package.

Outside the network namespace, deliberately: nothing here reaches a network, and the
database it writes into is one it made in a temp directory of its own — which is also what
keeps `docs/standards/testing.md`'s first rule, since no real browser profile is touched.

**Test.** Windows and macOS have no equally cheap isolation primitive, so there the rule is carried by
the test harness (T11) plus the offline cargo above.

**Under cargo-nextest (T170f)**, which runs every test as a process of its own with as many
at once as the runner has cores: `cargo test` ran the ~145 binaries one after another, and one
slow binary held up the rest (run 35430523430). The `ci` profile in `.config/nextest.toml`
carries what this step's flags used to: `fail-fast = false`, so a leg reports every failure it
found rather than the first, and a slow-test timeout that names a hung test instead of letting
it take the job's 45 minutes. Plain `cargo test` stays the contract: see
docs/standards/testing.md, rule 7.

**rustdoc.** rustdoc is the only tool that resolves intra-doc links, and nothing else in CI runs it:
clippy does not build the docs, and `cargo test --doc` compiles the examples without
checking a single link. `--document-private-items` because most of this workspace is
private — a link inside a `mod` that is never exported is exactly the kind that rots. It
does not make this a documentation-coverage gate: `missing_docs` still exempts private
items, checked by deleting one of their doc comments.

`rustdoc::all` is denied in the workspace manifest, so this fails on a developer machine
too; RUSTDOCFLAGS stays because it also covers the rustc warnings rustdoc's compile pass
raises, which no `[lints]` table can express.

Once per OS, not once: every OS directory in mixengine-platform is mapped onto `sys` by
`#[path]`, so a host-only run leaves two thirds of that crate unbuilt, and the broken link
that created T2a was in the macOS half. It lives in this job rather than in `lint` because
`--target` stopped being enough the moment T6 added a dependency that compiles C: rustdoc
does not link, but cargo still runs `libsqlite3-sys`'s build script, and cross-compiling
SQLite needs a toolchain for the foreign OS that no runner has. Reproduced before moving
it — `cargo doc --target x86_64-unknown-linux-gnu` from another host fails in cc-rs with
"failed to find tool x86_64-linux-gnu-gcc", not in rustdoc.

It runs even when a test failed. In `lint` this check was parallel with the tests; as the
last step of `test` it would be skipped by the first failing test instead, hiding every
broken intra-doc link until that test was fixed — a check that only runs when everything
else already passed is not much of a check. `success() || failure()` rather than
`always()`, which would also start it after the job had been cancelled.

**Not on Windows (T170d).** There it is a job of its own, `rustdoc`, because the Windows `test`
leg is the run's critical path and 2.5 minutes of it were this step. On macOS and Linux the
leg is short, and a job of its own there would cost one of macOS's five concurrent slots.

### `services`

**The suites that need a real program (T170e)**, split out of `test` so that a new package adds
a leg that runs in parallel rather than minutes to one that does not. The groups are a matrix
field, and every fetch and every suite below is guarded by one: a new suite joins a group, and a
leg past 15 minutes on a warm cache becomes a new row. Windows is split because it is the slow
system; macOS and Linux took 8.5 and 6.6 minutes for all of them in run 35430523430. The design
is docs/specs/2026-09-19-t170-a-test-job-that-scales-design.md, part B.

Every step keeps the guard it had in `test`, so a leg with no package for a suite still says so
rather than skipping it quietly.

**Fetch a real Caddy.** The one thing in this job that is downloaded rather than built, and the reason it is worth
it: `crates/mixengine-cli/tests/caddy.rs` is the only test in the workspace that judges a
recipe against the program it configures. Everything else about T31 is provable in one
process; that Caddy *accepts what MixEngine generates* is not, and a template with a Windows
path in it is exactly the kind of thing that is fine on two systems and broken on the third.

From our own signed index's release rather than from upstream, pinned, so this leg installs
the same artifact a user would. It is a **test fixture and not an install**: nothing here
checks a signature or a hash, because the code that does is `core::index` and `core::install`
and both have their own suites. What would break if the download were wrong is this test,
loudly.

Before the offline steps below, because from there on there is no network — and on Linux no
route out at all.

**Fetch a real nginx.** **And the other front end — T37.** The same reasoning as the Caddy step, plus one thing only
this program can settle: an `include` inside an nginx configuration resolves against the
*prefix* rather than against the file, so whether `include sites/*.conf` is judged in the
staging directory — which is what makes a broken rendering refused before it is installed —
is a question about nginx's own path handling and about nothing MixEngine can assert.

**The whole tree, not the binary.** A generated `nginx.conf` includes the archive's own
`conf/mime.types` by absolute path, so the fixture packs what was unpacked here.

**Fetch a real PHP.** **A second real program, and it buys what the Caddy one cannot.** Caddy is one binary that
behaves the same on all three systems; PHP is the opposite, and T32 is the task about that:
a Unix artifact publishes `php-fpm` and a Windows one publishes `php-cgi`, which are
different SAPIs configured by different mechanisms — a file on one, an environment on the
other. A recipe that renders correctly on both is not a recipe that runs on both, and only
the program can tell the two apart.

**Fetch the oldest PHP the module names have to satisfy.** And the oldest PHP on offer, which is the only one that can falsify a module name — the
branch above cannot. PHP 7.2 added a fallback that appends the shared-library suffix to an
`extension =` value it cannot open, so a *wrong* name loads anyway from 7.2 on; 7.0 and 7.1
hand the value to the loader verbatim. A suite pinned to 8.3 measured the spelling and could
not have caught it, which is what happened: five modules were silently absent from every PHP
7.0 this product installed, and the warnings were on every command.

**Fetch a real MariaDB.** And a real MariaDB, for T33 — the first recipe that has to *create* something before it can
run at all, and the one whose every platform difference is upstream's rather than ours: two
different `mariadb-install-db` programs, a bootstrap that refuses `SET PASSWORD`, and an
option parser that reads a backslash as an escape.

**A cost worth stating.** MariaDB is much larger than Caddy or PHP, and this adds a download
plus a real bootstrap to every `test` job on every runner. If it pushes CI past what is
tolerable, the answer is a separate job rather than a quieter test.

**Fetch a second MariaDB, of an older series.** And a *second* MariaDB, of another series, for T36 — two instances of one server, at two
versions, running side by side. 10.6 is the oldest line the index publishes and is chosen
for what it does not share with 11.4: upstream renamed every program the bootstrap runs
between the two, and their `share/` layouts differ. Two instances of one version would
share a `packages` row and prove only that two directories can have two names.

**The cost is a second download and a second bootstrap**, and it is why the suite that uses
it is its own step: if that becomes intolerable, the answer is a job of its own rather than
a suite that stops running two servers.

**Fetch a real MySQL.** And a real MySQL, for T34c — the other database with these programs' names, and not a
version of the one above. 8.4 is the line `docs/features/services.md` names; the two 5.x
lines take the other two bootstrap routes and are judged by `mixengine-packages`' own smoke
test on every published cell, which is where the artifacts that need them are made.

**There is no Windows-on-ARM cell in any MySQL line** — upstream has never published one and
nothing here compiles Windows — so that leg fetches nothing and the test step below skips
itself rather than failing on a download that could not succeed.

**Build the suites this leg runs.** Only this leg's suites, plus the binaries the harness spawns. **`--bins` is not optional**:
cargo builds a package's own binaries for its integration tests, so `--test caddy` gets `mix`,
but `mixengined` and `mixengine-shim` belong to other packages, and the harness finds them
beside `mix` or panics. In `test` the `--all-targets` build made them in passing; here nothing
would (the first run of this job failed on exactly that). With the workspace's features, for
T170b's reason.

**Test against a real connection.** T69's connection count, against a connection that really is established. `01`,
`MIB_TCP_STATE_ESTAB` and `-sTCP:ESTABLISHED` are three claims about three unrelated
mechanisms, and the captured tables the unit tests parse prove the parsing and say nothing
about the constant — so a socket somebody is on the other end of is the only thing that can
check any of them, and CI is where all three of those mechanisms exist.

**This job and not `system`**, which is what the suite's own note used to claim: counting
connections needs no administrative token, and `system` is the job for what an unprivileged
process cannot prove. What this needs is a real socket on each of the three operating
systems, and this is the job that runs on all three. `#[ignore]`d and given a step rather
than left to the workspace run, in the certutil step's shape: a leg where it did not run
says so instead of quietly reporting a pass.

On Linux outside the network namespace, deliberately. Inside it the tables hold this test's
own sockets and nothing else; picking one port out of the machine's whole table is the
harder question, and it is the one the idle sweeper asks.
**`--workspace --all-features` on every suite in this job, never `-p` (T170b).** Cargo unifies
features over the packages it was asked for, so `-p mixengine-platform` is a different feature
set from `Build tests`' and recompiled 43 crates to run a test that took 0.00 s (run
35430523430); `--test <suite>` still picks the one suite, in whichever member has it.

**Test what a shared site listens on.** What a *shared* site puts on the network, against the same real Caddy — roadmap task T76.
Its own step rather than a second `--test` above, for the reason every suite here has one: a
failure should name what failed without anybody reading the log.

It proves what is listening rather than what a firewall allows — every connection it makes is
to this machine's own address and so never crosses one — which is the half T74's first real
run found broken, with `netstat` and not with a firewall. The rule half is
`mixengine-core --test firewall`, on the Windows leg below.

Not on Linux, for the same reason the Caddy step above is not: that runner does its front-end
work inside a network namespace.

**Test against a real nginx.** And the other front end through the same arc — T37. Its own step rather than a second
`--test` on the line above, for the reason every suite below has one: a failure should name
which recipe failed without anybody reading the log. The two run the *same* sequence of
assertions from `tests/harness/frontend.rs`, which is what makes them a parity suite rather
than two files that resemble each other.

The guard on the variable is what would make a Windows-on-ARM leg green: there is no nginx to
fetch there and no upstream that publishes one.

**Test against a real MariaDB.** And the one that needs the MariaDB. Its own step for the reason the PHP one is: a failure
should name which recipe failed without anybody reading the log.

The Linux leg runs this from inside `test-no-network.sh` rather than here, and the
difference is the credential store: the bootstrap refuses a machine without one **by
design**, and that script is where a `gnome-keyring` is started on a session bus of its own.

**`--nocapture`, and a timeout of its own.** libtest holds a running test's output until the
test ends, so the first run of this suite on macOS reported twenty-seven minutes of silence
and then the job's own timeout. The suite says what it is doing as it goes; this is what
lets a reader see it, and the step timeout is what leaves the doc tests below enough of the
job to still run.

**Test two instances of one server, at two versions.** And the one that runs two of them at once — T36. Its own step rather than more of the one
above, because what it proves is a different claim and because it is the expensive half:
two installs, two bootstraps, two servers up together. Measured at fifty seconds on a
local Linux, against that suite's thirty-six.

The Linux leg runs this from inside `test-no-network.sh`, for the reason MariaDB's own
suite does: two first-run rituals, two generated root passwords, and a credential store
that only exists inside that script's session bus.

**Test against a real PostgreSQL.** And the one that needs the PostgreSQL — T34. Its own step for the reason the three above
have one: a failure should name which recipe failed without anybody reading the log.

The Linux leg runs this from inside `test-no-network.sh`, for MariaDB's reason: the ritual
puts the generated superuser password in the OS credential store and refuses a machine with
none, and that script is where a `gnome-keyring` runs on a session bus of its own.

**That the Windows leg runs this at all is T34a.** `postgres` refuses a token holding an
enabled `BUILTIN\Administrators`, which the step at the top of this job asserts this runner
still has; every supervised child is now created from a restricted copy of it —
`docs/decisions/0010-supervised-child-never-inherits-administrators.md`.

**Test against a real MySQL.** And the one that needs the MySQL — T34c. Its own step for the reason every one above has
one: a failure should name which recipe failed without anybody reading the log.

The Linux leg runs this from inside `test-no-network.sh`, for MariaDB's reason: the ritual
stores a generated root password in the OS credential store and refuses a machine with none.

The guard on the variable is what makes the Windows-on-ARM leg green: there is no MySQL to
fetch there, and a suite that is `#[ignore]`d until a real server exists has nothing to say.

### `system`

The job `docs/operations/build-and-release.md` says arrives with the first `#[ignore]`d system
test. T40 is that task: `mixengine-elevate` creates a root-owned audit directory outside
MIXENGINE_HOME, and nothing about that can be proved from an unprivileged process.

It runs on every run of this workflow rather than only on branches that touch `platform` or
`elevate`, which is what that table used to say: the triggers here are `push: master` and
`workflow_dispatch`, and a dispatch carries no diff to test a path filter against.

**35 minutes and not 20** since the certificate suites joined it. Those build the CLI and the
daemon — which this job had no reason to compile before — and then drive a real Caddy through a
rotation on each of the three systems. The number is the shape of the job, not a measurement of
one run: the steps that could hang rather than fail have clocks of their own.

**45 since T87's**, which adds one more suite that drives the CLI and the daemon and then takes
the machine apart — with a clock of its own, on the rule above.

**Build the port-access suite.** T42's port access, against each machine's own mechanism. The unit tests drive the codec and
the pf text against strings; what only a real machine answers is whether the kernel
recognises the attribute this writes, whether it clears it on a write, and whether pf
actually sends 80 to a server on 8080. Both tests take a copy of whatever they touch and
assert the machine came back as it was.

Windows has no leg: that system grants nothing, its whole answer is a refusal, and a refusal
needs no token — so it is asserted in the `test` job with everything else.

**Build the resolver suite.** T45's resolver wiring, against each machine's own mechanism. **A leg on all three**, unlike
port access: Windows has a real mechanism here rather than a refusal.

Every test that concludes anything from a name that did not resolve first proves its own DNS
server is answering — the T45 design, D14. That rule exists because four of the six
measurement rounds behind this task were void: the fake server was started in one step and
asked from the next, the runner killed it with the step, and nothing noticed until a control
was added. Which is also why the suite is one test rather than several sharing a server.

**Fetch a real Caddy.** T49a and T54's writes — the only tests in the workspace that touch this machine's own trust
store, and until now the only ones no job ever ran. `docs/standards/testing.md` rule 1
gates them on `MIXENGINE_SYSTEM_TESTS=1` **as well as** `#[ignore]`, because `#[ignore]` alone
is not enough: the `test` job runs the `caddy` suite with `--ignored`, and without the second
gate a rotation would install and remove a certificate authority on every Windows and macOS
runner in it. Nothing set the variable anywhere, so what the second gate bought was that a
rotation had never been measured at all — while T52, T53 and T54 are ticked `[x]`.

This is the job both designs name for them, and the job whose whole purpose is writing to the
machine. The Caddy comes down here rather than in the fetch block of `test` because this job
has no network namespace to get in ahead of; nothing below it is offline but cargo.

**The certificate authority this machine ends up trusting (Windows).** `--test-threads=1` for the hosts suite's reason, with the store in place of the file: there is
one trust store on this machine.

**The whole `caddy` suite and not the one gated test in it.** A `--exact` filter would be
precise today and silent the day somebody renames the test — cargo answers a filter that
matched nothing with `0 tests` and exit 0, which is the reporting-a-pass this job exists to
avoid. The four front-end tests it re-runs are not waste: they ran unprivileged in `test`, and
here they run under a token that can actually write, which is a different machine.

**The certificate authority this machine ends up trusting (macOS).** As root, in the elevated suite's shape, and **that is what makes this leg meaningful**: `do
shell script … with administrator privileges` on a process that is already root runs straight
through, which the launcher step above measured rather than assumed. So a rotation here is
granted, and the gated `caddy` test — which asserts `outcome == "rotated"` and then asks the
running server what it presents — has something to assert about.

`timeout-minutes` of its own for the launcher step's reason: if a prompt does appear after
all, nothing on this runner can answer it, and this is the clock that ends the wait.

**The certificate authority this machine ends up trusting (Linux).** **`cert` and not `caddy`, and root changes nothing about that.** A runner has no polkit agent
— which is not a gap in this job but ADR 0005's worst branch, asserted for real by the
launcher step above — so `probe()` answers `Unavailable` whoever is asking and a rotation is
never granted here. The gated `caddy` test requires `outcome == "rotated"`, so on this system
it could only ever fail; the four front-end tests beside it already run in `test`, inside the
network namespace. What Linux does answer is the refusal, and that is exactly what the `cert`
test asserts: replaced or left exactly as it was, and in neither case a candidate private key
left on disk.

**The daemon's autostart entry.** The daemon's autostart entry — T85b. **This job and no other**, because these tests register a
real logon task and a real systemd user unit: `docs/standards/testing.md` rule 1 gates them
on `MIXENGINE_SYSTEM_TESTS=1` as well as `#[ignore]`, and this is the job whose purpose is
writing to the machine.

A scratch name on both systems — `MixEngineSystemSuite`, `mixengine-system-suite.service` —
never the entry a person's daemon depends on, and each test removes what it made. macOS runs
nothing extra here: that leg writes one file and drives no tool (the T85b design, D8a), so its
whole cycle is already proved unignored in the `test` job against a directory it owns.

**The Linux leg asserts one of two things** and prints which: a runner with a systemd user
manager registers and reads back, and one without has to refuse with a reason naming the
command to run by hand. Neither branch passes by doing nothing.

`--test-threads=1` for the hosts suite's reason with the login configuration in place of the
file: there is one of it on this machine. As the ordinary user, deliberately: every mechanism
here is per-user and nothing about it needs root.

**Build the uninstall suite.** T87. **The clean-VM smoke test the roadmap asks for, and a fresh runner is the clean VM**: it
has never had MixEngine on it, so what the suite finds on the machine after a grant is what
MixEngine put there and nothing else.

**Last of the suites in this job, and that ordering is the whole of why it is here rather than
beside `cert`.** It removes the hosts block, the resolver wiring, the port grant, the
certificate authority, the privileged helper and the audit log; every suite above it would
then be running against a machine this one had already taken apart.

Built as the ordinary user and run as root on Unix, which is this job's standing arrangement.
`--test-threads=1` for the hosts suite's reason: there is one of each of these on a machine.

**The trust store this job left behind.** T49a's store, for the packet filter's reason and one more. This job's macOS leg is the only
place `security add-trusted-cert` and `remove-trusted-cert` are ever run, and the round trip
asks two different questions of them: whether the certificate is *in* the keychain, which is
what `TrustStore::probe` measures, and whether the machine actually *trusts* it, which is
what the operation is for. Those are not the same fact, and nothing before this printed
either of them — the first run of this suite hung for twenty minutes and produced no
evidence at all.

**What the uninstall suite left behind.** T87. What the uninstall suite left, which on a passing run is nothing — and on a failing one
is the whole answer. `always()` for the reason every step in this block has it: what an
elevated job left on a machine is worth printing whether or not the assertions about it held.

**Windows prints its pending-rename queue**, because there the helper cannot be unlinked while
it is running and what the suite asserts is that the operating system accepted the removal —
the T87 design, D8.

### `bench`

T29. The performance budgets from docs/standards/testing.md, of which the shim's is the first
to exist. It is a job of its own rather than a step in `test` for two reasons: these are
`#[ignore]`d and would otherwise be skipped there anyway, and they need a **release** build,
which is a second compilation of the workspace that no correctness answer should have to wait
behind.

On all three systems, unlike the single-runner row the ops document first sketched: the gate is
the same everywhere, but the hand-over it stands in front of is not one mechanism — Unix `exec`s
and Windows starts a child inside a Job Object — and the reported wall clock is the only place
that difference is ever written down as a number.

**Fetch the three servers M3 is about.** **M3's three servers**, fetched before the offline steps below because from there on there is
no network, through the script every fetch in CI uses (T172a).

The versions are the ones the `test` job's own steps pin, and they are pinned in two places
deliberately: a bench comparing itself against last month's number has to be measuring the
same programs, and a suite bumped by somebody else's step would move the number without
anybody deciding to.

**Fetch the three PHPs the cold path wakes.** **Three PHPs, and three different versions on purpose** — roadmap task T72a. The cold path is
measured three times because one CI measurement has misled this project before: the M3 bench
is bimodal on ubuntu, where a red has meant a bad minute rather than a regression. Three
rounds need three *cold* pools, and a pool is only cold once — so three pools, which means
three runtime installs, which means three versions.

7.0.33 is the floor this product offers, 7.4.33 is the legacy version people still run, and
8.3.33 is what the `test` job pins. **The first two predate `pm.status_listen` entirely**, so
this is also the standing proof of T72a's decision to read a pool's status page off its own
socket: the day somebody reaches for the cleaner arithmetic, two thirds of this measurement
go red rather than a paragraph being disbelieved.

A directory each, because the script's default would have them overwrite one another, and
`MIXENGINE_PHP_RUNTIMES` is the list the suite reads — separated the way this system
separates a PATH.

**Install a secret service.** T33's requirement, seen from the job that is not `test`: the MariaDB bootstrap puts the
generated root password in the OS credential store and refuses a machine with none. Windows
and macOS have one in the OS. On Linux this installs it, and the measuring step below wraps
itself in a session bus of its own — the smaller half of what
`.github/scripts/test-no-network.sh` does, rather than teaching that script a second profile.

The network namespace that script also sets up is deliberately not reproduced: this suite
talks to a `MockRegistry` on loopback like every other, and the isolation the `test` job adds
is a belt the `bench` job has never worn.

**Build the program the shim is measured against.** `fakeservice` is the program the shim is measured in front of, and it is a **binary of a
dev-dependency crate**: selecting one test target does not build it, and a release copy left
by an earlier build is used as it is. A stale one is what this step exists for — it was
found by the benchmark's own check that the fronted program really ran, having been built
before that program learned `--version`, and a benchmark that measures a binary nobody
rebuilt is a benchmark measuring last month.

**Idle footprint.** **The idle footprint**, and it runs *before* M3 deliberately — `features/resource-isolation.md`'s first published number, and the
third thing this job gates. Its own step rather than another `--test` on a line above, on
this job's rule: a failure should name what failed without anybody reading the log.

It needs the Caddy fetched above and nothing else — one daemon, one web server, and nothing
running. It spends thirty seconds settling before the first reading, deliberately: what is
measured is a home doing nothing, and a reading taken the instant a start walk returned would
measure the walk.

**No `dbus-run-session` wrapper**, unlike the M3 steps: nothing here starts a MariaDB, so
nothing here has a password to put in a credential store.

**Ahead of M3 because a failing step ends the job.** This one takes forty seconds and
needs one server; M3 starts three of them eight times over and is bimodal on ubuntu. The
first run of this budget was skipped on that runner for exactly that reason, which is a
measurement lost to somebody else's bad minute.

**Cold path.** **The cold path**, `features/resource-isolation.md`'s second published number and the fourth
thing this job gates — roadmap task T72a. Its own step for this job's rule: a failure should
name what failed without anybody reading the log.

**Ahead of M3 for the idle footprint's reason**, and it needs the reason more: a failing step
ends the job, and this one waits a full minute for the sweeper before it can measure
anything. Losing that to somebody else's bimodal warm start would cost the whole
measurement.

It needs the Caddy and the three PHPs fetched above and nothing else — no MariaDB, so no
`dbus-run-session` wrapper and no credential store on Linux.

**What it waits for is a sweep, not a clock.** The pools must be stopped by the sweeper and
not by a person, because a service somebody stopped is one the activator deliberately refuses
to wake (T70's D8). An idle policy is a whole number of minutes, so the wait is about ninety
seconds and is paid once for all three rounds.

**What the tuned defaults save.** **What the tuned templates save** — roadmap task T73, and the fifth thing this job measures.
Two MariaDB instances in one home, one on the recipe's defaults and one put back to the
server's own, started in turn and read through the daemon's own sampler.

**What is gated is the difference between them**, never either number on its own: an
absolute budget on MariaDB's RSS would be a promise held hostage to next month's MariaDB, on
a quantity this project does not control. The suite's module note argues it in full.

**Ahead of M3 for the idle footprint's reason** — a failing step ends the job, and M3 is
bimodal on ubuntu, where a red has meant a bad minute rather than a regression. Behind the
cold path because this one bootstraps two databases and that one waits out a sweep; neither
should be paying for the other's bad luck.

It needs the MariaDB fetched above, and a credential store with it: the bootstrap generates a
root password and refuses a machine that cannot hold one. Windows and macOS have one in the
OS; Linux gets the same `dbus-run-session` wrapper the M3 step below uses.

**M3 — three services, warm.** **M3**, which is `docs/features/services.md`'s one number about starting things: caddy,
mariadb and redis healthy in under ten seconds warm. Its own step rather than another
`--test` on the line above, for the reason every real-server step in the `test` job has one:
a failure should name what failed without anybody reading the log.

`--nocapture` because the numbers are the output. A timeout of its own because this starts
three servers eight times over and a hang inside libtest prints nothing until it ends — the
suite carries a watchdog of `mariadb.rs`' kind for that, and this is the outer bound.

Windows and macOS run it directly; Linux wraps it in a session bus with a `gnome-keyring` on
it, because the MariaDB bootstrap has a password to store and refuses a machine that cannot.

### `bindings`

T56, and the fifth job of the table in docs/operations/build-and-release.md: the published
TypeScript contract, regenerated from `mixengine-proto` and compared with what is committed.
Until this existed, a `ts-rs` type whose committed output had drifted was caught by a person or
by nobody.

**Ubuntu alone.** Generation is OS-independent — measured, and the `test` job runs the generator
on all three every run, because it passes `--all-features` and the exporter is a `#[test]`. So a
second and third runner here would re-measure one behaviour at twice the cost, which is the
reasoning T86a's `windows-latest` probe uses one job along.

### `docs`

T90: the user handbook. The corpus's own invariants — parity between the two languages,
resolvable links, a translation that was revisited after its source changed, prose that is
wrapped — are `cargo test` and therefore run in `test` on all three runners with no step here.
That is T89's rule: a suite that needs nothing downloaded and no privilege arrives without a job.

What needs a job is the part that is not a test: building the site, which compiles a Markdown
renderer, and holding the committed command reference against what `mix` prints — the same shape
as `bindings`, for the same reason. A red job here names which of the two broke.

**Ubuntu alone**, again for `bindings`' reason: the generator reads embedded strings and writes
files, and a second and third runner would re-measure one behaviour at twice the cost.

### `desktop`

T85, and the sixth job of the table in docs/operations/build-and-release.md: the artifacts a
person downloads. Three runners and host architecture only — the second architecture on Windows
and Linux is a cross-compilation of a workspace that builds SQLite, AWS-LC and libdbus from C on
runners that carry no cross toolchain, and it is roadmap task T85a rather than a flag added here.
macOS is the exception and is universal, because Apple's toolchain builds the other slice with no
extra sysroot.

**Nothing here is signed.** Authenticode and an Apple Developer ID are not purchased (ADR 0005),
and the minisign signature that actually protects an update is T86's. Each script writes a
`.sha256` beside its artifact, which is not the same thing and is not offered as one.

Each script also opens what it just made and checks the five binaries are in it — four for the
headless archives T105 added, which additionally refuse a webview they find. That is where this
job's real assertion lives: an empty archive is a perfectly valid archive, and a `.deb` with no
helper in it installs cleanly and leaves the machine one file short of being able to elevate.
T103, ADR 0027: the desktop application under apps/desktop. Its frontend's build, tests and
lint, then its own Cargo workspace's clippy and tests — on one OS, because nearly all of it is
pure logic (the standalone client's own CI made the same choice), and `build` is what proves the window links
on all three. It gates `release` the way `bindings` does, and for the same reason: a type
reshaped in `mixengine-proto` fails `tsc` here, in the same run — the check ADR 0011 gave up.

`cargo audit` is here rather than on a schedule of its own: this workflow only runs when
somebody asks, so an advisory published overnight turns red at the moment somebody is looking.
Its documented ignores are `apps/desktop/src-tauri/.cargo/audit.toml`'s.

### `window`

**`build` is three jobs per leg since T171**: `window` and `binaries` are the two release builds,
neither of which reads anything the other writes, so each gets a runner of its own and they run
at the same time; `build` then only packages what they handed on. T170's C2 ran the same two
builds side by side on one runner and was slower, because they split its CPUs — two runners is
what that measurement left. docs/specs/2026-09-20-t171-a-build-that-fans-out-design.md.

The three matrices are the same five rows and must stay so: `build` downloads by `matrix.os`.
Actions has no YAML anchors, so the rows are written three times and the comments once, here.

**T173a: off a tag this builds at `opt-level = 0`**, the third variable `release-profile` writes
after LTO and codegen units. The whole `build` group fell from 111.2 leg-minutes to 77.1, and this
leg from a 17.5–18.5 band to 9.1 in a full run and 7.1 in a `jobs=build` one. Level 1 was measured
first and moved only `binaries` (about 9%), never `window`: `mixlab` is one crate of 306 seconds'
codegen, and level 1 barely touches it.
docs/specs/2026-09-20-t173-a-window-that-builds-in-under-ten-design.md.

The saving is not free and the price is measured: a branch's installers grew by 16 to 76%, and
the five packaging legs pay 5.5 leg-minutes more to tar and upload them. A `window-<os>` somebody
downloads to try the application is now an unoptimised build.

**What the `--timings` report settled (T173c).** The report is behind the `MIX_TIMINGS` repository
variable — `gh variable set MIX_TIMINGS --body 1`, dispatch, `gh variable delete MIX_TIMINGS` — so
asking costs no commit. It said the tail of this job is `mixlab`'s own codegen, 306.3 s, against
4.4 s to link it, which is why `rust-lld` was dropped rather than adopted: there is no link time
for a faster linker to take. What is left is 624 s of dependencies before `mixlab` starts, and
`mixlab` itself, whose front end is single-threaded — both belong to `apps/desktop/src-tauri`'s
dependency list, not to CI.

**T173b: Defender steps aside on the Windows legs** (`.github/actions/defender-aside`), because a
release build writes tens of thousands of object files and each is read again as it lands. A
refusal is a `::notice::` and never a failure: `Add-MpPreference` needs a privilege the runner may
not grant, and an exclusion that silently did not apply would make the next measurement of this job
a lie.

**T175: what the window compiles, and two things it turned out to need.** The `--timings` report
(above) named `image` and `moxcms` — 99 seconds — entering through `tauri-plugin-clipboard-manager`
→ `arboard`, whose default `image-data` reads clipboard *pictures*. This application reads one
thing from that plugin, the text the terminal pastes, so the plugin was replaced by `arboard`
itself with `default-features = false` and one command of our own. Measured after, on run
35495658721: 898 units against 908, and neither crate in the report.

Two cuts beside it were tried and refused, and both are worth knowing before somebody tries again:

- **`mongodb` keeps its default features.** `compat-3-0-0` is not a spare copy of bson — it is what
  makes `mongodb::bson` *be* bson 2, which 23 call sites in `modules/db/drivers/` are written
  against. Removing it is a driver migration that touches how an archive is written, not a
  dependency tidy-up.
- **`modules/db` stays in the `mixlab` crate.** It is 28,786 of 34,000 lines but **47% of the
  compile**: 88.4 s whole against 47.0 s with it unhooked. Two halves that size do not pipeline
  into the two minutes that would have paid for a third crate and a cut through the `db` ↔ `launch`
  cycle. The same split would halve a *local* rebuild, which is a different argument for a
  different day.

docs/specs/2026-09-20-t175-dependencies-that-earn-their-place-design.md.

**This leg is bimodal, so one run proves nothing about it.** Eleven samples before T173 ran 18.5,
18.5, 18.4, 18.3, 18.3, 18.2, 18.2, 18.0, 17.5, 15.0, 13.8 — the same commit on a fast or a slow
host, four and a half minutes apart. Read a small change on `binaries (windows-latest)`, whose band
is 10.4–11.6, and accept this leg as evidence only when a change leaves the band entirely.

T85a, D2: the four binaries are built in a manylinux_2_28 container so the artifact links
an older glibc than this runner ships, rather than whatever `ubuntu-latest` happens to
carry this month. Same architecture as the runner in both rows below — never
cross-compilation, only an older sysroot. The tag is pinned and dated for the reason every
other toolchain in this product is: "whatever the runner has" is not reproducible.

T103: the window is built on the host of these two legs, not in the container — the
manylinux image is AlmaLinux 8, whose WebKitGTK is the 4.0 API on libsoup 2, and Tauri 2
links 4.1 on libsoup 3. The container step does not care what its host runs, so the host
is pinned to 22.04 to give the window the glibc 2.35 floor the standalone client's own releases had.

### `binaries`

**Smoke test this leg's toolchain.** T85a, D8 wanted a toolchain that is broken on a new leg to fail in under a minute rather than
sixty minutes into a packaging run, and it built `mix` on its own first to get that. Since
T171 this job *is* the build and packaging is another job, so there is nothing long to fail
ahead of. What is left worth asking is whether this runner's toolchain makes a binary that
runs — asked of the one just built, rather than of a second compile of `mix` without
`crt-static`, which cost 89 s.

### `build`

**Install the packaging tools this runner is missing.** `rpmbuild` is not on the ubuntu image. Installed rather than made optional: an artifact that is
quietly not built is a release that is quietly missing one.

**Install the packaging tools this runner is missing.** **NSIS is not on the windows image**, measured: the first run of this job got through the
release build and the portable zip and then stopped at "missing tools: makensis". `7z` and
`unzip` *are* there, which is why only this one is installed.

Chocolatey puts it at `C:\Program Files (x86)\NSIS`, which is the path
`packaging/windows/build.sh` looks at first.

**What an unsigned release shows SmartScreen.** T86a. What an unsigned release looks like to the machines that judge it, measured on the
artifacts this leg just built: the Mark-of-the-Web and quarantine attributes SmartScreen and
Gatekeeper actually read. Neither dialog can be seen from here — those two readings are a
person's, on the draft release, and are release-checklist item 5.

**`windows-latest` alone.** Mark-of-the-Web is not architecture-dependent, so the arm64 leg
would re-measure one behaviour on a different file at twice the runner cost — the design, D12.

`MIX_PROBE_INSTALL` is set here and in no other place. The readings behind it install for
real: the NSIS installer into a temporary directory and this account's `PATH`, and on macOS
the `.pkg` into `/usr/local/bin` and `/Library/PrivilegedHelperTools` as root, because there
is no `-target` that isolates that. Both probes put the machine back as they found it, and
the macOS one refuses outright if MixEngine is already installed on it.

Each probe *fails* on a statement about our own artifact that came back wrong — an installer
that started propagating a mark, a package that acquired a signature nobody bought — and
records a *void reading* for anything this machine could not answer. So a red leg here is a
change to the release story, and a green one that measured nothing says so in its report.

### `release`

T86, D4. Signing happens once, on one runner, and this is it. The five build legs are untouched:
the secret would otherwise reach five jobs, and `minisign` has no official build for the arm64
Windows runner.

`needs` deliberately omits `system` and `bench`. Both run on the tag and a person reads them, but
neither gates: `bench` is bimodal on ubuntu, and a release blocked by somebody else's bad minute
is a release process nobody trusts. What gates is "the repository is consistent", "the code is
correct on three operating systems", and "the artifacts were built".

**Pack the published API contract.** T56. The API contract, packed from the committed tree — which is current because this job
needs `bindings`, rather than because a second generation happened here. Before the signing
step, like the feed below, because being in this directory is how a file gets signed;
`feed.sh` does not pick it up, since it matches a payload by the
`mixlab-<version>-<os>-…` shape and a helper by `mixengine-elevate-<version>-…`, and this
is neither.

**Assemble the draft release.** **A draft, and a person publishes it.** T88's feed lives at `releases/latest/download/`, and
that URL must not move because somebody pushed a tag to see what would happen; T86a has to
watch a real download's SmartScreen behaviour and needs a moment to be ready for it; and
publishing to the world is not the same deliberate act as tagging.

Notes are generated from the commits and are a starting point: the draft exists to be edited.
The unsigned-binary warning is prepended ahead of the generated list rather than left to
CHANGELOG.md, because a release page is what a downloader actually reads before running
SmartScreen or Gatekeeper past its warning.
