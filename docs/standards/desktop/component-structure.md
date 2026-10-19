# Component folder structure

Every component lives in its own folder, named after the component (PascalCase). Which parent it
goes under is the question to settle first:

| Parent | For |
| --- | --- |
| `src/components/` | Primitives: no module's concepts in them, **and** another module would have a real use for them. |
| `src/shell/components/` | The shell's own — the settings dialog, the update toast. |
| `src/modules/<id>/components/` | Everything else. This is the default; the bar for the other two is deliberately high. |
| `src/modules/db/<engine>/components/` | A component only that engine draws — the Mongo document views, the Redis key list. Same folder shape. |

Two decided examples, for calibration. `JsonView` is a primitive: a read-only JSON viewer with no
BSON in it, which a REST client will want for a response body. `FilterBar` is not, despite the
general-sounding name — it is built from the SQL and Mongo operator lists, so a module that has
neither cannot use it.

A component under `src/components/` may not import from `src/modules/`. Nothing typechecks that, so
it is checked by grep — see [adding-a-module](adding-a-module.md).

The folder contains:
- `ComponentName.tsx` — the component implementation.
- `ComponentName.module.css` — the component's styles, following [[css-modules]]. A `composes: x
  from "…"` is a path **`tsc` does not check**: it fails at `vite build`, with a message naming
  neither the file nor a line. Moving a component means checking those by hand.
- `index.ts` — re-exports the component as the folder's public entry point.

Consumers must import the component via the folder path only, never by reaching into `ComponentName.tsx` directly. This keeps the folder's internal file layout free to change without breaking callers.

## State and effects

- **An effect depends on what it reads.** `react-hooks/exhaustive-deps` and `rules-of-hooks` stay
  on. When a dependency is a function rebuilt on every render, the fix is to make it stable
  (`useCallback`, or read it through a ref), not to turn off the lint. A disable is allowed only
  when its comment gives a reason a stable callback cannot fix, and the shared workaround for one
  reason lives in one hook, not in a copy per file (T209a).
- **States that change together are one value.** Loading, error and result, or a draft and whether
  it was edited, go into one `useReducer` or one discriminated union. With a separate `useState`
  for each, a combination that cannot happen can still be written. A component with more than
  about a dozen `useState` is the sign to look (T209f).
- Splitting a large component into hooks and parts follows the `splitting-a-react-component` skill
  in `.claude/skills/`, and is checked in the running window, not only by its tests.
