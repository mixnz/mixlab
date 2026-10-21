---
status: approved
date: 2026-10-21
task:
  - T211g
  - T211h
---

# T211g, T211h — One home undoes only its own, and six sentences that said the wrong thing (design)

Both found on the same Mac as T211e and T211i (macOS 15, two homes under one account, 2026-10-21).
T211g is what one home's privileged writes and its uninstall take from another home; T211h is six places
where `mix` or MixLab printed something untrue or doubled. They ship together because they were measured together and
will be measured again together, not because they share code.

## T211g — what a home applied is that home's to undo

### Measured, not assumed

Every privileged thing MixEngine writes is either **this home's** or **the machine's**:

| Residue | Who it belongs to today | Defect | Here |
| --- | --- | --- | --- |
| Trust store, browsers | per home: each home has its own authority, found by `key_id` | none | — |
| Port access, Linux | per home: a capability on this home's front-end binary | none | — |
| Hosts block | **machine**: one `# BEGIN MixEngine` block in the hosts file | one home's `hosts-apply` replaced another's `blog.test` with its own names; a home whose resolver is wired asks for the empty block, which empties it for every home | D2, D5 |
| Resolver wiring | **machine**: one file per TLD under `/etc/resolver`, one systemd-networkd link, one NRPT key, each naming one port | the inventory plans the revoke only when the wiring names this home's port, but `ResolverRevoke` removes every MixEngine-marked artifact whatever port it names (`macos/resolver.rs`'s `sweep`) | D3 |
| Privileged helper, audit log | **machine**: one `mixengine-elevate`, one `elevate.log`, at paths that do not depend on the build | removed by any home's uninstall | D4 |

**Two homes on one machine is the ordinary case, not the rare one.** ADR 0024 gives every build
that is not a release a home of its own, and `helper_path()` and `audit_directory()` do not depend
on the build. So every machine that develops MixEngine and uses it runs two homes on one helper,
one audit log and one hosts file — the machine that found this.

One property holds already and this design leans on it: **a surviving home repairs what it needs
at its next start.** `require_hosts`, `require_resolver` and the helper's install row run at every
daemon start and queue whatever this home's state says is missing. So taking away something another
home used costs that home its names until its next start and one more prompt. What cannot be put
back is the audit log.

### D1. The ADR: what a home applied is that home's to undo

ADR 0066 amends two accepted decisions, and is written before the code:

- **ADR 0051** — *An uninstall removes from the machine what this home applied and no other home
  uses. What another home still uses is kept, and the row says which home.* A finished uninstall
  may now end with `Kept` rows that nobody asked for; it still ends the daemon.
- **ADR 0005** — *Only the region between `# BEGIN MixEngine` / `# END MixEngine` may be touched*
  becomes *only a region MixEngine marked, and only the requesting home's*: one region per home,
  tagged (D2), plus the untagged region from before, which the helper only ever empties.

### D2. The hosts file holds one block per home

- The markers gain a tag: `# BEGIN MixEngine <tag>` / `# END MixEngine <tag>`. `<tag>` is
  `mixengine_platform::hosts::tag_for(home: &Path) -> String`: FNV-1a 64-bit over the home's
  canonical path, 16 lowercase hex digits. No hash crate: `mixengine-elevate` refuses `sha2` on
  principle (its manifest), FNV is ten lines, and the tag needs to be stable, not secret.
- **A path, not `HomeId`.** A home deleted by hand and created again in the same place keeps its
  tag, and its first apply overwrites the block it left. `HomeId` would be new, and the old block
  would stay for ever. A home moved elsewhere gets a new tag; D5 removes what it left.
- **The helper derives the tag itself** from `PrivilegedRequest.home`, which it already resolves
  and checks is the caller's (`request.rs`). `HostsApply` gains `home: Option<String>`: `Some(tag)`
  must equal `tag_for(request.home)` (or name an orphan, D5), `None` is the legacy block. The
  daemon cannot write another home's block by naming it.
- `markers.rs` learns tagged blocks: any number of distinct tags, a second block with the same tag
  is malformed as before. `HostsFile::managed(home: Option<&str>)` reads one block, so
  `require_hosts`, `doctor` and the uninstall inventory compare this home with this home.
- **The legacy block drains, and is only ever emptied.** A home's apply removes from it each name
  the same apply writes into its own block, and the legacy block goes when it is empty. The helper
  refuses `HostsApply { home: None }` with any entries. An uninstall asks for this home's block and
  the legacy block, as two queue rows (`hosts-apply`, `hosts-apply:legacy`): the legacy block was
  written by whichever home applied last before the update, and keeping it would keep it for ever.

### D3. The resolver revoke names the port

`ResolverTarget::ResolverDirectory` and `ResolverTarget::SystemdLink` gain `port: u16`, and the
helper removes an artifact only when it names `127.0.0.1:<port>`:

- macOS: `sweep` keeps every file whose contents name another port.
- Linux: the systemd-networkd files go only when their `DNS=` names this port.
- Windows: unchanged. NRPT has no field for a port, which is why T44 puts the server on 53 there,
  so only one home on a machine can be wired at a time and the one rule is that home's.

The inventory already plans the revoke only when the wiring names this home's port, so the daemon
side changes only in `operation_for`, which builds the target with the port it probed. No ledger
entry: the port in the artifact is the evidence.

**Who pays.** Two homes on the same DNS port, never running at once, share one wiring. Whichever
uninstalls first removes it, and the other queues it again at its next start.

### D4. The helper keeps a ledger of the homes it serves

The helper and the audit log have no port to compare and no block to tag. Only the helper sees every
home that uses it, because every request names its home, so the helper keeps the record:

- **`homes.json`** beside `elevate.log` in the audit directory, root-owned, `0644`, replaced
  atomically under its own lock (`homes.lock`). One entry per home path:
  `{ "home": "<canonical path>", "caller": "<Owner, as the audit log writes it>", "last": <unix seconds> }`.
- **Every batch records its home before any operation**, so a batch that fails still says who
  asked. A ledger that cannot be written costs a line on stderr, as an audit line does, and never
  fails the batch.
- **`HelperRemove` and `AuditLogRemove`** read the ledger and drop entries whose directory is gone
  (`symlink_metadata`, a symlink counts as gone). They act only when no **other** entry remains.
  Otherwise they answer the existing `OpOutcome::Unmanaged`: `reason` names the other homes,
  `manual` is the command that removes the file by hand. `Unmanaged` already means *neither applied
  nor failed, and here is what to run* (T74), and the audit log already records it.
- **A ledger this batch created keeps both.** It has seen only this home and says nothing about
  homes from before it, so the helper removes itself and the log only when `homes.json` existed
  before the batch. A single-home machine that has granted anything since the update removes them
  as before; the helper's own replacement is such a batch.
- **A ledger that cannot be read keeps both**, with the reason. Keeping a file is recoverable;
  removing another home's helper and log is not.
- `AuditLogRemove` that does act removes `homes.json` and `homes.lock` with the log.
- The helper decides this itself, from the disk, rather than trusting the daemon's plan — the rule
  `mixengine-elevate` already lives by. The daemon reads the same file (world-readable) to plan, so
  `mix uninstall` shows `Kept` before `continue?` rather than after.

On the daemon's side:

- The inventory's helper and audit-log rows are `Removal::Kept` with the helper's sentence when the
  ledger lists another living home, and when there is no ledger.
- The T182b rule *every batch removes the log* becomes *every batch asks to*: the auto-appended
  `AuditLogRemove` is still sent, and the helper keeps the log when another home is listed.
- `uninstall::outcome::settle` reads a second-reading `Kept` as the answer. Without it, a helper row
  planned for removal that the helper kept (another home granted in between) would read as
  *removed at the next restart* on Windows and *failed* elsewhere.
- A finished uninstall whose only remaining rows are `Kept` ends the daemon, as ADR 0051 says for
  `--keep-home`.

**Who pays.** A second home that has asked for nothing since the update is not in the ledger, and an
uninstall of the first home will remove the helper and the log under it — once the ledger exists,
which takes a grant after the update. Its next start queues the helper's install again; the log is
gone. The gap closes the first time each home grants anything.

### D5. Orphan blocks

A tagged block whose home is gone — deleted by hand, moved — has no home left to remove it. Every
tagged block was written by a batch that recorded its home first (D4), so the ledger names it:

- `mix doctor` reports a tagged block whose tag matches no ledger entry with a living directory, as
  `hosts_block_orphaned`, naming the names it holds.
- `mix doctor --repair` queues `HostsApply { home: Some(<orphan tag>), entries: [] }`, its own row
  in the queue (`hosts-apply:<tag>`).
- The helper accepts a tag that is not the caller's only for an empty apply, and only when the
  ledger has no living home with that tag. A tag the helper cannot place (no ledger) is refused.

### D6. The helper's version

`HostsApply.home`, the resolver target's port and the ledger change what the helper reads and
writes, and `HostsApply` and `ResolverRevoke` refuse unknown fields: `HELPER_VERSION` moves past
whatever has been released (`packaging/helper-lock.sh --bump` if it says so), and the replacement
of an older helper runs first in its batch, as T182b arranged, so no older helper is handed the new
shapes. No new `OpOutcome` variant (D4 reuses `Unmanaged`).

### D7. What `mix uninstall` prints

`Kept` rows already print their sentence. The last line, which today says the machine is clean,
says *"N things were kept because another home on this machine uses them"* when any row is `Kept`
for that reason. The daemon counts them (`UninstallReport.kept_for_others`); `mix` prints the
number and decides nothing.

## T211h — six sentences

| # | Where | What it said | Change |
| --- | --- | --- | --- |
| 1 | `helper.rs` `upgrade_sentence` | *"…MixEngine replaces it at the next permission prompt"* when `keep_in_step` could not stage the helper (a package-managed copy, a feed that did not answer) and queued nothing | `keep_in_step` keeps its reason in `Elevation`'s state; the sentence says *"…and it stays as it is: <reason>"* when no `HelperReplace` or `HelperInstall` is queued |
| 2 | `mix job status`, `mix job list` | a succeeded job at 40% or 70%, with its last step under *doing* | `mixengine_core::jobs::finish` sets `percent = 100` on success only; a failed or cancelled job keeps where it stopped (the existing test's reason). `job_status` prints *doing* only while running, and *stopped at* for a failed job |
| 3 | `mix doctor --repair` | *"waiting  the managed hosts block / the managed hosts block"* | `repair.rs`'s `Enqueue::Hosts` sentence says what will be done: *"rewriting it to hold this home's names"* |
| 4 | MixLab's elevation dialog, `hosts-apply` detail | `127.0.0.1` twice, no names | `pendingOps.ts` reads `domain`, not `name` — `HostEntry` in `bindings/` has no `name` — typed against `PrivilegedOp` instead of `unknown`; the test that used `name` is corrected |
| 5 | MixLab's elevation dialog, lead | *"Everything below will be changed in one prompt"* where `canPrompt` is false | a third lead, *"These changes need an administrator, and this window cannot ask for one here."* (the *cannot prompt* line below it already says why) |
| 6 | MixLab's elevation dialog, lead | *"Anything marked below waits for this screen"* where no row is marked | `leadWhileTerminalWaits` loses its second sentence, which becomes its own key shown only when some row has `needs_the_screen`; the choice of lead is a pure `leadKeys` beside the dialog, so it is tested |

New and changed strings go into `en` and `vi` together (`writing-user-facing-text`).

## Not here

- **Two homes fighting over the resolver**: both running, on different ports, each start rewires
  `.test` to itself. That is a question of which home owns `.test` while both run, not of a write
  reaching too far.
- **Machine-wide firewall rules and macOS's pf anchor**: one label and one anchor for every home,
  the same shape as the hosts block. Not measured; the ledger D4 adds is what a fix would build on.

## MixLab

T211h's rows 4–6 are MixLab's half: the elevation dialog names hosts entries by their domain, and
its lead matches what the window can do and what is marked. T211g has no screen of its own: uninstall
is not a screen of MixLab (ADR 0051); the elevation dialog lists a `hosts-apply` row's entries
whatever its new `home` field says. A kept helper or log arises only inside `daemon.uninstall`,
which MixLab does not call. `hosts_block_orphaned` is one more `ProblemId`: Settings → MixEngine's
doctor section prints a finding's `because` as the daemon sends it, and its Repair button already
calls `daemon.doctor_repair`, so the window repairs an orphan block as `mix doctor --repair` does. No new method, so `check-client-surface.mjs` is unaffected.
