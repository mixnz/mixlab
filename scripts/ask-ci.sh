#!/usr/bin/env bash
# Ask CI for an answer about a branch, which is the only way to get one.
#
#   scripts/ask-ci.sh                 # the branch you are on, every job
#   scripts/ask-ci.sh some-branch     # a branch by name
#   scripts/ask-ci.sh --watch         # request it, then wait for the verdict
#   scripts/ask-ci.sh --jobs test     # `test`, `services` and `rustdoc`; every other job is skipped
#   scripts/ask-ci.sh --no-gate       # push without running `scripts/gate.sh` first
#
# **`scripts/gate.sh` runs before the push**, and a red gate pushes nothing: rustfmt, clippy,
# rustdoc, the helper's version and the documentation links are all answered here in about a minute,
# where CI answers them in twenty. `--no-gate` is for a push whose point is CI itself — a workflow
# being edited. A branch named that is not the one checked out is pushed without the gate, since the
# working tree is the only one it can check.
#
# **`--jobs` narrows the question, and a narrowed answer is not an answer about the workspace.**
# The groups are the job names in `ci.yml` — `lint test system bench bindings docs desktop build install` —
# and `all` is the default, so a run covers everything unless somebody asked it not to. It is there
# for the loop where one job is red and the other eight have nothing to say about it yet: ask for
# that job, fix it, and ask for `all` before believing anything.
#
# `ci.yml` fires by itself on a `v*` tag and on nothing else, so every branch — `master` too —
# has to ask. This is the two commands that asking is: push the ref, then dispatch a run on it.
#
# **The push is not a convenience.** `gh workflow run --ref` names a ref on the *remote*, so a
# dispatch without it builds whatever the remote last saw, silently and with a plausible-looking
# green tick. Pushing first is what makes the run about the commit you are looking at.
#
# **`ci.yml` is read from the default branch.** A workflow only becomes dispatchable once it is on
# `master`; a branch that edits `ci.yml` runs its own copy once selected, but it cannot introduce a
# workflow that master has never seen.
#
# Exit status: 0 when the run was requested, 1 when something refused, 64 for a misuse of this
# script — so it is never read as a report about CI.

set -uo pipefail

watch=0
gate=1
branch=""
jobs=""
want_jobs=0
for arg in "$@"; do
  if [ "$want_jobs" -eq 1 ]; then
    jobs="$arg"
    want_jobs=0
    continue
  fi
  case "$arg" in
    --watch) watch=1 ;;
    --no-gate) gate=0 ;;
    --jobs) want_jobs=1 ;;
    --jobs=*) jobs="${arg#--jobs=}" ;;
    -*) echo "unknown option: $arg" >&2; exit 64 ;;
    *) branch="$arg" ;;
  esac
done

if [ "$want_jobs" -eq 1 ]; then
  echo "--jobs needs a group: all lint test system bench bindings docs desktop build install" >&2
  exit 64
fi

# **Refused here rather than by GitHub.** A `choice` input rejects an unknown value with an API
# error that says nothing about which words are allowed, and a typo should not cost a round trip.
case "$jobs" in
  "" | all | lint | test | system | bench | bindings | docs | desktop | build | install) ;;
  *)
    echo "unknown job group: $jobs" >&2
    echo "one of: all lint test system bench bindings docs desktop build install" >&2
    exit 64
    ;;
esac

[ -n "$branch" ] || branch="$(git branch --show-current)"
if [ -z "$branch" ]; then
  echo "not on a branch, and none was named" >&2
  exit 64
fi

if [ "$gate" -eq 1 ]; then
  if [ "$branch" != "$(git branch --show-current)" ]; then
    echo "not running the gate: $branch is not the branch checked out here" >&2
  else
    if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
      echo "the gate checks the working tree, which has changes that will not be pushed" >&2
    fi
    bash "$(dirname "${BASH_SOURCE[0]}")/gate.sh" || {
      echo "nothing was pushed; fix the above, or pass --no-gate" >&2
      exit 1
    }
  fi
fi

git push origin "$branch" || exit 1

if [ -n "$jobs" ] && [ "$jobs" != "all" ]; then
  gh workflow run ci.yml --ref "$branch" -f jobs="$jobs" || exit 1
  echo "requested a run on $branch — the $jobs job only, every other one skipped"
  echo "a green answer here is about $jobs and not about this workspace; ask for all before believing it"
else
  gh workflow run ci.yml --ref "$branch" || exit 1
  echo "requested a run on $branch"
fi

if [ "$watch" -eq 1 ]; then
  # Through `bash` rather than by executing it: `watch-ci.sh` is committed 100644, so a checkout on
  # Unix has no execute bit to rely on.
  exec bash "$(dirname "${BASH_SOURCE[0]}")/watch-ci.sh"
fi

echo "watch it with: bash scripts/watch-ci.sh"
