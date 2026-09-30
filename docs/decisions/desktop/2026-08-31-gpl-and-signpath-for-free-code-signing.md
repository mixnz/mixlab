# GPL-3.0, so that code signing can be free

The standalone client is licensed GNU GPL-3.0-or-later. The licence was not chosen on its own merits: it was chosen
because it is the entry ticket to free code signing, and that in turn was chosen because the
unsigned-binary warning is the first thing a new Windows user meets.

## What forced the question

A Microsoft Store submission needs a signed installer. The Store does **not** sign EXE/MSI apps —
that is an MSIX-only benefit — and its requirements say the binary and every PE file inside it must
be signed by a certificate chaining to the Microsoft Trusted Root Program. Self-signed is refused.
So the Store did not merely make signing desirable; it made it a precondition.

## The three ways to get a certificate, and why this one

- **A commercial OV certificate** — roughly $200–400 a year, no strings on the project. The
  fallback if the chosen path fails.
- **Azure Artifact Signing** (formerly Trusted Signing) — $9.99 a month, far cheaper, but sold to
  organisations and individuals in the USA and Canada, and to organisations in the EU and UK. Not
  available to an individual in Vietnam, which is the end of it.
- **SignPath Foundation** — free for open-source projects, an OV-level certificate with the key in
  an HSM. Its conditions are the ones a project can actually meet by choosing to.

The SignPath condition that costs something is the licence: *an OSI-approved Open Source licence
without commercial dual-licensing, for all components*. Everything else the project already
satisfied.

## Why GPL-3.0 and not MIT or Apache-2.0

All three satisfy SignPath. The repository was already public, so what a licence changes here is
not whether anyone can *read* the code — that was given up already — but whether they can reuse it.
GPL keeps the most: a fork stays open, and nobody can take the standalone client closed and sell the closed version.

The dependency tree permitted the choice rather than forcing it. A scan of all 766 crates and 26
shipped npm packages found nothing copyleft and nothing proprietary — MIT, Apache-2.0, BSD, ISC,
Zlib, MPL-2.0, BSL-1.0, CC0 and the OFL for the bundled font. Two consequences worth keeping:

- Nothing obliged the project to go copyleft. GPL is a choice, not an inheritance.
- Apache-2.0 covers roughly 460 of those crates, and Apache-2.0 is compatible with GPL**v3** but
  not with GPLv2. The version number is not a preference; v2 was never available.

## What this forecloses

Dual licensing. The same source cannot also be sold under a separate proprietary licence — that is
precisely what SignPath excludes.

Selling the standalone client is *not* foreclosed. Open source does not mean free of charge, and a paid Store
listing over GPL source is lawful. It is only commercially weak, because anyone may rebuild or
redistribute what they receive. That is a business limit, not a licence violation, and the
distinction matters if the question is ever revisited.

## Distributing GPL software through the Microsoft Store

There is a known hazard here: Apple's App Store terms once forced GPL apps off the store. Microsoft
avoided it deliberately — their terms let a developer's own licence conflict with the Standard
Application License Terms *to the extent required by the FOSS in the app*.

That exemption only applies if custom licence terms are supplied on the listing. Left at the
default, every Store app falls under the Standard Application License Terms, and those do conflict
with the GPL. So a Store submission must point at the GPL explicitly; accepting the default would
put the listing in breach.

## Where the rest lives

The operational half — how signing is wired into the release, and the ordering trap that will break
the updater if it is got wrong — was in `docs/RELEASING.md` under *Signing*, and the public
statements were `site/code-signing/` (required by SignPath) and `site/privacy/`. All three stayed
behind in the archived `mixnz/<old>` repository when the application moved here
(ADR 0027, phase 11): this decision was never enacted — the licence stayed MIT OR Apache-2.0 —
and code signing remains parked for MixEngine as a whole.
