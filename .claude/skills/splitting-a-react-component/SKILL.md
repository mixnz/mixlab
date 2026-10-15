---
name: splitting-a-react-component
description: Use when splitting a large React component under apps/desktop/src into hooks and parts, or making any change there meant to leave the screen exactly as it was — and when a refactor "should not change anything" but a scroll position, a filter or a banner behaves differently afterwards.
---

# Splitting a React component

A component's callbacks close over its state, so a file past the size limit cannot be split by
moving lines. It becomes **hooks for what it does and parts for what it draws** (T207k, T208a).
That is a rewrite. Two things keep it a refactor: where each piece of state lives, and a recording
that proves the screen did not change.

## Where state and effects may go

| It moves into | Only when |
| --- | --- |
| a part (child component) | the part is mounted **exactly as long as** the parent: always drawn, not behind a condition the parent flips |
| a hook the parent calls | anything else, including state or effects about a part that mounts and unmounts |

A child that mounts later than its parent changes behaviour in two ways:

- **Its state resets.** A search box's text or a remembered scroll offset is lost every time the
  part unmounts behind a workspace.
- **Its effects replay under StrictMode.** The dev build runs setup, cleanup, setup on every mount.
  An effect that remembers "first run after mount" in a ref sees its own first run on the replay.
  In T208a, the saved list's layout effect moved into the list component. It put the offset back,
  then the replay scrolled to the marked row over it (526 → 30 after Disconnect). The effect went
  back into a hook `DbTab` calls, keyed on `connectionId` as before.

Two more rules:

- **Break cycles by passing values, not hooks.** When hook A clears hook B's feedback and B reads
  A's state, B takes what it read as an argument at call time (T208a's `connect(request)`).
- **A returned function listed in an effect is a `useCallback`** with the dependencies it had, so no
  effect runs at a moment it did not run before.

Write down every effect that now runs in a different order, and why nothing it reads changed.

## Proving the screen did not change

`npm run build`, `npm test` and `npm run lint` say nothing about behaviour. Record the DOM in the
demo before the first commit and after each one, and compare. [harness.js](harness.js) has the
helpers; copy it beside the step script you write for the screen.

1. **Serve the demo.** Run `npm run dev` in `apps/desktop` (port 1420) and open `/demo/demo.html`.
   Seed `localStorage` (`mixlab-modules`, `mixlab-theme`, `mixlab-lang`, and `mixlab-session` with
   the tab and its `state`), then reload.
2. **Put the script under the repository.** Vite serves the repository root through
   `/@fs/<absolute path>`, and nothing outside it. A git-ignored directory such as `.superpowers/`
   works: `await (0, eval)(await (await fetch("/@fs/C:/…/steps.js")).text())`.
3. **Stub only what the demo lacks, in the fixture's shape.** `window.__demo.unmocked` names every
   command nothing answered; for each, look in `demo/fixtures/*.ts` or `bindings/` first. A
   stub answering `secrets_load` with `{}`, where the fixture answers `{ password: "demo" }`, made a
   saved entry change under the form, which read as a regression until `master` did the same.
4. **Prove the recording is deterministic.** Run it twice on `master` and get no differences, then
   keep it in `sessionStorage`. A reload keeps that, and keeps the stubs out of the baseline.
5. **Check the reload served the new code** with `fetch("/src/…/File.tsx")`, looking for a new
   identifier: Vite rewrites relative imports to absolute paths, so `"./shape"` is never in it.
   Resource timing stops at 250 entries and cannot tell you.
6. **Bracket the console.** Errors from HMR while files were half-edited stay in the log, so read
   only what lies between two `console.error` marks around the run. Read React's warnings there
   too: T208b's run showed duplicate keys in a sizer row, an old bug the move had only carried.

A tab restored from the session is not mounted until it is shown. To see a screen come up under a
stub — the MixEngine gate with the daemon "stopped" — install the stub first, then click the tab.

Driving React from a script: set `value` with the native setter and fire `input`. Fire `dblclick`
and `contextmenu` with `dispatchEvent`. Find fields by their label text, which can change with
state ("Save as" on a new form, "Name" on a saved one).

## When a difference shows up

Find out whether it is old behaviour before fixing it. Check `master` out into a scratch worktree
(a literal path, never a variable). Give it the real `node_modules` as a junction and serve it on
port 1430, then run the same steps. Remove the junction with `(Get-Item <path>).Delete()` in
PowerShell **before** `git worktree remove --force`, or the removal walks into the real
`node_modules`.

## Before calling it done

- Every string literal of the old file is in the new ones. Check with
  `grep -o '"[^"]*"' old | sort -u` and `comm -23` against the new files.
- Comments that pointed at a position ("the effect above") and now cross files name what they meant.
- `npm run screenshots -- --check` renders every scene.
