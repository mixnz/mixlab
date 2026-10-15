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
