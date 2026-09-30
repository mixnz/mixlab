# 0044. MixLab is the product, and MixEngine is the engine inside it

**Status**: Accepted — supersedes the naming paragraph of
[0027](0027-the-desktop-client-lives-in-this-repository.md); its decision 6 is superseded by
[0046](0046-the-sync-server-lives-beside-the-client-it-serves.md), which leaves no separate
repository to name; its decision 5 is superseded by
[0049](0049-a-download-is-named-after-what-it-installs.md), which names a download after what it installs
**Date**: 2026-09-20

## Context

[ADR 0027](0027-the-desktop-client-lives-in-this-repository.md) brought the desktop application into
this repository because *"Two products are what users are refusing"* — two installers, two cadences,
two updaters for one thing. It settled the packaging, and in the same paragraph it settled the
naming:

> The name is the window's alone: the daemon, the CLI, the helper, the shim, the home directory,
> `MIXENGINE_HOME`, the installers and the release feed keep MixEngine's name. MixLab is what a
> person opens; MixEngine is what runs underneath it and what a terminal calls.

**That left one product with two names, and neither reads as a part of the other.** "Docker Desktop"
and "Docker Engine" both plainly belong to Docker. `MixEngine` and `MixLab` share four letters and
no hierarchy: nothing in either word says which one contains the other, so a person who has heard of
one cannot find the other, and a person who has seen both asks whether they need to install two
things. Both questions arrive in practice.

**What people use daily is the window.** It is what they open, screenshot and recommend, and the
name they would type into a search box is MixLab — while the repository, the handbook's published
URL and the download page all say MixEngine.

**Three measurements made the change smaller than it looks.**

- The two longest-lived things on a user's machine already say MixLab: the desktop binary is
  `mixlab` in `MIX_BINARIES` (`packaging/common.sh`), and the credential store's service name is
  `MixLab` (`apps/desktop/src-tauri/src/secrets.rs`), with `<Old>` kept read-only beside it.
- The documentation already splits by audience. [../README.md](../README.md) says `guide/` is for
  *"whoever uses MixEngine"* and everything else is for *"whoever builds it"*. That is 32 files
  against 272.
- There is no installed base to carry. v0.0.1 through v0.0.6 are tagged and in nobody's use.

**And the timing is forced.** [T177](../specs/2026-09-20-t177-a-copy-only-you-can-read-design.md)
puts a name on a sign-in screen, in a verification email, on a self-hosting page and in a second
repository — places a name is expensive to retract from.

## Decision

**MixLab is the product. MixEngine is the engine inside it, and the name of the headless
distribution.**

1. **The umbrella is MixLab**: the repository (`mixnz/mixlab`), `README.md`, the installer's
   displayed name, and the handbook in `docs/guide/`.

2. **MixEngine is not retired, because it still names something real.** The daemon, the CLI, the
   helper, the shim, the ten crates, `MIXENGINE_HOME`, `Programs\MixEngine`, `@mixengine/api`,
   `mixengine-packages` and the 272 builder-facing documents keep the name. A daemon renamed after a
   window it does not have would be a worse name, not a tidier one.

3. **The download page answers the question in one line:** MixLab includes MixEngine; MixEngine on
   its own is for a machine with no screen. [ADR 0027](0027-the-desktop-client-lives-in-this-repository.md)
   rules 3 and 4 are untouched — a headless install stays a first-class product and never grows a
   window it did not ask for.

4. **`docs/guide/` is retranslated by hand, never by substitution.** Most of its 398 occurrences of
   *MixEngine* correctly name the engine; a blind replacement would produce "the MixLab daemon",
   which is wrong in a way the current text is not. Product name becomes MixLab; engine name stays.

5. **Release artifact names are identifiers and do not change.** `mixengine-<version>-<os>-<arch>`
   is globbed by `packaging/feed.sh`, rebuilt by `feed-check.sh` and `sign.sh`, and names the
   `mixengine/` directory inside the payload that `core::install` reads. Renaming it ripples into
   exactly the set decision 2 freezes, and buys a file name. What a person downloads is labelled
   MixLab by the page they download it from.

   **Whether the copies already installed keep updating is measured, not assumed.** A renamed
   repository redirects on GitHub, so `updates::feed::DEFAULT_URL` may well keep resolving; it may
   also not. Nothing here is built on either answer, because no copy of v0.0.1 – v0.0.6 is in use.
   The rename records what actually happens rather than predicting it.

6. **The sync server is `mixlab-sync`.** [ADR 0045](0045-mixlab-has-an-account-and-mixengine-does-not.md)
   rule 1 makes that account the toolbox's and not the daemon's, so naming its server after the
   daemon would contradict the decision it implements.

## What this does not decide

Domain names, a legal entity, pricing, and whether `Programs\MixEngine` moves. Each is a later
decision that adds to this one rather than reversing it, and none is written here so that none has
to be superseded to make it.

## Consequences

**Easy.** One name to search for, one name on the download page, and a repository named after what
it is for. The handbook's audience split does the scoping work: 32 files move, 272 stay correct
where they are. Nothing in the build, the installers' contents or the API changes.

**Hard, and accepted.** The handbook's published URL moves with the repository and no inbound link
survives it. The repository name and the crate prefix diverge — `mixlab` holding `mixengine-*` — the
way `moby/moby` holds Docker; a reader meets the engine's name in the source and the product's name
on the tin. And two names still exist. What changes is that one of them now plainly contains the
other, which is the whole of what this decision buys.
