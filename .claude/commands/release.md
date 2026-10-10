---
description: Cut a release — readiness checks, version bump, gate, tag, watch the tag run to a draft
argument-hint: [version, e.g. 0.0.18 — defaults to the next patch after the latest tag]
disable-model-invocation: true
---

# Cut a release

Branch: !`git rev-parse --abbrev-ref HEAD`
Uncommitted: !`git status --porcelain`
Latest tag: !`git describe --tags --abbrev=0`

Version: $ARGUMENTS (empty → bump the patch of the latest tag)

**Invoking this command authorizes**, for this release only: commits on `master` (an upgrade fixture and the
release bump), pushing `master`, dispatching `release-rehearsal.yml`, pushing the tag, and rerunning a tag
run's failed jobs when the cause is infrastructure. **Not** authorized: publishing the draft, deleting a tag
or a release, or changing product code to fix a red run — stop and ask for those.
Follow `docs/operations/releasing.md` and the checklist in `docs/operations/build-and-release.md`; the
`windows-shell-choice` skill decides which shell runs what.

## 1. Preconditions

- On `master`, clean, and `git fetch` shows it in sync with `origin/master`. Otherwise stop.
- Stop any running `mixengined` / `caddy` before cargo.
- `## Unreleased` in `CHANGELOG.md` covers every user-visible change in `git log <latest tag>..HEAD`.
  A missing entry → stop and say which.

## 2. Readiness (run these in parallel with step 3)

- `cargo deny check` — advisories, bans, licenses, sources all ok.
- `cargo test -p mixengine-core --test index -- --ignored --nocapture` — passes; any empty cell must be in `KNOWN_EMPTY`.
- Schema: `git diff --stat <latest tag> HEAD -- crates/mixengine-core/migrations`. A new migration → capture
  `cargo run -p mixengine-core --example capture-upgrade-fixture -- <schema>` with its seed and commit it as
  `test(upgrade): capture the schema v<version> ships`. No new migration → nothing to capture.

## 3. Rehearsal

`gh run list --workflow release-rehearsal.yml --limit 3`. If the newest is not green **on the HEAD being
tagged** (the bump commit aside), `gh workflow run release-rehearsal.yml --ref master` and wait for it in the
background. It must be all green, with no `window` leg near its 45-minute timeout. Red → stop and report.

## 4. Bump and gate

1. `node scripts/set-version.mjs <version>`.
2. Insert `## v<version>` directly under `## Unreleased` in `CHANGELOG.md`.
3. Check the diff touches the same files as the previous `chore(release)` commit, then commit everything it
   changed as `chore(release): v<version>`.
4. `bash scripts/gate.sh < /dev/null` — must end with "the gate is green".

## 5. Tag

Only once steps 2–4 are green: `git push origin master`, `git tag v<version>`, `git push origin v<version>`.

## 6. Watch the tag run

Find the `ci.yml` run for the tag and watch it in the background, reporting each newly red job.
- **Red** → `ci-flake-triage` skill: read the job log first. Infrastructure (a 5xx from a feed or GitHub,
  a lost runner) → wait for the run to finish, then `gh run rerun <id> --failed`. Anything else → stop and
  report; taking back the tag (`releasing.md`) needs the user's yes.
- **Green** → `gh release view` the draft: it exists, is a draft, and has as many assets as the previous release.

## Report

Version, the bump commit, the tag run id (and any rerun with its cause), and the draft's URL. Say that
publishing it is the user's click.
