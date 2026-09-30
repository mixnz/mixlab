# 0026. The active front end is a row, and switching it is a job

**Status**: Accepted
**Date**: 2026-09-06

## Context

[ADR 0004](0004-caddy-as-default-web-server.md) chose Caddy as the default web server with Nginx as
a first-class alternative, exactly one active at a time. Among the costs it accepted it wrote: *"We
mitigate by generating both, keeping feature parity, and making the switch one setting."*

That sentence was written on 2026-08-10, and everything it would have to be built on arrived after
it. There was no `services` table yet, no `Recipe::role` — **T37** added that, and with it the
refusal that makes "exactly one front end" a rule something can break rather than an accident of
there being only one recipe. There was no shape for the port-80 grant either; **T42** gave it one,
and it is not the same shape on the three operating systems.

The standalone client's Phase 4 spec is what asked the question out loud, writing a Settings screen against
[../features/client-surface.md](../features/client-surface.md). The finding: *which web server is
active* has no readable or writable state anywhere. `front_end::held_by` derives it by scanning the
table and uses it only to **refuse** a second front end; no RPC answers it, `Config` has no field
for it, and there is no `settings.*` namespace. A client could only get at it by hardcoding that the
package names `caddy` and `nginx` mean "front end" — a copy, in the client, of a table the daemon
compiles in.

This ADR does not reopen ADR 0004. Its decision stands unchanged; what is settled here is the one
clause it left as a gesture.

## Decision

**The `services` table is the truth, and nothing else records it.** The active front end is the row
whose recipe answers `Role::FrontEnd` — precisely what `front_end::held_by` already reads to refuse a
second one. **No field is added to `config.toml`.**

**The reading is a field on `ServiceSummary`**: `role`, copied from the recipe. Static, additive on
the wire under [ADR 0019](0019-an-added-response-member-is-optional.md), and it bumps no protocol
version.

**The switch is a job, not a write to a setting.** `service.set_front_end` stops the old front end,
deletes its row, creates the new one, re-renders `sites/` (**T43**) and starts it — deleting before
creating, so "exactly one" is never momentarily false and the refusal needs no exception carved out
for its own caller. Where the platform requires a new grant it **enqueues** an elevation operation
and does not raise a prompt itself, per [ADR 0005](0005-on-demand-elevation.md).

## Consequences

**Easy**:

- One statement of the fact, so the answer and the refusal cannot disagree. A config file that said
  `nginx` while the table held a `caddy` row would have no arbiter, and generated config is
  disposable by project rule — the table is the only durable statement there is.
- The client stops inferring. `ServiceSummary::role` answers by what a package is *for*, which is
  the same distinction T37 made so that neither program is the one the code happens to know about.
- Adding a third front end adds a recipe, not a variant in an enum somewhere and a string in a
  client.

**Hard / accepted costs**:

- **A switch can end with the home still on the old front end, and that has to be describable.** On
  Linux the port-80 grant is `cap_net_bind_service` written into the `security.capability` attribute
  **of the binary**, so changing front end changes which binary needs it; macOS redirects by port
  and carries over; Windows reserves nothing and grants nothing. A machine where nobody grants stays
  where it was — the degraded mode of [ADR 0005](0005-on-demand-elevation.md), applied here. The
  outcome a switch must never produce is a home whose sites are rendered for a server that cannot
  answer.
- The switch is therefore not instant and not always silent, where "one setting" implied both. It
  would have *looked* simpler as a config field and been a lie on one of the three platforms.
- `ServiceSummary` grows a field that is constant for the life of a row. Accepted: the alternative is
  a second method to ask a question the list is already answering, which
  [../features/client-surface.md](../features/client-surface.md) rules out in its own acceptance
  criteria.

**Enforcement**: no client may map a package name to a role. The generated TypeScript bindings carry
`ServiceRole`, and a client that needs the distinction has it from the daemon or does without.

## Alternatives considered

- **A `front_end` field in `config.toml`**, which is what ADR 0004's phrase suggests. Two sources of
  truth that can disagree, with no arbiter and no migration for a home where they already do; and it
  would still not be the whole switch, because the row, the rendered sites and the grant all have to
  move together.
- **A `settings.*` namespace.** Everything the Settings screen lists already has a home —
  `autostart.*` (T85b), `daemon.*`, `path.*`, `config.toml` — so a new namespace would be a second
  door onto each of them and one more place for two answers to differ.
- **Let the client sequence `service.delete` and `service.create` itself.** It puts the ordering,
  the re-render and the grant in the client, which is the business logic this project keeps out of
  clients; and a client that dies between the two calls leaves a home with no front end at all.
- **Refuse to support switching, and make it an install-time choice.** Honest, and it was tempting
  while the read half looked like the whole problem. Rejected because ADR 0004 sold nginx as a
  first-class alternative to people whose production is nginx, and an alternative you can only pick
  once is not first-class.
