# 0033. `<root>/bin` is a projection of what is installed, not of a compiled constant

**Status**: Accepted; point 1 of the decision, and its poll, superseded by [0057](0057-a-runtime-s-commands-are-in-bin-only-while-it-is-installed.md)
**Date**: 2026-09-15

## Context

`<root>/bin` is the only directory MixEngine puts on a person's PATH, and until now its contents
were `shims::COMMANDS` — a compile-time constant naming four languages and Composer. Nineteen files,
the same nineteen on every machine, and a refresh removed anything else it found there.

That constancy was a deliberate and documented property. `shims.rs` argues it directly: `bin/` holds
`node` on a machine with no Node.js, because a shim that says *which command to type* is a better
answer than `node: command not found` from a tool whose job is managing versions of Node.

Two complaints from somebody using the finished product showed what the constant cannot express.

**A database has clients.** Every service package already records its executables — MariaDB's
`mariadb-dump`, Postgres' `psql`, Redis' `redis-cli` — in `packages.provides_json` at install time.
None of them was a command. A row in `COMMANDS` could not fix it either, because a service has no
version resolution: `resolve::runtime` walks up from the working directory, and a database answers
to *instances* instead, each with its own package version and its own port.

**A globally installed tool is not a command.** Measured rather than reasoned about: `npm config get
prefix` inside a MixEngine Node answers the Node install's own directory, so `npm install -g yarn`
writes `yarn.cmd` into `runtimes/node/24.19.0/` — a directory that is on nobody's PATH. A `yarn` put
into `bin/` by hand was deleted by the next daemon start, because `bin/` removes what no command
names.

Design:
[docs/specs/2026-09-15-t130-what-a-terminal-inherits-design.md](../specs/2026-09-15-t130-what-a-terminal-inherits-design.md).

## Decision

**`<root>/bin` is a projection of installed state.** Three sources compose it, and the compiled table
stays first among them:

1. `shims::COMMANDS`, unchanged and still independent of what is installed.
2. The client commands each `Recipe` declares, for the service packages this home has installed.
3. The tools a scan finds in each installed runtime's own bindir, recorded in `bin_commands`.

A service client resolves through its **instance** — which decides both the binary and the endpoint
it is handed — and a discovered tool resolves through its **kind**, the same way `npm` does. A name
two installed packages both claim is settled by a total order that ends in the package name, and the
losers are reported rather than hidden.

## Consequences

**The directory's contents now change with an install.** `mysqldump` appears when a database is
installed and goes when it is removed. That is the point, and it is also the property `shims.rs`
previously argued against having — the argument still holds for *runtimes*, where a shim has a
useful sentence to say, and does not hold here: `mysqldump` on a machine that has never had a
database is a name nothing would ever make work.

**Names MixEngine did not choose are now on somebody's PATH.** A globally installed npm package
called `git` puts a `git` ahead of the machine's own. Every version manager that fronts global tools
has this property. MixEngine's own binaries are refused outright (`globals::RESERVED`) and
`mix doctor` reports the rest; refusing more would be deciding on somebody's behalf that a tool they
installed is not one they meant.

**The thing that fills `bin/` has to read the database.** `daemon::shims::Shims` gained a `Store`, a
`Catalogue` and a mutex — three callers now fill the directory instead of one, and a refresh sweeps
before it writes.

**A short poll runs while the daemon does.** Two seconds by default, one `stat` per installed
runtime, and nothing at all when no bindir has moved. `[bin] rescan_seconds` is the key, and
`mix path rescan` is the way to not wait for it.

**A name can be left behind for a moment.** Between an uninstall and the next refresh, `bin/` holds a
shim nothing claims. It answers with a sentence naming itself rather than the old "this is a
MixEngine shim" — which was written for a different situation and reads as a bug in this one.

## Alternatives considered

**Put each runtime's own directory on the PATH.** Rejected, and it is the rejection that shapes
everything above: a global tool belongs to *one version of one language*, so a PATH entry pointing at
one install makes `cd`-ing into a project that pins another silently run the wrong program — the
exact failure the shim exists to prevent.

**A filesystem watcher instead of a poll.** Rejected for now. It would react a little sooner and
would cost a dependency whose behaviour is a different program on each of the three systems, with
its own queue, coalescing and failure modes to reason about on every one of them. What is being
watched is a handful of directories, and the question asked of each is one `stat`.

**Front only names nothing else claims, and refuse a contested one.** Rejected: a name a person
expects and cannot type is the complaint this task opens with, and an arbitrary-but-stable winner
that `mix doctor` names is strictly better than a hole.
