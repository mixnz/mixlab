---
name: using-shared-components
description: Use when writing or changing any UI under apps/desktop/src — a form field, a dropdown, a checkbox, a textarea, a dialog, a list row, a tab header, a clickable icon — or when a screen needs a control that src/components/ may not have yet.
---

# Using shared components

MixLab's library is `apps/desktop/src/components/`, one folder each. **A raw HTML control in a
module is a decision, and a decision needs a reason written down.** The raw elements already in the
tree are debt, not precedent.

The primitives are not styling wrappers — each carries behaviour a raw element silently lacks:

| Primitive | What raw loses |
| --- | --- |
| `Input` | `autoComplete`/`autoCorrect`/`spellCheck` off, `allowClear` |
| `Select` | portal dropdown, search box, `keepOpen`, ellipsised trigger |
| `Checkbox` | `indeterminate` — a DOM property, re-set every render |
| `Textarea` | grows to fit content up to `maxRows` |
| `Modal` | overlay, portal, Escape — **and the focus trap** |

That last row is the argument: thirteen dialogs once hand-rolled the first three and none had the
fourth. A duplicated primitive is a copy missing something, and nobody notices which.

The redesign (phase 20, [design](../../../docs/specs/2026-09-17-t157-mixlab-redesign-design.md))
added the parts a screen is built from:

| Primitive | For |
| --- | --- |
| `Button` variants | `primary`, `default` (secondary), `soft`, `ghost`, `danger`, `positive`, `link`; `busy="Starting"` locks it with dots |
| `Switch` | an on/off setting that applies at once — `aria-pressed`, `small` in a table row |
| `SegmentedControl` | mutually exclusive choices; `mode="tabs"` switches a view, `mode="filter"` narrows a list |
| `FilterChip` | independent filters, any number on |
| `StatusPill` | a state in a word and a dot — `success`/`warning`/`danger`/`neutral`, `pulse` while changing |
| `Card` | a section of a screen: title, description, count, actions; `flush` for an edge-to-edge table |
| `PageHeader` | a screen's title, badges, description and actions |
| `Table` | a semantic table of managed things at `--row-h`; `data-align="end"` for action cells |
| `MonogramBadge` | two letters and a hue *derived* from a name — never a table of names |
| `IconTile` | an icon in a tinted square, on `MonogramBadge`'s sizes — a folder, a lock, a globe |
| `Popover` | an anchored panel that is neither a menu nor a listbox |
| `EmptyState` | nothing to show, said as a sentence with a way forward |
| `RadioCard` | one of a few exclusive choices as a card; a real radio, so arrow keys walk the group |

**A `Table` row's leading badge or tile is 34px**, `MonogramBadge` and `IconTile` alike: rows
of one height, in screen after screen, drifted to 28, 30 and 34 when each was sized by hand.

Sizes come from the density tokens (`--control-h*`, `--row-h`), so none of these takes a density
prop: a region that holds rows sets `data-density="compact"` on its root.

## The rules

- **Form controls → always the primitive.** `<Input type="number">`, never `<input type="number">`.
  A prop it does not forward is a reason to add the prop, not to drop to raw.
- **Every clickable → a shared component.** `Button` (action), `ItemList` (row), `TabStrip` (tab),
  `ContextMenu` (menu entry), `ActionBar` (icon actions). "My case behaves differently" is not an
  exemption: what is shared is the drawing, not the behaviour.
- Run `ls apps/desktop/src/components/` before concluding something is missing. There is no barrel —
  `import Button from "../../../../components/Button";`

## When nothing fits

1. **Count call sites, including the one you are writing.** Two or more → make it shared now.
   Standing example: `type="radio"` sat open-coded in two screens until `RadioCard` replaced both.
2. **One call site, generic shape → still make it shared.** The bias is toward creating: a control a
   second screen would plausibly want is cheaper as a primitive today than as two divergent copies
   later. Ask what the *thing* is — "a segmented control", not "the idle-timeout picker".
3. **One call site, genuinely singular** (a colour picker in one panel) → raw, with the comment below.

A new primitive is a folder: `Thing.tsx`, `Thing.module.css`, `index.ts`. Pure logic beside it gets
a `.test.ts` (`Select/scroll.ts`, `TabStrip/reorder.ts`).

## The escape hatch

Going raw requires the comment saying why, as `BodyEditor.tsx` does:

```tsx
// A plain `<textarea>` rather than the shared one, which grows to fit its text: this pane has a
// height of its own and the box should fill it, not push the layout about.
```

Name the component, say what it would do, say why that is wrong here. A raw element with no reason
written down is the defect. This binds callers, not the library — inside `src/components/`, raw
elements are the implementation.

## Red flags

- A raw `<input>`, `<select>`, `<textarea>` or `<button>` in `src/modules/` or `src/shell/` with no comment.
- A local `Field()` or `styles.btn` re-implementing a primitive inside one module.
- A second screen open-coding what the first one open-coded.

| Excuse | Reality |
| --- | --- |
| "The tree is full of raw `<button>`" | It is debt. The count argues for the rule, not against it. |
| "It's just styling, mine looks the same" | Styling is what you can see. The focus trap and `indeterminate` are what you cannot. |
| "Only one screen needs it" | Then ask what the *thing* is. A generic shape still becomes a primitive. |
| "I'll extract it later" | Later is the second divergent copy. Extract at the second call site. |
| "The primitive lacks the prop I need" | Add the prop. |
| "Adding a component is out of scope here" | Then say so in the comment. Silence turns a decision into a defect. |
