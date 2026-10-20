---
status: implemented
date: 2026-10-21
task: T209d
---

# One daemon harness for every integration suite (design)

Thirteen suites in `crates/mixengine-daemon/tests/` start `mixengined --home <home>` with their own
`struct Daemon(Child)`, and eleven of them carry their own `Client` with `connect`, `call` and `ask`
(seven add `refuse`). The copies are the same down to the line, and they cite each other by name
("on `tests/sites.rs`' reasoning") because there was nowhere to write the reason once. They are
copies because `env!("CARGO_BIN_EXE_mixengined")` reaches only a test of the package that builds the
binary (`docs/standards/testing.md`, *Fixtures*), so `mixengine-testkit` could not spawn the daemon
itself. It does not have to: the suite passes the path in.

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209d.

## What was read

Measured on `master` at `d210bb04`.

**Starting the daemon.** Every copy below is `Command::new(env!("CARGO_BIN_EXE_mixengined"))`,
`--home <home>`, stdout and stderr to null, `spawn().expect("the daemon binary runs")`.

| suite | extra arguments | stderr | `Drop` |
| --- | --- | --- | --- |
| `elevation.rs` | – | null | kill |
| `extension_sites.rs` | – | null | kill |
| `extensions.rs` | `--index-url`, `--index-key` | null | kill |
| `idle.rs` | – | null | kill |
| `limits.rs` | – | null | `end_daemon` |
| `logs.rs` | – | null | kill |
| `metrics.rs` | – | null | kill |
| `packages/main.rs` | `--index-url`, `--index-key` | null | kill |
| `projects.rs` | – | null | kill |
| `renewal.rs` | – | null | kill |
| `revoke.rs` | – | null | kill |
| `runtimes/main.rs` | `--index-url`, `--index-key` | appended to `<home>/daemon.stderr` | kill |
| `sites.rs` | – | null | kill |

`crates/mixengine-cli/tests/harness/mod.rs` has the fourteenth: a `Daemon(Child)` with `pid` and
`wait_until_gone`, ended by `end_daemon`, spawned from `daemon_binary()` (the `mixengined` next to
`mix`) with arguments of the caller's, or with variables removed from its environment. It is already
shared by every CLI suite. Only its spawn and its `Drop` are a copy.

**Talking to it.** Eleven suites carry the same `Client`: one HTTP/1.1 connection over
`mixengine_platform::ipc::Connection`, the driver spawned, and `ask` posting a JSON-RPC body to
`/rpc` with `host: mixengine`, waiting for `sender.ready()`, asserting `200 OK` and parsing the body.
`call` asserts no `error` and returns `result`; `refuse` asserts no `result` and returns `error`.
The differences:

- `renewal.rs`, `revoke.rs` and `logs.rs` have no `ask`: `call` does the whole request.
- `idle.rs` and `metrics.rs` spell it `open` and `rpc`, and do not wait for `ready()`, although
  `idle.rs` sends three requests over one connection: the race `logs.rs` found and wrote down.
- `runtimes/main.rs` keeps the home's paths and, when a request gets no answer, panics with
  `daemon.stderr` and `daemon.log` attached.
- `logs.rs` and `metrics.rs` also send a `GET` over the same connection and read the answer as a
  stream.

`mixengine_testkit::call` (`create.rs`) is a different thing and stays as it is: one call over a
connection of its own, for a suite with no client.

## D1 — `mixengine_testkit::Daemon`

A new `crates/mixengine-testkit/src/daemon.rs`:

```rust
/// A `mixengined` this test started and is still holding, ended when it drops.
pub struct Daemon(Child);

impl Daemon {
    /// `binary --home <home>`, stdout and stderr to null, not yet spawned: for a suite that adds
    /// arguments, redirects stderr or removes a variable before it starts.
    pub fn command(binary: &Path, home: &Home) -> Command;
    /// Spawn what `command` built.
    pub fn spawn(command: Command) -> Self;
    /// `spawn(command(binary, home))`, the case thirteen suites have.
    pub fn start(binary: &Path, home: &Home) -> Self;
    /// End it the way a crash does: killed, no shutdown run.
    pub fn kill(self);
    /// The process it runs as.
    pub fn pid(&self) -> u32;
    /// Wait up to `SHUTDOWN` for it to end by itself, and say whether it did.
    pub fn wait_until_gone(&mut self) -> bool;
}
```

Each suite passes its own `env!`: `Daemon::start(env!("CARGO_BIN_EXE_mixengined"), &home)`, the
path taken as `impl AsRef<Path>` so no `Path::new` is needed. Nothing waits for the endpoint
inside `start`, because the suites do not agree on when to: most call `home.wait_until_listening()`
right after, `idle.rs` and `limits.rs` declare services first, and the CLI harness waits through
`mix status`. The `Fixture` structs that pair a `Home` with a `Daemon` stay in their suites, with
their own fields.

`pid` and `wait_until_gone` come from the CLI harness, whose `Daemon` becomes a re-export of this
one. `Home::wait_until_gone`, which watches the endpoint, is a different question and stays.

**My call:** a `Command` to adjust rather than a builder or a `start_with(args)`. The three
variations found (index flags, stderr to a file, `env_remove`) are all things `Command` already
says, and a builder would restate them.

## D2 — one `Drop`, and it is `end_daemon`

Every `Daemon` ends the way `limits.rs` and the CLI harness end theirs today:
`mixengine_testkit::end_daemon`, which asks the daemon to stop, waits up to `SHUTDOWN`, and kills it
only if it is still there. The other twelve kill at once.

This is the one change in behaviour this task makes, and it is in teardown only. The reason is
`end_daemon`'s own: on macOS a killed daemon leaves the services it started running, reparented to
`launchd`. `idle.rs` and `logs.rs` start services and kill their daemon today, so they can leave
`fakeservice` processes behind on a Mac. The cost is the time a daemon takes to stop when asked,
which is its normal shutdown, paid once per test that is not already ending it.

`lifecycle.rs` is the exception and keeps its kill (see *Not in scope*). Inside the suites that
moved, two tests end their daemon mid-test on purpose, to see what the *next* start does:
`renewal.rs`' `a_start_issues_for_a_site_whose_certificate_is_missing` and `revoke.rs`'
`a_deadline_that_passed_while_the_daemon_was_down_is_acted_on`. A stop that ran its shutdown would
be a second thing each depends on, so they call `Daemon::kill` and keep the kill they had.

## D3 — `mixengine_testkit::Client`

A new `crates/mixengine-testkit/src/client.rs`, the eleven copies made one:

```rust
/// One HTTP/1.1 connection to a home's daemon.
pub struct Client { /* sender, and the home's paths for a failure message */ }

impl Client {
    pub async fn connect(home: &Home) -> Self;
    /// `POST /rpc`, and the whole JSON-RPC response.
    pub async fn ask(&mut self, method: &str, params: Value) -> Value;
    /// `ask`, insisting on a `result`, and returning it.
    pub async fn call(&mut self, method: &str, params: Value) -> Value;
    /// `ask`, insisting on an `error`, and returning it.
    pub async fn refuse(&mut self, method: &str, params: Value) -> Value;
    /// Any other request over the same connection, its answer unread: for `GET /logs/…` and the
    /// metrics stream.
    pub async fn send(&mut self, request: Request<Full<Bytes>>) -> Response<Incoming>;
}
```

Every request waits for `ready()`, the comment that says why is written once here, and `idle.rs`
and `metrics.rs` get the wait they were missing. A request with no answer panics with the home's
`daemon.log` and, when the file exists, `<home>/daemon.stderr` attached, which is what
`runtimes/main.rs` alone does today. `Home` gets a `daemon_stderr_file()` naming that file, and
`runtimes/main.rs` points its stderr at it with `Daemon::command`. No other suite writes it, so for
them the message gains the log and nothing else.

What a suite adds on top of a client today (`runtimes`' and `packages`' `install` and `finished`,
`extension_sites`' `finished`, `logs`' stream, `metrics`' frames) becomes a private trait in that
suite implemented for `Client`: a suite cannot add inherent methods to a type of another crate, and a
trait leaves every call site as it was. The three suites that pass index flags each keep one
`started(home, registry)` that builds them with `Daemon::command`.

**My call:** the failure message attaches the log for every suite rather than only for `runtimes`.
It changes what a failing test prints, never whether it fails.

`mixengine-testkit` already depends on `hyper` (client, http1), `http-body-util`, `hyper-util` and
`tokio` with `rt`, so neither module adds a dependency.

## D4 — what the change must not alter

- **Test counts, per test binary.** `cargo test -p mixengine-daemon -p mixengine-cli -- --list`
  is recorded before the first commit and compared after the last; the lists are equal name for
  name.
- **Arguments and environment.** Each suite starts the daemon with the arguments, in the order, and
  with the environment it uses today; only the `Drop` differs (D2).
- **No suite keeps a copy.** After the change, in `crates/*/tests`:
  - `CARGO_BIN_EXE_mixengined` appears only as the argument handed to `Daemon`, and in the places
    *Not in scope* names;
  - `impl Drop for Daemon` and `http1::handshake` appear nowhere outside those places.
- **Size.** Every suite gets shorter. `daemon.rs` and `client.rs` are each well under 200 lines.
- No comment written by this task cites a roadmap task number (`check-task-citations.mjs`).

There is no test of `Daemon` or `Client` inside `mixengine-testkit`: its own suites cannot reach
`mixengined` for the same reason this task exists. The thirteen daemon suites and the CLI suites are
the test, and they run on all three systems in CI.

## D5 — commits

One branch, `t209d-one-daemon-harness`, and one pull request. Each commit builds and passes:

1. `daemon.rs` and `client.rs` in `mixengine-testkit`, `Home::daemon_stderr_file`, the exports in
   `lib.rs`, and `sites.rs` switched to them.
2. Eight more suites in `mixengine-daemon/tests/`, and `Daemon::kill`.
3. `logs`, `metrics`, `packages` and `runtimes`, whose clients did more than call.
4. The CLI harness's `Daemon` replaced by the testkit's, its spawn through `Daemon::command`.
5. `docs/standards/testing.md`, *Fixtures*, gets a bullet for `Daemon` and `Client` saying why the
   binary is an argument. The phase file ticks T209d, and this spec becomes `implemented`.

## MixLab

None. This changes how the test suites start and talk to `mixengined`, nothing in the daemon, the
API or the window. No API method is added, no screen changes, and there is no CHANGELOG line.

## Not in scope

- **`lifecycle.rs`'s `Foreground`.** It is not a copy of the harness: it runs the binary with
  arguments ahead of `--home`, holds a `--detach` that is not the daemon, and kills on purpose,
  because the graceful path is what its tests exercise. Its `run`, which reads a process to its end,
  stays too.
- **The `api` suite's `Daemon`.** It owns a bare `TempDir`, not a `Home`, so its daemon starts
  without the `[dns] port = 0` seed every `Home` writes; it waits sixty seconds rather than
  `STARTUP`'s thirty, after a measured CI run; and it opens one connection per request on purpose.
  Moving it onto `Home` would change the daemon it tests.
- **`elevation.rs`'s moved binary.** One test copies `mixengined` into a directory of its own and
  starts that copy, to prove what a daemon with no helper beside it answers. Where the binary sits
  is the thing under test.
- **`storage.rs`'s `run` and `storage_report`.** They run the binary to its end (`--detach`,
  `--storage`) and read its output; nothing is held, so there is no harness to share.
- `self_update.rs`'s `Running`, and the shim, elevate and trampoline harnesses: other binaries.
- Making the suites' `Fixture` structs one. They hold different things (a registry, a configured
  home, services declared before the start), and a shared one would need options for each.

## Is there an ADR here? No

`mixengine-testkit` already holds the fixtures the suites share, and stays a dev-dependency only. No
boundary between crates changes.
