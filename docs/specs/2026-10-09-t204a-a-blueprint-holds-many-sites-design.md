---
status: draft
date: 2026-10-09
task: T204a
---

# T204a — A blueprint holds many sites (design)

Roadmap phase 42. T204 gave the project manifest `[[sites]]` and left blueprints alone, on purpose:
a blueprint is its own type, with its own schema, its own resume rule and a gallery published from
another repository ([T204's design](2026-10-08-t204-a-manifest-holds-every-site-design.md), *Not in
scope → Blueprints*). This is that half. T205 has since landed
([design](2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md)), and with it ADR 0061 and
`[[next_steps]] site`, which this design gives a meaning.

## What this closes

Read against `master` at `de057317`:

1. **Capture refuses every project with more than one site.** `blueprint.capture` answers
   `ProjectHasSeveralSites` (`blueprints/capture.rs`). The refusal is honest, and it means the
   `ezweb` of T204 (three sites, three kinds) can be exported to a colleague as `mixengine.toml` and
   cannot be turned into a blueprint at all.
2. **A blueprint has one `[site]`.** `BlueprintManifest.site: Option<BlueprintSite>`, so a
   hand-written blueprint cannot describe a Laravel site beside its Vite dev server, or an API beside
   its admin.
3. **The apply assumes one site in four places**, and each one would do the wrong thing rather than
   fail if it were simply handed two:
   - the resume rule `has_a_site` (*this project has a site* is the whole question), so a resumed
     apply would call the second site already made;
   - `AddDomain` finds its site by the project's root (`SiteRef::Path`), which names no site once
     there are two;
   - `CreateSite` links every service the apply ensured (`context.ensured`), which T204 D3 already
     calls the wrong default for several sites;
   - MixLab's `AfterApply` opens `site.list`'s first row.
4. **`[[next_steps]] site` is refused outright** (`manifest.rs`, `checked_step`), waiting for this
   task to say what it names.

## Goal

A project with several sites captures into one blueprint, and applying that blueprint makes every
site with its own domains, kind, routes and services, from `mix` and from MixLab. A blueprint with
one site is written, read and applied exactly as it is today, at the schema it is today.

## Not in scope

- **A PHP per site.** `[runtimes] php` is one version and every php-fpm site an apply makes runs the
  project's pin. A project whose php-fpm sites run two PHPs cannot be described by this format, and
  capture says so (D6) rather than picking one.
- **A front end, a scaffold or an archive per site.** `[scaffold]` runs once, in the project root;
  `[php]` and `front_end` are about the machine. None of them belong to a site.
- **A `dotenv` per site.** T205a's rule (one service per manifest carries it, because an apply
  remembers one database) is unchanged.
- **Merging into an existing project's sites.** An apply makes a project; a site this project
  already has is resumed (D4), never edited.

## Is there an ADR here? No

ADR 0061 already decides the one cross-cutting question, how the blueprint schema moves, and its
last consequence says what a later bump owes: *"the next one states which key raises it and why that
key changes what an apply does."* D2 is that statement. Nothing else here crosses a module or
process boundary: the wire gains one optional member (`PlanAction::CreateSite.services`) under
[ADR 0019](../decisions/0019-an-added-response-member-is-optional.md), and no migration is needed
(D4 resumes by domain, which `site_domains` already holds).

## Decisions

### D1 — `[[sites]]` beside `[site]`, and the writer picks the form the file can hold

```toml
schema = 3

[[sites]]
kind = "php-fpm"
doc_root = "public"
https = true
domain_pattern = "{project}.test"
services = ["mariadb", "redis"]

[[sites]]
kind = "reverse-proxy"
upstream = "http://127.0.0.1:5173"
https = true
domain_pattern = "vite.{project}.test"
services = []

[[services]]
name = "mariadb"
instance = "main"
database = "{project}"
user = "{project}"

[[services]]
name = "redis"
instance = "main"
```

Each entry has exactly the keys a `[site]` has today, read by the same `BlueprintSite`
deserialiser, plus `services` (D3). `[[sites.routes]]` nests the way `[[site.routes]]` does.
`BlueprintManifest.site: Option<BlueprintSite>` becomes `sites: Vec<BlueprintSite>`.

**Reading** — the rules a TOML parser cannot state, checked in `RawManifest`'s `TryFrom` beside
D2 and D4 of T205:

- `[site]` reads as a one-entry list, `[[sites]]` as the list, neither as an empty list.
- **Both** is refused, naming the two keys (T204 D1's rule for the project manifest).
- **`[[sites]]` in a file that declares `schema` below 3 is refused.** An older build ignores an
  unknown key, so such a file would apply there as a project with no site at all: the file would be
  lying about which builds can read it. The refusal names the schema the file needs.
- **No name twice.** Every expanded-looking name (each `domain_pattern` and each alias, compared as
  written) appears in one entry once. Two entries answering to one name would make the second
  `CreateSite` fail on `already_exists` halfway through an apply, and would make D5's `site`
  ambiguous. Refused at read time, naming the name and both positions, because unlike
  `mixengine.toml` a blueprint is generated, and a duplicate there is a bug in whatever wrote it.

**Writing.** `render` writes `[site]` for one site with no `services`, and `[[sites]]` otherwise
(D2), so a one-site blueprint is byte-identical to today's rendering and every gallery file stays
its own rendering. Entries keep the order of `sites`, and a step's `site` is written back.

### D2 — Only a blueprint with more than one site is schema 3

`SCHEMA` becomes 3, the highest this build reads. `schema_of` answers 3 when `sites.len() > 1`, or
when an entry carries `services` (D3, which `[site]` cannot hold), and otherwise what it answers
today (2 for an archive or a `dotenv`, else 1). `render` writes `[[sites]]` in exactly those cases.

**Why this key raises the schema (ADR 0061's test).** `[[sites]]` changes what an apply does: an
older build that ignored it would make a project with no site, no domain and no certificate, and
report success. That is exactly the case the refusal exists for, so an older build reading a
schema-3 file answers `UnknownBlueprintSchema`, naming the blueprint, and imports nothing. That is
the right behaviour, and no older build needs a change for it.

A hand-written `[[sites]]` holding **one** entry and no `services` is read, and re-rendered by
`store::save` as `[site]` at the lowest schema that holds it. Nothing is lost, and the copy becomes
readable by more builds than the original. One that names its `services` stays `[[sites]]` at 3,
since `[site]` would link every service instead.

### D3 — `services` on an entry names that site's links

In a blueprint, `[[services]]` is what the project has to *have*. With several sites the file has to
say which site links which service, on T204 D3's rule:

- **Each `[[sites]]` entry may carry `services = [...]`.** An item names a `[[services]]` entry by
  its `name`, or as `name@instance` (the entry's `instance` as written, `per-project` included) when
  two entries share a name. An item that names no entry, or names two, is refused at read time.
- **Absent means every `[[services]]` entry**, which is what a `[site]` links today. `[]` means none.
  Absent and empty are different, on `ProjectUpdate`'s rule.
- **`services` on `[site]` is refused.** A one-site blueprint links everything, and accepting the key
  there would be a behavioural key at schema 1 or 2 that an older build ignores, which is what D2
  exists to prevent. A person who wants a one-site blueprint to link less removes the service.
- **The plan resolves each item to the `ServiceId` its `EnsureService` step makes**, through the same
  `instance_of` the services loop uses (so `per-project` becomes the project's handle), and carries
  the list on the step: `PlanAction::CreateSite` gains

  ```rust
  /// The services this site links, as their `EnsureService` steps name them — T204a.
  /// `None` is every service the apply ensured, which is what a one-site blueprint links.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  services: Option<Vec<ServiceId>>,
  ```

  `None` for `[site]` and for an entry with no `services` key, so a one-site plan is the same on the
  wire as today. The executor passes `Some(list)` to `site.create` as it stands.
- **`None` is read off the plan, not off what the walk created.** Today the executor links
  `context.ensured`, which only an `EnsureService` that *created* an instance pushes to: a step
  planned `Satisfied` returns before it is carried out. So on a home that already runs
  `mariadb@main`, applying `laravel` makes a site linked to nothing, `service.start { project }`
  does not start MariaDB, and a capture of that project loses its `[[services]]`. `None` becomes
  every `EnsureService` step's id in the plan (`steps::ensured_in`), whatever its disposition, which
  is what *every service the apply made sure of* meant all along; `context.ensured` goes.

### D4 — One group of steps per site, and a site is resumed by its names

**Order.** Each entry plans as the group a `[site]` plans as today, and the groups follow file order:

```
… services, databases …
create_site  (1)   add_domain shop.test (primary)   add_domain www.shop.test   issue_certificate
create_site  (2)   add_domain vite.shop.test (primary)                         issue_certificate
set_php_extension … scaffold … write_dotenv
```

A group rather than a tier per action kind, because the executor already reads it that way:
`CreateSite` takes its names from the `AddDomain` steps straight after it (`steps::names_after`,
which stops at the first step that is not one), so the domains of site 2 placed after site 1's would
be given to site 1. `the_steps_are_in_dependency_order` keeps its tiers, with `CreateSite`,
`AddDomain` and `IssueCertificate` becoming one tier ("sites") whose internal order is asserted per
group: create, primary, aliases, certificate.

**Resume.** Running an apply again plans against what the first one left (`blueprints.md`,
*Resuming is running it again*). With one site the question was *does this project have a site*;
with several it has to be *which*:

- **An entry is already made when a site of this project answers to any of its expanded names**,
  primary or alias, on T204 D2's reasoning: a renamed primary with the old name kept as an alias
  still finds its site. `CreateSite` plans `Satisfied`; its `AddDomain` steps already plan
  `Satisfied` for a name this project's site holds (`domain_step`), and `Create` for one it does
  not, which the executor adds.
- **One existing site answering to the names of two entries** blocks the second entry's
  `CreateSite`, naming both: the home holds one site where the blueprint describes two, and neither
  merging them nor making a third is a guess an apply should make.
- **A one-site blueprint keeps today's rule** (*this project has a site*), so a project whose one
  site was renamed outright and is then re-applied resumes as it does today.

No migration: the identity is the names, and `site_domains` already holds them.

**Finding the site for `AddDomain`.** The executor stops using the project's root
(`SiteRef::Path`), which names no site once there are two. It uses `SiteRef::Domain` of the first
name in the step's group that already answers (`steps::group_names`, looking back to the group's
`CreateSite`). For a one-site project that is the same site the root found.

**Rollback** is unchanged in shape. Each `CreateSite` records `Made::Site { domain }` before it calls
`site.create`, so a failure after the second site removes both, newest first, and a failure on the
second removes the first. A site that was `Satisfied` was not made by this apply and is not in the
ledger, so a resumed apply that fails never removes what an earlier run made.

### D5 — `[[next_steps]] site` names an entry by its `domain_pattern`

- **Its value is one entry's `domain_pattern`, as written** (`site = "vite.{project}.test"`). That is
  the identity a blueprint entry already has, it survives capture (the pattern is the tokenised
  domain), and it is the identity T204 gave `[[sites]]` entries in `mixengine.toml` (their
  `domain`). A separate `name` key would be a second identity for the same thing, and one a person
  could not read off the address bar.
- **Required on every step when the blueprint has more than one site, and refused when it has one**,
  as today. Required because every step belongs somewhere: a `serve` runs one site's program, a
  `once` runs in one site's tree, an `open` opens one site's address. A step with no `site` in a
  several-site blueprint would leave a client to guess which.
- A value that is no entry's `domain_pattern` is refused at read time, naming the step's position
  and the patterns that exist.
- `steps::expanded` expands `site` with the rest, so `BlueprintApplied.next_steps` and
  `ProjectDetail.next_steps` carry the real domain (`vite.shop.test`), which is what a client
  shows and opens.
- **The welcome page (T124) lists a site's own steps**: those whose `site` is one of its names, or
  every step when the blueprint has one site. `served.rs` keys the steps by project today; it keys
  them by project and expanded domain instead.

### D6 — Capture writes every site

`ProjectHasSeveralSites` is removed. A project with several sites captures into `[[sites]]`, in
`sites::records` order, which is primary-domain order and so deterministic (T77's D7):

- **Each entry** is today's `[site]` capture of that site: kind with its pool dropped, `doc_root`,
  `https`, the tokenised primary and aliases, routes with their pools dropped.
- **Each entry's `services`** is that site's `site_service_links`, minus the front ends and minus
  the pool, written as D3's items (`name`, or `name@instance` when the blueprint holds that name
  twice). Written always for a several-site capture, `[]` included, so the file says what each site
  links rather than falling back to *every service*.
- **`[[services]]` is the union** of every site's links, each once, in order of first appearance.
  `database`/`user` come from the project's `mixengine.toml` as today.
- **One PHP.** `[runtimes] php` and `[php] extensions` come from the PHP the php-fpm sites' pools
  run. When they run more than one version, capture is refused with
  `Error::ProjectRunsSeveralPhps { project, sites: Vec<(domain, version)> }`, mapped explicitly to
  `conflict` in the daemon's `error.rs`. The refusal it replaces never was: `ProjectHasSeveralSites`
  has no arm there and reaches clients through `_ => internal`, which tells a person the daemon
  broke when the project simply did not fit. This one is narrower and as honest: a blueprint has one
  PHP, and dropping one site's version is the silent loss T77 refused.
- **`[[next_steps]]`** come from the project's blueprint as today (T205 D5). When that blueprint had
  one site and its steps no `site`, and the capture now has several, each step is given the entry
  whose `domain_pattern` equals the old blueprint's `[site] domain_pattern`, or the first entry when
  none does. Without that, a project made from `laravel` that later gained a second site by hand
  would capture into a file this build refuses to read.
- A project with one site captures exactly as today.

### D7 — `mix`

- `mix blueprint capture` stops printing the several-sites refusal and prints the new one (D6).
- `mix blueprint apply` / `--dry-run` print the plan as today. The groups are already in order and
  each `create_site` line gains its primary domain (read from the `AddDomain` after it, as the
  executor does), so two sites read as two: `create site shop.test (php-fpm, public)`.
- Steps with a `site` print it after the kind: `serve (vite.shop.test)  npm run dev`, in
  `mix blueprint apply`'s answer and in `mix project show`.

## MixLab

- **Blueprints screen, `ApplyDialog`:** the plan list is grouped by site. Each `create_site` step
  becomes a sub-heading naming its primary domain, and its domains and certificate sit under it, so
  a three-site plan reads as three sites rather than one long run of domains. The grouping is read
  off the plan's order, as the executor reads it; nothing is decided in the client.
- **`CaptureDialog`:** no longer shows the several-sites refusal for such a project; the capture
  succeeds. The new refusal (several PHPs, D6) arrives as an ordinary error and is shown as one.
- **`ImportDialog`:** a schema-3 file imports. Nothing new to draw.
- **`AfterApply`:** the address it opens is the first entry's site (the first `AddDomain` with
  `primary` in the applied plan), not `site.list`'s first row, which is ordered by the daemon and
  not by the blueprint. With an `open` step, its `site`'s address plus `path`. The *ready* state lists
  every site's address, each with **Open**, when there is more than one.
- **`NextStepsPanel`** (AfterApply and the Projects screen): steps are grouped under their site's
  domain when they carry one. *Run the required steps* keeps T205 D8's rule over the whole list
  (required `once` steps, then the first required `serve`, and a tab per further `serve`), since a
  project's tabs are one project's work whichever site they serve.
- **Dashboard, Quick Start:** unchanged. It applies gallery blueprints, which keep one site, and its
  address comes from `AfterApply`.
- **No new method**, so `check-client-surface` has nothing to add. `PlanAction::CreateSite` gains an
  optional member, and `bindings/` is regenerated (`packaging/bindings.sh`).
- **With MixEngine off**, nothing here runs: all of it is in the `mixengine` module.
- **Strings** go through `t()` in `en` and `vi` (the grouping heading, the list of addresses).

## The gallery

**No gallery entry gains a second site in this task**, so no gallery file changes and
`mixengine-packages` has nothing to re-publish. Under *"a coverage surface, not a list of
favourites"* a several-site entry needs a gap only it closes, and none of the candidates (a Laravel
API beside a Vite front end, Strapi beside Next.js) is a stack the gallery cannot already set up as
two projects. The roadmap line's *"`mixengine-packages` re-publishes"* is corrected to say so. If an
entry is wanted, it is its own task with its own reason, and that task re-runs `publish-blueprints`.

## Testing

- **Manifest** (`blueprints/manifest.rs`): `[site]` reads as one entry and `[[sites]]` as its
  entries, in order; both together refused; `[[sites]]` under `schema = 1` or `2` refused, naming 3;
  a name twice (pattern or alias, across entries) refused with both positions; `services` items
  resolved by `name` and by `name@instance`, unknown and ambiguous items refused, absent told apart
  from `[]`; `services` on `[site]` refused. `render`: one site writes `[site]` at schema 1 or 2,
  two write `[[sites]]` at 3, and a one-entry `[[sites]]` file re-renders as `[site]`. A schema-4 file
  is refused by name. Steps: `site` required on every step with several sites, refused with one, and
  refused when it names no entry.
- **Gallery** (`tests/blueprint_gallery.rs`): unchanged, and still each file its own rendering, which
  is what proves a one-site blueprint renders as before.
- **Plan** (`blueprints/plan.rs`): a two-site manifest plans two groups in file order, each
  create → primary → aliases → certificate; `the_steps_are_in_dependency_order` over it. `services`
  on `CreateSite` is `None` for `[site]` and for an entry with no key, and the ensured ids
  (`per-project` expanded) otherwise. Resume: a project holding entry 1's site plans entry 1
  `Satisfied` and entry 2 `Create`; a site found by an alias is `Satisfied`; one site holding names
  of both entries blocks the second; a one-site blueprint keeps `has_a_site`'s answer.
  `front_end` is planned for a manifest with any site.
- **Apply** (`crates/mixengine-daemon/tests/`): a two-site blueprint applies; each site has its own
  domains and only its own links. An `AddDomain` on a resumed apply reaches the right site, not the
  root's. A failure injected after the second site rolls back both sites and keeps the database; a
  failure on the second leaves no site behind. A resumed apply makes only the missing site.
- **Capture** (`blueprints/capture.rs`): the test that asserted the refusal becomes a three-site
  capture with per-site `services`, a union `[[services]]` and schema 3; two php-fpm sites on two PHPs
  are refused naming both; steps without `site` are attached to the entry that matches the old
  pattern. The rendered-string test (*nothing forbidden reaches the rendered manifest*) runs over a
  several-site capture too.
- **The round trip** that phase 42's M42 asks of `mixengine.toml`, for blueprints: a project with
  three sites of three kinds, each linking a different service, is captured and applied under a new
  name, and the new project's sites have the same kinds, routes and links, with `{project}` expanded.
- **Welcome page** (`generate/served.rs`): each site lists its own steps.
- **`mix`**: `render` of a two-group plan and of steps carrying `site`, in `mod tests`.
- **MixLab** (vitest): the plan grouping over a two-site plan; `AfterApply`'s address choice (first
  entry, an `open` step's site, one site as today); the panel's grouping.

## Documentation, when it lands

- [features/blueprints.md](../features/blueprints.md): the manifest section shows `[[sites]]` and
  `services`; *Capture* replaces the several-sites paragraph with D6; *Apply* gains the resume rule
  of D4; *What is left to do* says what `site` names.
- [architecture/data-model.md](../architecture/data-model.md) where it shows a blueprint, if it does.
- `docs/guide/en|vi/` wherever blueprints are captured (the refusal is gone), restamped.
- Phase 42: T204a's line, the re-publish corrected per *The gallery*. Root `CHANGELOG.md` under
  `[Unreleased]`.
