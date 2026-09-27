# Phase 15 — What a terminal inherits

*Goal: a terminal opened inside a MixEngine home can open the databases it has installed, run the
tools it installed into a runtime, and reach its own HTTPS sites.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-15-t130-what-a-terminal-inherits-design.md](../specs/2026-09-15-t130-what-a-terminal-inherits-design.md).

---

**Three complaints from somebody using the finished product, and one question underneath all three:
what does a terminal inherit from MixEngine?** The answer was nineteen file names and two
environment variables, and each complaint is a different edge of that being too small.

> `bin` thiếu `mysql`, `mysqldump`, … các bin của database, redis, …
>
> `npm -g install yarn`, sau đó lệnh `yarn` không gọi được
>
> nodejs app, call https từ 1 site của mixengine báo lỗi `FetchError: … unable to verify the first
> certificate`. website này lúc đó vẫn mở trên trình duyệt bình thường

None of the three is a bug in what exists. `<root>/bin` was a projection of a compile-time constant
and did exactly that; npm's global prefix is the Node install's own directory and nothing puts it on
a PATH; and MixEngine installs its authority into the operating system's trust store, which is what
a browser reads and no language runtime does. What each one is, is a place where MixEngine had never
said anything.

Two decisions came out of it:
[ADR 0033](../decisions/0033-bin-is-a-projection-of-what-is-installed.md) — `bin/` becomes a
projection of *installed state* — and
[ADR 0034](../decisions/0034-mixengines-authority-reaches-a-runtime-through-a-generated-bundle.md) —
a runtime is handed a bundle of this machine's own roots plus MixEngine's.

## The commands a database brings with it

- [x] **T130** The client commands of a service *(P)*. `Recipe::clients` and `Recipe::client_env`,
      declared by the four database and cache recipes; `core::services::client` for which install a
      client runs out of; `shims::refresh` takes an extra list, and `shims::resolve_claims` settles
      a name two installed packages both want.
      **A client belongs to an instance, not to a version**, and that is the whole shape of it: a
      runtime resolves per directory and a database cannot, because it has instances with their own
      versions and their own ports. The instance decides both the binary and the endpoint — and the
      endpoint is not a nicety, since the port allocator gives 3306 to whichever of MariaDB and
      MySQL was created first, so a bare `mysql` told nothing would open a session on the other
      product's server and report success.
      **MariaDB answers to `mysql`, `mysqladmin` and `mysqldump` too**, and drops all three the
      moment the MySQL package is installed beside it: a real name always beats a spelling. Every
      supervised server and every bootstrapper — `mariadbd`, `initdb`, `pg_ctl` — stays out, because
      a shim in front of one would be a second way to start or overwrite something nothing is
      watching.

## The tools somebody installs themselves

- [x] **T131** A globally installed tool is a command *(P)*. `runtimes::globals` for where each
      package manager's bindir is and what in it may be fronted; the `bin_commands` table
      (migration 0022); a third dispatch arm in the shim; `daemon::bin_scan`'s two-second mtime
      poll, `[bin] rescan_seconds`, `path.rescan` and `mix path rescan`. The poll and its key were
      replaced by a watch on each bindir in T185b.
      **It follows the version the way `npm` does.** `yarn` is resolved for the directory it was
      typed in and looked for inside *that* version's bindir, so a project pinned to another Node
      gets that Node's Yarn — or a sentence naming the version and `npm install -g yarn`, rather
      than a bare 127. A PATH entry pointing at one install could never have done that.
      **What an idle machine pays** is one `stat` per installed runtime per tick, and nothing else:
      a tick where no bindir has moved returns before it opens the database.

## The authority a runtime has never been told about

- [x] **T132** The machine's roots, and a bundle with ours in it *(P)*.
      `TrustStore::roots` over `rustls-native-certs` — the Windows `ROOT` store, macOS's trust
      settings, Linux's `ca-certificates` — and `generate::ca`, which writes `etc/ca/bundle.pem`
      atomically and privately, with the authority's fingerprint in its header so a rotation makes
      it a changed file.
      **A store answering fewer than twenty roots was read wrong**, and nothing is written: pointing
      a runtime at a file holding a handful of certificates would replace a working trust store with
      a broken one on the next command somebody typed.

- [x] **T133** Every runtime is told where that bundle is. Node gets `NODE_EXTRA_CA_CERTS` naming the
      **authority**, because that mechanism *adds* to what Node already trusts; Python, Ruby and PHP
      get the **bundle**, because theirs *replace* a trust store.
      **PHP through the generated ini set rather than the environment**, so that `php -r` in a
      terminal and `curl_exec()` in a pool answer the same thing — which is the property T28's
      `conf.d` model exists to hold, and the case that matters most, since a site calling another
      site of this home runs inside the pool. It is also what makes PHP on Windows able to verify
      *any* HTTPS: the artifact ships no CA file at all.
      **A variable the person set is theirs**, in either direction.

## Saying so

- [x] **T134** `mix path rescan`, the doctor checks and the documentation. `path.status` gains
      `origins` and `conflicts` — where each command in `bin/` came from, and who won a contested
      name; two `daemon.doctor` checks, one of which repairs without a prompt; ADRs 0033 and 0034;
      and the four feature documents.

**Milestone M15** — on a fresh install, `mysqldump` and `redis-cli` are commands; `npm install -g
yarn` followed by `yarn --version` works in the same shell; and a Node, PHP, Python or Ruby program
started through `bin/` fetches `https://<site>.test` without being told anything.
