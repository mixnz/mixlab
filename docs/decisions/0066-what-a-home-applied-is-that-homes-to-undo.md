# 0066. What a home applied to the machine is that home's to undo

**Status**: Accepted. It amends [0051](0051-an-uninstall-ends-what-it-undoes.md) — an uninstall no
longer removes what another home on the machine still uses — and
[0005](0005-on-demand-elevation.md)'s rule that only the region between `# BEGIN MixEngine` and
`# END MixEngine` may be touched: there is now one such region per home.
**Date**: 2026-10-21

## Context

MixEngine's privileged writes were designed for one home on one machine. Several of them are the
machine's rather than the home's: one block in the hosts file, one resolver wiring per TLD, one
installed `mixengine-elevate`, one `elevate.log`. Every home that runs on the machine shares them.

Two homes on one machine is the ordinary case, not the rare one. [ADR 0024](0024-a-build-that-is-not-a-release-keeps-its-own-home.md)
gives every build that is not a release a home of its own, and the helper's path and the audit
directory do not depend on the build. So every machine that develops MixEngine and uses it runs two
homes over one helper, one log and one hosts file.

Measured on a Mac with two homes on 2026-10-21: one home's `hosts-apply` replaced the other's
`blog.test` with its own names; a home whose resolver was wired asked for the empty block and
would have emptied it for both; `ResolverRevoke` removed every MixEngine resolver file whatever
port it named; and `mix uninstall` of either home removed the helper and the audit log the other
still used. The helper comes back at the other home's next start, at the cost of a prompt. The log
does not come back.

## Decision

1. **An uninstall removes from the machine what this home applied and no other home uses.** What
   another home still uses is kept, and the row says which home. A finished uninstall may end with
   such `Kept` rows; it still ends the daemon, as 0051 says of `--keep-home`.
2. **Only the helper can know which homes use it,** because every request it receives names its
   home. It keeps a ledger of those homes beside the audit log, records each batch's home before
   any operation, and removes itself and the log only when no other home in that ledger still
   exists. It decides this from the disk, not from the daemon's plan, which is the rule 0005 set
   for everything the helper does.
3. **The hosts file holds one region per home.** The markers carry a tag the helper derives from
   the requesting home's path, so a daemon cannot write another home's region by naming it. The
   untagged region from before is read, drained as homes write their own, and only ever emptied.
   0005's rule becomes: *only a region MixEngine marked, and only the requesting home's.*
4. **A region whose home is gone may be emptied by another home,** and only when the helper's
   ledger says that home no longer exists. Without this, a home deleted by hand would leave its
   names pointing at loopback for good.

## Consequences

- The helper keeps state between runs for the first time: `homes.json` in the audit directory,
  root-owned and world-readable, like the log. A ledger that cannot be read keeps the helper and
  the log; keeping a file is recoverable, removing another home's is not.
- A ledger the current batch created has seen only this home, so it removes nothing. A home that
  has asked for nothing since the update is not in the ledger, and an uninstall of another home can
  still remove the helper and the log under it. The gap closes the first time each home grants
  anything.
- The hosts file's format changes. An older build reads no region of its own in a file written by a
  newer one and writes an untagged region again; the next newer apply drains it.
- The design, and what it changes in each component, is the
  [T211g design](../specs/2026-10-21-t211g-t211h-one-home-undoes-only-its-own-design.md).
