---
status: approved
date: 2026-10-08
task: T204
---

# T204 — A manifest holds every site (design)

Roadmap phase 42. A project has held any number of sites since T39a, and its `mixengine.toml` has
held one: T39a's D9 wrote a `[site]` only for a project with exactly one, and called that "a limit
of the file format, not of the model". This design widens the format to `[[sites]]`, so exporting a
project writes all of it and adopting a colleague's checkout gets all of it.

## What this closes

Exporting `ezweb`, a project with three sites, printed:

```
Wrote C:\Users\haiqu\Developer\www\ezweb\mixengine.toml. A manifest holds one site, so these were left out: ezportal.test, ezweb.test, ezwebsite.test.
```

Read against `project.export` (`crates/mixengine-daemon/src/projects.rs`), five things are wrong
with that, and only the first is the format:

1. **All three sites are missing from the file, and none of them was written.** The sentence reads as
   if some were kept. In fact the file holds `[project]` and `[runtimes]` and nothing else, so the
   colleague who clones it gets a project with no sites.
2. **`[[services]]` goes too, and nothing says so.** `merge_services` runs only inside
   `if let Some(site)`, so a project with two or more sites exports none of its services. The notice
   names the sites and never mentions the services.
3. **A `[site]` already in the file stays as it was.** Export one site, add two more, export again:
   the second export does not touch `[site]`, so the file goes on declaring one site of three, with
   whatever `aliases` and `routes` it had back then. That is the disagreement between file and home
   that writing `aliases` and `routes` in full exists to prevent. `site.create` with no arguments
   then takes that stale domain and fails with `already_exists`.
4. **`mix` says nothing at all.** `render::project_export` prints `wrote <path>` and never reads
   `sites_omitted`, so on the CLI the sites are left out without a word.
5. **MixLab presents the loss as a success.** The Projects screen puts the sentence in a notice,
   not a warning.

## Goal

`project.export` writes every site of the project, along with which services each site links. A
colleague adopting the checkout can create each of those sites from the file, from `mix` or from
MixLab. A file written by a build from before this task keeps meaning what it meant.

## Not in scope

- **Blueprints.** A blueprint is its own type (`core::blueprints::manifest`, T77's D1), with its own
  `schema`, its own `domain_pattern`, a gallery published as signed files from `mixengine-packages`,
  and an apply plan whose resume rule is "this project has a site" (`plan.rs`, `has_a_site`).
  Widening it means `schema = 2`, one `CreateSite` step per entry with its own resume identity, and
  a re-publish in the other repository. That is a different set of problems, so it is **T204a**,
  with a design of its own. Until then `blueprint.capture` keeps refusing a project with several
  sites (`ProjectHasSeveralSites`). That refusal was already the honest answer, and it stays honest.
- **Adopting every site in one call.** See D6.
- **Pruning.** Export stays a merge, not a mirror (T39a's D9, first rule). An entry the home has no
  site for is reported and kept as written (D4).

## Is there an ADR here? No

A new ADR is for a cross-cutting decision, or for changing one an ADR already records. This is
neither:

- **No ADR records the one-site format.** It is T39a's D9, a spec decision, and T39a,
  `blueprints.md` and `core::lib` each say the widening is expected ("`[[sites]]` is where that
  widens"). A later change to an implemented spec gets a newer spec
  ([plans-and-specs.md](../standards/plans-and-specs.md)), and this is that spec.
- **The change stays inside one file format and one module.** `core::manifest` reads and writes
  `mixengine.toml`. `project.export`, `site.create` and `project.show` are its callers. No other
  component, process boundary or repository is affected. The wire changes only by optional
  members, which [ADR 0019](../decisions/0019-an-added-response-member-is-optional.md) already
  covers.
- **Compatibility with older builds is a property of this format, not a policy.** D1 keeps a
  one-site project in `[site]`, so a build that has never heard of `[[sites]]` reads one-site files
  exactly as before. A several-site file reaches an older build as an unknown section, which that
  build already ignores, and that is today's result for such a project anyway: no site.

The decision that would need an ADR is the blueprint `schema` bump, because it reaches
`mixengine-packages` and every published gallery file. That decision belongs to T204a, and T204a's
design decides whether it needs an ADR.

## Decisions

### D1 — `[[sites]]` beside `[site]`, and the writer picks the form the file can hold

```toml
[[sites]]
domain = "ezweb.test"
aliases = ["www.ezweb.test"]
doc_root = "public"
kind = "php-fpm"
https = true
services = ["mariadb@main", "redis"]

[[sites]]
domain = "ezportal.test"
kind = "reverse-proxy"
upstream = "http://127.0.0.1:3000"
https = true
services = []
```

Each entry has exactly the keys a `[site]` has today, read by the same `ManifestSite` deserialiser,
plus `services` (D3). `[[sites.routes]]` nests the way `[[site.routes]]` does.

**Reading.** `Manifest::site: Option<ManifestSite>` becomes `Manifest::sites: Vec<ManifestSite>`.
A `[site]` reads as a one-entry list, `[[sites]]` as the list itself, and a file with neither as an
empty list. A file holding **both** is refused as `Error::Manifest`, naming the two keys. Picking
one would apply something the author did not write, and the reader already refuses a `[site]` with
a malformed `kind` the same way. Every caller that reads `manifest.site` today reads `sites` instead.
There are two of them: `site.create` and the shim's `resolve`, and `resolve` only needs the file to
parse.

**Writing.** Of the two forms, the writer uses:

| The file has | The project has | Written as |
| --- | --- | --- |
| neither | no site | nothing (today) |
| neither, or `[site]` | one site | `[site]` (today) |
| neither, or `[site]` | two or more | `[[sites]]`; an existing `[site]` becomes its first entry |
| `[[sites]]` | any number | `[[sites]]` |

One site stays `[site]` because that is the form every released build reads. A file that already
uses `[[sites]]` stays `[[sites]]` and does not switch back when a site is removed, so the diff
somebody reviews shows the change in sites and not a change of form. When `[site]` becomes the first
entry of `[[sites]]`, its keys, their order and their comments go with it, and the comment above the
`[site]` header moves to the first `[[sites]]` header. Comment placement is a `toml_edit` detail,
and the writer tests assert it (Testing).

### D2 — An entry belongs to the site that holds its `domain`, and the export owns the keys it writes

To update an entry, the export has to know which site it describes. **An entry belongs to the
project's site that holds its `domain`, whether as the primary or as an alias.** A domain belongs to
exactly one site in a home (`site_domains_domain` is `UNIQUE`, T39a), so at most one site can match.
Matching on any of the site's names, not just the primary, keeps a renamed primary working:
`blog.test → www.blog.test` with `blog.test` kept as an alias still finds the entry, and the export
rewrites its `domain` to `www.blog.test`.

The pass, in file order:

1. Each entry whose `domain` a not-yet-matched site holds is updated **in place**. It keeps its
   position, its comments and every key this export does not own.
2. Each site left over is **appended**, in `sites::records` order.
3. An entry that matches no site, or matches a site an earlier entry already took, is left exactly as
   written and reported (D4).

**Owned keys are written in full**, on the rule `aliases` and `routes` already follow ("a file that
disagrees with the home it came from"): `domain`, `aliases`, `doc_root`, `https`, `kind`, the
payload keys of every kind (`upstream`, `port`), `routes` and `services`. That includes removing
whatever a different kind left behind: a site that went from `reverse-proxy` to `static` loses its
`upstream`. Today's `[site]` writer leaves a stale `upstream` in that case, and this pass fixes it
for both forms. A key the export does not own, a `# comment`, or a key a later task adds stays
untouched.

### D3 — `services` names a site's links; `[[services]]` stays the project's list of what to have

In the home, a service is linked to a site (`site_service_links`). In the file, `[[services]]` is
top-level and carries what a colleague has to *have*: `name`, `instance`, `version`, plus a
hand-written `database`/`user`. With several sites the file has to say which site links which
service, so:

- **Each entry gets `services = [...]`**, a list of service ids (`mariadb@main`, `redis`) written in
  full from the site's links. An absent `instance` resolves the way it does in `[[services]]` today:
  the bare name first, then `name@main` (`sites.rs`, `linked`). An id doesn't have to appear in
  `[[services]]` to be valid. The syntax is refused when reading; whether the service exists is
  `site.create`'s `not_found`, as it is today.
- **`[[services]]` becomes the union of every site's links**, merged the way it is now: identity by
  `name` + `instance`, add and update, never delete, so a hand-written `database = "blog"` survives.
  This also fixes the second fault in *What this closes*. The merge no longer depends on there being
  exactly one site.
- **When an entry has no `services` key**, `site.create` falls back to every `[[services]]` entry.
  That is today's behaviour, so every file written before this task, and every hand-written `[site]`,
  keeps linking what it linked. `services = []` means none. Absent and empty are different here, on
  `ProjectUpdate`'s rule.

The export writes `services` in both forms, `[site]` included. A build from before this task ignores
the key and links every `[[services]]` entry. For a one-site file those are the same set, so nothing
changes for that build.

### D4 — `ProjectExport` says what the file holds that the home does not

`sites_omitted` stops being true: every site is written. It is **removed**, and in its place:

```rust
pub struct ProjectExport {
    pub path: String,
    pub created: bool,
    /// The `domain` of each site entry this home has no site for, left exactly as written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites_kept: Vec<String>,
}
```

These are entries a person wrote by hand, or entries for sites that have since been deleted here.
Export never deletes (D2, step 3), so it says they are there, in the same answer that shows it left
them. Removing `sites_omitted` is safe across versions. The member was optional, MixLab reads it as
`?? []`, and the only other reader is the daemon's own test. A new MixLab paired with an old daemon
just no longer shows the old sentence, and the old daemon is still the one that drops the sites.

`mix project export` prints the list: `kept 1 site entry this home has no site for: old.test`.
That closes the fourth fault in *What this closes*.

### D5 — `site.create` picks its entry: `from`, then the domain, then the only one

`site.create { project }` with nothing else is the import (T39a's D7). With several entries, "the
manifest's site" no longer names one. `SiteCreate` gains:

```rust
/// Which of the manifest's sites to fall through to, by any of its names.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub from: Option<String>,
```

The entry used for the fall-through is chosen in this order:

1. **`from` given:** the entry with that name as `domain` or alias. If none has it, `not_found`
   naming the domains the file declares.
2. **Otherwise, `domains` given:** the entry with `domains[0]` among its names. If none has it and the
   file declares **one** site, that site, which is today's behaviour. If none has it and the file
   declares several, **no entry**: the request describes a site the file does not, so the defaults
   apply and services fall back to none, not to every `[[services]]` entry. Guessing that a new site
   wants every service the project's other sites use is exactly the kind of invented default T39a's
   D10 refused.
3. **Otherwise:** the only entry, when there is exactly one, which is today's behaviour. No entry,
   when there are none: the defaults, with `[[services]]` as before. **Refused** when there are
   several: `invalid_argument`, "mixengine.toml declares 3 sites: ezportal.test, ezweb.test,
   ezwebsite.test", with the hint "`mix site create --from ezweb.test` adopts one". Creating
   `<slug>.test` here would invent a site in place of the three the file names.

Two entries naming the same domain make step 1 or 2 ambiguous. That is refused at the point of
choosing (`invalid_argument`, naming the domain and both positions), not in the reader, so a
duplicate in `[[sites]]` never breaks the shim's `resolve` for that directory.

Once the entry is chosen, the fall-through works field by field as it does today: an argument beats
the entry, and the entry beats the default. `mix site create` gains `--from <DOMAIN>`, and the
`import` alias it already has stays.

### D6 — No bulk import in this task, and why

A colleague with a three-site checkout runs `mix project create` and then three `mix site create
--from …`, instead of the "clone, two commands" the guide promises. Doing all three in one call
sounds simpler, but each `site.create` can fail on its own: a domain another project holds, a
service not declared here, a `.local` without `--i-know`. One call creating several sites then needs
a partial-success shape that does not exist yet. Either the whole set rolls back, which undoes hosts
entries and certificates already made, or the call answers per entry, which is a new response type
for both clients. Both are a method with a design of its own.

Looping in the client is ruled out: `mix` and MixLab only render what the daemon returns. So this
task gives each client what it needs to do it one site at a time and *see* what is left (D7). If the
three-command import turns out to be friction in practice, a `site.import` comes in a later task.

### D7 — `project.show` lists what the file declares, and which of it is here

To offer the import at all, a client has to know which sites the file declares. Today only `site.create`
reads `[site]`, and only at the moment of creating, so MixLab has no import path from the manifest.
`ProjectForm` sends `kind` and `https` every time, so the fall-through never reaches either.
`ProjectDetail` gains:

```rust
/// The sites the project's `mixengine.toml` declares, in file order.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub declared_sites: Vec<DeclaredSite>,

pub struct DeclaredSite {
    pub domain: String,
    pub aliases: Vec<String>,
    pub state: DeclaredSiteState,
}

#[serde(tag = "is", rename_all = "snake_case")]
pub enum DeclaredSiteState {
    /// A site of this project holds the domain.
    Here,
    /// No site in this home holds it; `site.create { from: domain }` adopts it.
    Missing,
    /// Another project's site holds it, or an extension's, so adopting it here would be
    /// `already_exists`. `owner` is that project's name or that extension's id.
    Elsewhere { owner: String },
}
```

On the wire that is `"state": {"is": "elsewhere", "owner": "shop"}`, the nesting `PinSource` already
uses under `source`. An entry with no `domain` is listed under the name `site.create` would give it,
`<slug>.test`, and `from` matches that name too (D5).

`project.show` already reads the manifest to report effective pins (`core::projects::effective_pins`).
That function hands its parse to the caller, so the file is read once, and the only new cost is one
domain lookup per entry. A file the reader refuses already fails `project.show` today,
because the pins come from the same parse, so that behaviour stays. `mix project show` prints the
list under the pins, and next to each `missing` entry prints `mix site create --from <domain>`.

## MixLab

- **Projects screen, project panel** (`screens/Projects/Projects.tsx`): under the pins, a
  *Sites in mixengine.toml* list rendered from `declared_sites`. A `missing` entry has an **Add**
  button that calls `site.create { project, from: domain }` with nothing else, so every field falls
  through to the file. An `elsewhere` entry names its owner and has no button. A `here`
  entry is plain. When the list is empty, the section is not shown. This is MixLab's first import
  path from the manifest, and it closes the gap described in D7.
- **Export notice:** `exportOmitted` (en, vi) is removed. When `sites_kept` is non-empty, the
  notice adds one sentence naming them and saying they were left as written, shown at warning level,
  because what it reports is a disagreement between file and home.
- **No new method**, so `check-client-surface` has nothing to add. `site.create` gains a member and
  `project.show`'s answer gains one. `bindings/` is regenerated (`packaging/bindings.sh`).
- **With MixEngine off**, the Projects screen is already one MixEngine draws from the daemon, and
  nothing here reaches outside the `mixengine` module.

## Testing

- **`core::manifest`, reader:** `[site]` gives one entry and `[[sites]]` gives its entries, in
  order. Both together is `Error::Manifest`. Each entry's `services`, when absent, is told apart from
  `[]`. A malformed id in `services` is refused. A file written before T135 (`[site]` with no
  `routes`) still reads.
- **`core::manifest`, writer:** every row of D1's table. `[site]` converted with its comment and an
  unknown key preserved. An entry found by an alias after the primary was renamed, and its `domain`
  rewritten. A stale `upstream` removed when the kind changed (both forms). An unmatched entry left
  byte for byte and reported. Two entries matching one site: the first updated, the second kept and
  reported. `[[services]]` written as the union and merged, with a hand-written `database` kept.
- **`site.create`:** each branch of D5, including the refusal text and its hint, the duplicate-name
  refusal, and the service fall-through for an entry with no `services` key, with `services = []`,
  and with no entry chosen.
- **`crates/mixengine-daemon/tests/projects.rs`:** the existing
  `an_export_writes_the_site_and_names_the_ones_it_could_not` is replaced by a round trip: a project
  with three sites of three kinds, each linking a different service, is exported. Then the project is
  deleted (the directory is kept) and re-created from the same root, and each site is created with
  `from`. The re-created sites must have the same domains, kinds, routes and links, and
  `project.show` must report every entry as `here`.
- **`mix`:** `render::project_export` with `sites_kept`, and `render::project_detail` with
  `declared_sites` in all three states.
- **MixLab:** the Projects panel renders the three states, Add sends `from` and nothing else, and the
  export notice shows kept entries. Vitest, beside the screen's existing tests.

## Documentation, when it lands

- [architecture/data-model.md](../architecture/data-model.md): the manifest example gains `[[sites]]`
  and `services`.
- `docs/guide/en|vi/projects-and-sites.md`: the adoption paragraph covers several sites (`--from`,
  `mix project show`, the Add button), and "two commands" becomes accurate again for one site.
  `docs/guide/en/cli.md`: `--from`.
- [features/blueprints.md](../features/blueprints.md): "`[[sites]]` is where that widens" now
  points at this design for the project manifest and at T204a for blueprints.
- `apps/desktop/CHANGELOG.md` and `CHANGELOG.md`: a project's every site is exported.
