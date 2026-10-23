---
status: approved
date: 2026-10-23
task: T209f
---

# Forms that hold their state as one value (design)

Six components hold their state in 15 to 24 separate `useState`. Several of those values only make
sense together: a list and whether it has been read yet, a dialog's target and the hint it is
showing, the answers to a plan and the plan they answer. Held apart, a combination that cannot
happen can still be written, and one of them already is (D2).

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209f. The rule for
new code is in [`docs/standards/desktop/component-structure.md`](../standards/desktop/component-structure.md),
*State and effects*. T209a, which touched `RedisWorkspace.tsx` before this, has landed.

## What was measured

`grep -o "useState[<(]"` per file in `apps/desktop/src`, on `master` at `6bf29a25`. The roadmap's
numbers are one higher each: they counted the import line.

| File | `useState` | Lines |
| --- | --- | --- |
| `modules/mixengine/screens/Packages/Languages.tsx` | 24 | 796 |
| `modules/terminal/components/TargetForm/TargetForm.tsx` | 20 | 747 |
| `modules/db/redis/RedisWorkspace.tsx` | 18 | 648 |
| `modules/db/sql/SqlWorkspace.tsx` | 17 | 862 |
| `modules/mixengine/screens/Extensions/Extensions.tsx` | 15 | 584 |
| `modules/mixengine/screens/Blueprints/ApplyDialog.tsx` | 15 | 665 |

Nothing under `src/` uses `useReducer` yet, so this design also sets the shape the first ones take.

## D1 — the shape

- **A read and whether it has answered is one value: `T | null`.** `null` is "not read yet", and the
  loading state draws on it. A read that fails keeps what it had, or an empty `T` the first time, so
  the screen draws exactly what it draws today once the spinner goes.
- **The dialogs a screen can open are one union, `dialog: … | null`.** At most one is open, and the
  values a dialog carries (its target, its hint, a checkbox inside it) live in its arm.
- **A form's fields are one draft, changed by a reducer.** The reducer and its actions sit in a
  `.ts` file beside the component, pure and tested with vitest, as the module's other `*State.ts`
  files are. `dispatch` is stable, so a function that only dispatches needs no ref to be read from
  an effect.
- **Values that change on their own stay `useState`**: a filter box, an expanded row, a banner, the
  job list the watch feeds. Folding them in buys nothing and makes every change a dispatch.

No effect runs at a moment it did not run before, except where D2 says so. Every effect whose
dependency list changes is written down under its file, with why what it reads did not change.

## D2 — what each file becomes

### `Languages.tsx` — 24 to 12

| Today | Becomes |
| --- | --- |
| `installed`, `onDisk`, `installedLoaded` | `local: { installed, onDisk } \| null` |
| `available`, `stale`, `unavailable`, `updates`, `availableLoaded` | `catalogue: { available, stale, unavailable, updates } \| null` |
| `devkit`, `devkitHere` | `devkit: { offer, here }`, each half set by its own read as today |
| `uninstallTarget` + `forceHint`, `asking`, `upgrading`, `askingUpgrade`, `removingDevkit` | `dialog`: `uninstall` (target, hint), `requirements` (release, step), `upgradePlan` (update, plan), `upgradeRequirements` (update, keep, step), `removeDevkit` (version) |

`jobs`, `installingJob`, `expanded`, `filter`, `openLines`, `error`, `notice` and `adopting` stay.
The union changes one thing: today the requirements question of an install whose answer arrives
after another dialog was opened draws a second dialog over the first. Now the later one replaces it.

### `TargetForm.tsx` — 20 to 3

| Today | Becomes |
| --- | --- |
| `kind`, `name`, `runOnConnect`, `onRestore`; `shellName`, `cwd`, `envText`, `pathText`, `advanced`; `host`, `port`, `username`, `authType`, `password`, `keyPath`, `passphrase` | `draft`, through `targetDraft.ts`: `edit` (one field), `apply` (a saved entry), `clear`, `defaults` (the shell and directory the settings name, only into empty fields) |
| `targetId`, `savedSnapshot`, `stale` | `held`: `{ kind: "none" }`, `{ kind: "removed" }` (the notice is up), `{ kind: "held", id, snapshot \| null, changedElsewhere }` |

`shells` stays. `applyTarget`, `clearForm` and `resetSshFields` become reducer cases, so
`applyLatest` and its layout effect go. `buildTarget`, `buildChoice` and `snapshotOf` move into
`targetDraft.ts` with tests: they read only the draft.

**A bug this fixes**, reachable today: the entry is deleted in another tab, the form says *removed
elsewhere*, and the person presses Save. The form now holds the new entry and says Update, and the
*removed elsewhere* notice is still up beside it. Saving moves `held` to
`{ kind: "held", changedElsewhere: false }`, so the notice does not outlive the save. That is a line
in the CHANGELOG's `### Fixed`.

The other pair looked the same and is not: when the entry changes elsewhere while the form holds
edits and the person presses Update, the store's new entry equals the form, `savedChange` answers
`reload`, and that clears the notice. The demo recording shows it gone on `master`. The reducer
keeps that path.

### `RedisWorkspace.tsx` — 18 to 10

| Today | Becomes |
| --- | --- |
| `keys`, `cursor`, `scanDone`, `keysLoading`, `loadingMore` | `scan`, through `scanState.ts`: `started`, `published` (keys, cursor, done), `finished`, `moreAsked`, `moreLanded`, `moreEnded` |
| `selectedKey`, `groupPrefix`, `contentMode` | `content: { key, group, mode }` with `mode: "group"` only when `group` is set: `selectKey`, `openGroup`, `closeGroup`, `showTab`, `databaseChanged`, `keysDeleted` |
| `pattern` (applied) + `scanId` | `request: { pattern, id }` |
| `databases`, `databasesLoading` | `databases: { list, loading }` |

`pattern` (the box), `separator`, `keyLimit`, `reloadFocus`, `serverInfo` and the workspace error
stay. `scanRun` stays a ref: it is what makes a late page from an abandoned sweep land nowhere, and
nothing in state can do that. The scan effect reads `request` instead of `appliedPattern` and
`scanId`; it runs on the same presses, because `rescan` replaces `request` exactly when it set both.

### `SqlWorkspace.tsx` — 17 to 10

| Today | Becomes |
| --- | --- |
| `creatingDatabase`, `creatingTable`, `renamingTable`, `droppingTable` | `dialog`: `createDatabase`, `createTable`, `renameTable` (table), `dropTable` (table) |
| `transferStatus`, `transferProgress` | `transfer: { label, progress } \| null` |
| `selectedTable`, `pinnedTable` | `selection: { table, pinned }`, through `tableSelection.ts`, with `pinned` never set when `table` is not |
| `databases`, `databasesLoading` | `databases: { list, loading }` |

The transfer effect stops setting the progress back to `null` itself: closing the overlay drops the
whole value. It still registers the listener only while a transfer runs, and depends on whether
there is one, not on its label.

### `Extensions.tsx` — 15 to 8

| Today | Becomes |
| --- | --- |
| `installed`, `available`, `sites`, `unreadable`, `stale`, `allServices`, `loaded` | `lists: { … } \| null` |
| `installingSource`, `uninstalling` + `deleteData` | `dialog`: `install` (source), `uninstall` (row, deleteData) |

`serviceState`, `acting`, `jobs`, `installingJob` and `error` stay: the watch changes them between
reads.

### `ApplyDialog.tsx` — 15 to 8

`phase` is already a union. The answers to a plan move into it, because they mean nothing outside it
and `preview()` resets all six at once:

```ts
| { kind: "plan"; plan; needs; answers: { choices, scaffold, dotenv, prerequisites, devkit, devkitAgreed } }
| { kind: "running"; jobId; cancelAsked }
| { kind: "done"; applied; cancelled }
```

The devkit listing that answers after the plan is drawn is applied only to the plan it was asked
for, and dropped once the apply has started, which is what it already meant: the dialog has no way
back from a plan to the form, and `apply` reads the offer when it is pressed. The job and log effects depend on
the running job's id rather than on `phase`, so ticking *Cancel* (a new `phase` object) does not
empty the log and open its stream again.

`project`, `root`, `busy`, `error`, `jobs`, `showLog` and `logEntries` stay.

## D3 — the files not named in the task

The same count finds five more past a dozen: `db/mongo/components/NoSqlTable/NoSqlTable.tsx` and
`db/mongo/MongoWorkspace.tsx` (15), `mixengine/screens/Sites/Sites.tsx` (14),
`shell/components/SettingsModal/SyncSection/SignInForm.tsx` and
`mixengine/screens/Projects/Projects.tsx` (13). This task looks at each with D1's rules and changes
those where a pair can be written that cannot happen, starting with `MongoWorkspace.tsx`, which has
the same dialogs and database list as the SQL workspace. One whose values all change on their own is
recorded in the phase file as checked.

## Verification

- `npm run build`, `npm test` and `npm run lint` in `apps/desktop`, with the new `*.test.ts` files:
  every reducer case, and for each bug in D2 the sequence that reproduced it.
- In the running window, per the `splitting-a-react-component` skill: a DOM recording of each screen
  in the demo, before the first commit and after each one, with no difference outside D2's changes.
- `npm run screenshots -- --check`.
- `node scripts/check-file-sizes.mjs`: `SqlWorkspace.tsx` (862) and `Languages.tsx` (796) must not
  grow past 1,000; each should shrink.

## MixLab

All of this is MixLab's window. The terminal and `db` changes are in modules that never reach the
daemon and work with MixEngine switched off. `Languages`, `Extensions` and `ApplyDialog` are
screens of the `mixengine` module and send the same requests as before; no daemon method is added
or changed.

## Not in scope

- Splitting any of the six into hooks and parts beyond what moving the reducers out does.
- A rule or lint that counts `useState`. The standard says "about a dozen" is a sign to look, not a
  limit.
