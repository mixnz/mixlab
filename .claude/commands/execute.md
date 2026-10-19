---
description: Execute a plan on its own branch — no subagents, one commit per task, straight through to the end
argument-hint: <plan path, optional>
disable-model-invocation: true
---

# Execute a plan

Branch: !`git rev-parse --abbrev-ref HEAD`
Uncommitted: !`git status --porcelain`

Plan: $ARGUMENTS

**Invoking this command authorizes**, for this plan only: creating one branch and one commit per task on it.
Not authorized: push, CI dispatch, PR, merge, committing `master`. Every rule in CLAUDE.md still applies —
commit format, branch naming (never "claude"), LF line endings.

1. **Plan.** Use the path above; if empty, take the most recent plan under `docs/plans/` and say which one.
   Uncommitted changes that are not the plan's → stop and ask.
2. **Branch.** Cut a new branch off the current `master` named for the plan, unless already on a branch made for it.
3. **Execute.** Use `superpowers:executing-plans`, implementing every task yourself — **never spawn a subagent**,
   whatever a skill says. Run the tasks in order, straight through to the end, without pausing for feedback
   between tasks.
4. **One commit per task.** Gate each task with CLAUDE.md's *Common commands* for what it touched, then commit
   that task alone. Tick the roadmap phase file and flip the spec's status where the plan says so.

**Stop and ask** only on: a failure you cannot resolve yourself, the same failure twice with nothing learned,
or a plan step that contradicts a spec or CLAUDE.md.

**Report:** branch name, one line per task with its commit SHA, anything skipped and why.
