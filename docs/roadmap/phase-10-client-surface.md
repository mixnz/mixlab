# Phase 10 — Client surface

*Goal: what [`client-surface.md`](../features/client-surface.md) claims about itself is true.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

---

**This phase exists because of one sentence the file writes about itself.** Its acceptance criteria
end with *"Every screen above can be assembled from documented methods and events, with no method
existing solely to serve one of them"* — and that sentence was false in two places. Both
were found by **The standalone client reading the file against the API** while writing its own Phase 4 spec, which
is the failure mode [ADR 0011](../decisions/0011-no-gui-in-this-repository.md) accepted when it put
the graphical client in another repository: a claim made on paper here is checked by somebody else's
code, later, and the two tasks below are the bill for it. Neither was needed for v0.0.1 and neither
is a defect in what shipped.

A third gap the same reading found is closed in [todo.md](todo.md) rather than here, because the
answer was that the API is right and the prose was loose: `ServiceSummary` carries no CPU or RSS on
purpose, since a figure on `service.list` is a way to take T71's fast cadence without opening the
subscription that is supposed to gate it.

- [x] **T96** Disk usage broken down by category, and a cleanup that can only reach what is safe to
      lose.
      `daemon.disk_usage` is a strict read and `daemon.cleanup` is a job — the shape **T87** already
      established for `daemon.uninstall_plan` / `daemon.uninstall`, for the same reason: a call that
      measured and deleted in one breath would leave no moment in which the user could be shown what
      is about to go.
      **Five categories, not the four the screen asks for.** `client-surface.md` names runtimes,
      data, logs and certs; `cache/` is the fifth and it is the only one anybody actually wants to
      reclaim, so a chart drawn from the four would be a chart with nothing to do underneath it.
      **The categories are not the same kind of thing, and the API has to say so rather than let a
      client find out.** Each one answers *whether it can be reclaimed and by what* — `data/` never
      (they are somebody's databases), `runtimes/` only through `runtime.uninstall`, `certs/` only by
      losing HTTPS until the next issue, `logs/` and `cache/` by this method. A client that had to
      derive that would be deriving exactly the policy `CLAUDE.md` keeps out of clients, and a
      cleanup button that deleted a runtime directly would be the back door around **T32**'s refusal
      to uninstall a runtime under a running pool.
      **What may be removed is a closed list, never a walk of the home** — `daemon.bundle`'s rule
      (T93) applied to the other direction. Rotated log files and `cache/`; nothing else, whatever
      the sum says.
      Reachable as `mix disk` and `mix cleanup`, because a gap in the CLI is a gap in the product.

- [x] **T97** The active front end is answerable, and switchable — design in
      [ADR 0026](../decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md). **(P)**
      **The two halves are not the same size, and only the first one is cheap.** Reading is a
      `role: ServiceRole` on `ServiceSummary`, copied from the `Recipe::role` **T37** already
      compiles in: static, no sampling cadence to break, additive on the wire under
      [ADR 0019](../decisions/0019-an-added-response-member-is-optional.md) so it bumps no protocol
      version. It removes the one thing a client can do nothing about today — inferring that the
      package names `caddy` and `nginx` mean "front end", which is a hardcoded copy of a table the
      daemon owns.
      **Writing is a job that goes through the elevation queue, and the reason is Linux.** T42 grants
      port 80 by writing `cap_net_bind_service` into the `security.capability` attribute **of the
      binary**, so a home that switches from Caddy to nginx is a home whose new front end has no
      grant; macOS redirects by port and carries over, Windows reserves nothing. So the switch
      enqueues rather than acts, and a machine where nobody grants **stays on the front end it had**
      — which is the honest outcome and has to be described as one, not left as a home whose sites
      are rendered for a server that cannot answer.
      The walk is stop, swap the row, re-render `sites/` (**T43**), start — deleting before creating,
      so `front_end::held_by`'s "exactly one" is never momentarily false and the refusal needs no
      exception carved into it for its own caller.
      **The rule is *do not make it worse*, not *require a grant*** — settled while building it: a
      home whose current front end cannot answer either has nothing to lose, and refusing there
      would trap somebody on a server that cannot bind 80 to protect them from a server that cannot
      bind 80. So the old binary is probed as well, and `answering` on the report is what the
      machine said afterwards.
      **A create that will not render puts the old row back.** The window the delete-before-create
      order opens is real, and the step inside it is the one that can fail — a front end renders
      through its own checker, `nginx -t` and `caddy validate` — so the old declaration is read
      before it is deleted and `RolledBack` is what a refused rendering answers.
      Reachable as `mix service front-end` and `mix service set-front-end <caddy|nginx>`.
      Design:
      [docs/specs/2026-09-07-t97-the-active-front-end-design.md](../specs/2026-09-07-t97-the-active-front-end-design.md).
- [x] **T183** A service says which version it runs. `ServiceSummary` had no version, so
      `mysql@main` told nobody which MySQL it was, and the only fix open to a client was to guess
      from the id. `service.list` carries `version` from the row's parent, `mix service list` gains
      a `VERSION` column, and MixLab draws it beside the id. Design:
      [docs/specs/2026-09-24-t183-a-service-says-which-version-it-runs-design.md](../specs/2026-09-24-t183-a-service-says-which-version-it-runs-design.md).

**Milestone M10 — the standalone client's Dashboard and Settings screens draw whole, with no business logic in the
client.** Not *a client can call these methods*: the test is that the screen `client-surface.md`
describes can be built from what is documented, which is the claim the file has been making since it
was written and has not been able to keep. **Reached with T97.**

---

Previous: [Phase 9 — Ship](phase-9-ship.md) · Then: [Parked](parked.md)
