---
description: Land the current branch — gate, push, ask CI, open a PR, fix CI until green, squash merge, clean up
argument-hint: [note, optional]
disable-model-invocation: true
---

# Land this branch

Branch: !`git rev-parse --abbrev-ref HEAD`
HEAD: !`git rev-parse HEAD`
Uncommitted: !`git status --porcelain`

Note: $ARGUMENTS

**Invoking this command authorizes**, for this branch only: commits, pushes, CI dispatches, one PR, its squash merge
and deleting the branch. Not authorized: force push, committing or pushing `master`, tags. CLAUDE.md rules apply.

1. **Preconditions.** On `master` or uncommitted changes → stop and ask.
2. **Ask CI.** `bash scripts/watch-ci.sh --once` first: exit 2 means HEAD already has a run, skip to step 4.
   Otherwise `bash scripts/ask-ci.sh` (it runs the gate; red gate → fix, commit, retry).
3. **PR.** `gh pr view` — reuse it if one exists, else `gh pr create --base master` with a `## Summary`-only body.
4. **Watch.** `bash scripts/watch-ci.sh` in the background. Red → classify and fix exactly as `/watch-ci` says
   (`.claude/commands/watch-ci.md`: its table, gate, red flags), one commit, back to step 2.
   Done only when an `all` run is green on HEAD.
5. **Merge.** Write `<type>(<scope>): <message> (#id)` to a file in the scratchpad, then via PowerShell:
   `gh pr merge <id> --squash --delete-branch --body-file <path>`.
6. **Clean up.** `git checkout master`, `git pull`, `git branch -D <branch>` if still there, `git fetch --prune`.

**Stop and ask** on: the same failure twice with nothing learned, a failure needing a secret or a person's setting,
a merge conflict with `master`.

**Report:** run id, PR link, squash commit SHA, one line per fix, which reds were flakes.
