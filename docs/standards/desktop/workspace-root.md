# The root element of a workspace

Every pane a module tab renders as its whole body — a workspace, or anything standing in for one —
sits in `.tab-panel`, and `.tab-panel` is a **row-direction flex container** (`shell/chrome.css`, with
`display: flex` set inline by `Workspace.tsx` on the active tab). That one fact is the whole of this
convention.

Give the root element this block:

```css
box-sizing: border-box;
width: 100%;
height: 100%;
min-width: 0;
min-height: 0;
```

`.sql-workspace`, `.mongo-workspace` and `.redis-workspace` in `src/modules/db/db.css` all carry
it. Copy it whole.

## Why each line, and which one gets forgotten

- **`width: 100%` is the one that gets left off**, and it is the one that matters most. Width is
  the *main* axis of a row flex container, so a flex item sizes to its **content** there —
  `flex-basis: auto`, `flex-grow: 0`. A workspace without it is as wide as whatever is inside it,
  so it hugs the left edge, and anything centred in it is centred in that narrow strip rather than
  in the tab. Nothing errors; it just looks wrong in a way that reads as a spacing bug somewhere
  else.
- **`height: 100%` looks redundant and is not.** Height is the *cross* axis, where the default
  `align-items: stretch` already fills the panel — so leaving it off usually looks fine, right up
  until something in the tree sets `align-items` and the whole pane collapses to its content. It is
  cheap insurance, and it is what makes the block copyable without thinking.
- **`min-width: 0` / `min-height: 0`** turn off the automatic minimum size a flex item gets. Without
  them a long unbreakable child — a wide table, a long identifier — pushes the workspace wider than
  the tab and the *page* scrolls sideways, instead of the child's own `overflow: auto` container
  scrolling.
- **`box-sizing: border-box`** because this project has **no global `box-sizing` reset**. A root
  with `width: 100%` and any `padding` overflows its panel by exactly that padding without it.

## Checking it

Resize the window narrow and wide. A root missing `width` stops filling the tab; a root missing the
`min-*` pair makes the whole page scroll sideways rather than the inner list.
