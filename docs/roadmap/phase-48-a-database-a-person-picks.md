# Phase 48 — A database a person picks

*Goal: a person decides which database their terminal's clients talk to, and which port an
instance listens on. Neither is left to an order of creation they cannot change afterwards.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Asked for by users on 2026-10-20 and checked against `master` at `fe0ffb4d`. Neither task
depends on phase 47, and phase 47's T209e moves the code T210a changes, so whichever lands second
follows the other.

---

- [x] **T210** `service.set` moves an instance to another port. Today a port is allocated once,
      when the row is written, and never changes (`docs/features/services.md`, *Ports, and who
      gets 3306*). That spec already names the missing piece: "`mix service set`, which does not
      exist yet". The case that asks for it: `mysql@57` holds 3306 and `mysql@8` was given 3307.
      The person deletes `mysql@57` and wants `mysql@8` on 3306. Deleting a row rightly promotes
      nobody into its port, so today the only way there is to delete `mysql@8` too and lose its
      data directory.
      - `service.set { id, port }` and `mix service set <id> --port <n>`. The port is checked the
        way `service.create` checks one: free on the machine (a bind, not a query), not held by
        another row. A port held by a program MixEngine does not manage
        is refused with that program's identity, as at create. Checking and writing the row are one
        critical section, as at create.
      - The row changes, `etc/<id>/` is regenerated, and a running instance is restarted, because a
        port change is one of the changes a reload cannot apply. A stopped one stays stopped.
        Anything else generated from the row's port is regenerated with it. The design lists
        what that is.
      - The answer names what the daemon cannot change: a project's `.env`, a saved connection in
        MixLab's `db` module, somebody's shell history. The window offers to update its own saved
        connections that pointed at the old port.
      - Clients follow without anything else: `MYSQL_TCP_PORT`/`PGPORT` are read from the row on
        every run. Moving `mysql@8` onto 3306 also makes it the instance `mysql` picks by
        `services/client.rs`' "product's own port" rule, which answers the example above on its
        own.
      - The method is shaped so that editing overrides after create can join it later. That is not
        part of this task.
      - **MixLab:** a Port field on the service's settings, beside idle and limits, that shows the
        check's refusal inline. Needs a design with its `## MixLab` section, the
        `client-surface.md` entry, and the *Ports* paragraph of `services.md` rewritten to say a
        port moves only when a person moves it.
- [ ] **T210a** A default instance for a package's clients. With two MySQL instances, which one a
      bare `mysql --version` or `mysqldump` runs is decided by a fixed order in
      `crates/mixengine-core/src/services/client.rs`: `MIXENGINE_MYSQL`, then the only instance,
      then the one on the product's own port, then the lowest port. With no instance and several
      versions installed, the newest version wins. `mix path status` says who won, but the person
      can only change the answer for one shell, with the variable, or by moving ports. The same
      holds for every package that puts clients on the PATH: `redis-cli`, `psql` and the rest.
      - `service.set_client_default { package, target }` and `mix service default <package>
        <instance|version>`, beside `runtime.set_default`. The target is an instance, or an
        installed version when the package has no instance. The choice is stored, so the shim
        reads it without a daemon, like every other step of the order.
      - The order becomes: the variable, then **the stored default**, then the rules there today.
        A default naming an instance that was since deleted, or a version since uninstalled, is
        skipped, and `mix path status` and `mix doctor` say so. It is not an error at the moment
        a person types `mysql`.
      - MariaDB also answering to `mysql`, `mysqldump` and `mysqladmin` stays a rule between
        packages. A default is chosen inside one package and does not change which package owns a
        shared name.
      - Redis and Memcached clients read no environment, so for them the default chooses the
        binary and not the endpoint: `redis-cli` with no `-p` still connects to 6379. The design
        decides whether the shim passes `-p` for the default instance, or the docs say plainly that
        it does not.
      - **MixLab:** the Services screen marks which instance each package's clients use, and an
        instance's menu offers "Use for terminal clients". Needs a design with its `## MixLab`
        section, the `client-surface.md` entry, and *The commands a package brings with it* in
        `services.md` updated with the new step.

**M48** On a home with `mysql@57` on 3306 and `mysql@8` on 3307, deleting `mysql@57` and
running `mix service set mysql@8 --port 3306` leaves `mysql@8`'s data intact and a bare `mysql`
connected to it. On a home with two Redis instances, `mix service default redis redis@cache`
makes `redis-cli --version` answer from that instance's version. Both are done from MixLab as well
as from `mix`, on all three systems.
