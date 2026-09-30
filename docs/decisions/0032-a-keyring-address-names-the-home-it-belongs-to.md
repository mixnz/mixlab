# 0032. A keyring address names the home it belongs to

**Status**: Accepted
**Date**: 2026-09-13

## Context

MixEngine keeps the credentials it generates in the operating system's credential store and nowhere
else — [ADR 0006](0006-servicespec-in-proto-and-secret-free.md) keeps them off a `ServiceSpec`, and
`services/first_run.rs` refuses to fall back to a file. Until now the address of one was
`<service-id>/<user>`: `mariadb@main/root`, `postgres@main/postgres`,
`extensions/phpmyadmin/config`.

**That address is missing the one thing that makes it unique.** The credential store is one store per
operating-system *user*, and `MIXENGINE_HOME` means one user can have several homes — a sandbox for a
real run, a suite's temporary home, a second install for another set of projects. This repository's
own working agreement recommends a sandbox home, and `crates/mixengine-cli/tests/mariadb.rs` declares
its instance as `mariadb@main`. Every one of those writes to the same entry.

A database is what makes that fatal. The password is generated once, written to the store, and then
written *into the server's own data directory* by the bootstrap; from that moment the two copies have
to agree, and only a first run writes both. A second home bootstrapping its own `mariadb@main`
replaces the store's copy and not the first home's data directory, so the first home's server goes on
running with a password its owner can no longer produce.

**Measured, on a developer's machine, on 2026-09-13.** A sandbox home under `$TEMP` ran its first run
at 05:55:57 and the `mariadb@main/root` entry's last-written time moved to 05:55:57. The repository's
own home had bootstrapped its server on 2026-09-12 at 17:27 and was still running it. Everything that
authenticates as root broke at once: `blueprint.apply` failed at the database step of a Laravel
apply, and `mix service stop mariadb@main` could not shut the server down either — `mariadb-admin
shutdown` was refused with the same `ERROR 1045`, so the supervisor killed the process and the next
start recovered from a crash. The databases were intact; nothing could reach them.

## Decision

**Every credential address begins with the id of the home it belongs to**, and that id is a value the
home carries rather than one derived from where it sits.

    <home-id>/<service-id>/<user>          9f3c1a77b204/mariadb@main/root
    <home-id>/extensions/<id>/config       9f3c1a77b204/extensions/phpmyadmin/config

`migrations/0021_home_id.sql` mints it — six random bytes as hex, `ON CONFLICT DO NOTHING` — so a home
has one from its first open and no code path has to ask whether an identity exists yet.
`mixengine_core::home::id` reads it and `services::handoff::secret_key` composes the address, which
is the same single composition [T84](../roadmap/phase-10-client-surface.md) established.

**Not a hash of the home's root path**, which needs no row and was the smaller change. A path hash
strands every credential in a home the moment somebody renames its directory — the same outage this
decision exists to end, arriving for a different reason — and Windows makes it worse than that: case,
8.3 short names, UNC spellings and a trailing separator are four ways to write one directory and four
different hashes. A home that is *copied* rather than moved shares its id, and that is accepted: a
copied home also shares its data directories, so sharing the credential is briefly correct, and it is
a broken artefact in a dozen other ways first.

**An entry written before this decision is found, and moved, on the read that needs it.**
`mixengined`'s `secrets` module reads the new address, falls back to the address with the home taken
off, and writes what it finds to the new one. Lazily, because there is no set to walk: a service's
ritual credentials could be enumerated from the recipes, but a database *account's* cannot — the
account names arrive from whoever asks, and the only thing that knows them all is the server, which
cannot be asked without the credential being migrated first.

**The old entry is not deleted.** Another home on the machine may still be running a build that reads
it, and taking it away would cause exactly the outage above for a home nobody has upgraded yet. What
is left behind is an entry nothing writes any more.

## Consequences

**The published convention changes shape, and no client breaks.** `SecretAddress` is a value the
daemon *returns* — `database.client`, `database.credentials`, `database.open` — and the standalone client reads
`secret_key` out of the handoff URL rather than composing one
(`apps/desktop/src-tauri/src/modules/db/handoff.rs`). A client that composed the string itself would
break; none in this workspace does, and the field's own documentation has said it is composed by
`mixengine_core::services::handoff::secret_key` since T84.

**Two homes can hold two passwords for one service id**, which is what the whole decision buys, and
it is also what makes a test suite safe to run on a machine that has a home in use.

**A home with no id refuses rather than mints one.** `Error::HomeHasNoId` is a database that skipped
the migration or was edited by hand; generating a replacement would answer the call and orphan every
credential already stored under the first id.

**A credential already replaced by another home stays replaced.** Nothing here recovers the password
a server has in its data directory — it cannot be read back out. What T126 adds instead is that the
failure says so: a server refusing the superuser password this home holds is answered with what that
means and what the two ways out are, rather than with the server's `ERROR 1045`. Automating the
repair — stopping the service, re-setting the password against the data directory through the
recipe's own bootstrap — is left to a later task.
