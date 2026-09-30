---
status: implemented
date: 2026-09-17
task: T157
---

# T157 — MixLab, redesigned: two themes, two densities, one set of parts

Roadmap tasks T157–T163, phase 20. 2026-09-17.

**The case this comes from**: a design canvas for MixLab — thirteen artboards covering the
MixEngine module (Dashboard, Projects, Sites and its edit dialog, Domains & TLS, Runtimes, PHP
extensions, Services, Blueprints), the `db` module's connection editor, and two reference boards
(*Interaction states*, *Service action states*) — drawn dark-only, in Geist, with content set in
bordered cards. Two light drafts of *Dashboard* and *Interaction states* were derived from it on
2026-09-17 and accepted. The canvas is a private claude.ai artifact, so **every value this work
depends on is written down here**; the canvas is the picture, this document is the contract.

The canvas was drawn from the product's screens but not against its API. Where it shows something
MixEngine cannot do, or leaves out something MixLab already does, this document decides — see
[What the canvas shows that is not built](#what-the-canvas-shows-that-is-not-built) and
[What stays although the canvas leaves it out](#what-stays-although-the-canvas-leaves-it-out).

## What is already true

- **The stylesheets already speak in tokens.** Across `apps/desktop/src`, CSS holds roughly 1,300
  `var(--…)` references against roughly 400 colour literals, 177 of which are `shell/App.css`
  defining the tokens themselves. Changing what the tokens resolve to re-colours most of the
  window before a single screen is touched.
- **Themes are an attribute.** `shell/theme.ts` writes `data-theme` (`light`, `dark`, or absent
  for *system*), `data-accent` (absent for the default) and `data-glass`; `public/theme-preload.js`
  applies the stored theme before the bundle loads so a dark window does not flash white.
- **The accent is three handles.** `--accent`, `--accent-text`, `--accent-rgb`, per accent and per
  theme, resolved from `--c-<hue>`, `--c-<hue>-text`, `--c-<hue>-rgb` in `App.css`. Ten hues, blue
  by default.
- **Fonts are bundled, not fetched.** `main.tsx` imports `@fontsource/fira-code`; the app runs
  offline under a CSP. `shell/fonts.test.ts` asserts `--font-ui` and `--font-mono` are defined on
  `:root` and that no other stylesheet names a font.
- **Liquid glass is an opt-in look** carried by `shell/glass.css` (241 lines),
  `shell/components/GlassFilter`, an Appearance switch, `glass.test.ts`, and `glass` hooks in
  `Button`, `Checkbox`, `Input`, `Select`, `TabStrip`, `LoadingOverlay`, `ContextMenu`, `Tooltip`,
  `dialogMotion` and four `db` stylesheets.
- **The MixEngine sidebar already has the canvas's shape**: *Overview / Websites / Environment /
  Library*, the same twelve screens in the same order, Settings pinned to the bottom
  (`components/Sidebar/Sidebar.tsx`, decision D11 of T119).
- **Services is already a list beside a detail pane**, and the detail already carries Database,
  Autostart, Idle and Limits panels.
- **`RuntimeExtension.linkage`** tells a compiled-in extension from an optional one;
  `ExtensionsPanel` already disables the switch for `linkage === "static"`.
- **`DiskUsage`** answers five categories, each with `bytes`, `reclaim` and an optional
  `unreadable` note, plus `other_bytes`.
- **`ServiceRole`** distinguishes only `front_end` from `other`, and
  [ADR 0026](../decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md)
  forbids a client from mapping a package name to a role.
- **`BlueprintSummary`** carries `slug`, `name`, `description`, `created_at`, `source`, `trusted`,
  `signature`, `file` — no category, no stack, no post-apply hint.
- **There is no `service.update`.** A service's package, instance and port are fixed at creation.
- **The terminal has no theme of its own.** `TerminalView` constructs xterm with its default
  colours; only the search-match decorations are set.
- **The shared components** are `ActionBar`, `Button`, `CellDialog`, `Checkbox`, `ConfirmDialog`,
  `ErrorBanner`, `ErrorBoundary`, `Input`/`Textarea`, `ItemList`, `JsonView`, `LoadingOverlay`,
  `Modal`, `NameDialog`, `Pagination`, `Select`, `Splitter`, `TabStrip`, `Tooltip`, plus
  `ContextMenu` and `dialogMotion`. There is no switch, segmented control, chip, status pill, card
  or popover.

## Decisions

### D1 — Two themes, the dark one as drawn

Both themes stay, and *system* stays the default. The dark theme takes the canvas's values; the
light theme takes the accepted light drafts' values. Neither is derived from the other at runtime:
every colour token is written out twice in `App.css`, once per theme. `theme-preload.js` and the
theme picker stay.

**`data-theme` always carries the resolved theme.** Today *system* is the attribute's absence, so
every dark rule is written twice — once under `:root[data-theme="dark"]` and again under
`@media (prefers-color-scheme: dark) { :root:not([data-theme]) }` — in `App.css` and in thirteen
other stylesheets. Doubling the token set would double that. Instead `theme-preload.js` and
`theme.ts` resolve *system* through `matchMedia` and write `light` or `dark`, `theme.ts` follows
the media query while *system* is chosen, and the stored preference stays `<old>-theme` as it is.
Every `:root:not([data-theme])` block is then dead and is deleted.

### D2 — The accent picker stays, and mint is the new default

A hue **mint** joins the ten, and becomes the default a user who never chose one sees. The other
ten are re-tuned for the new surfaces in both themes.

Each hue now has **four** handles per theme instead of three:

| Handle | Meaning | Mint, dark | Mint, light |
| --- | --- | --- | --- |
| `--c-<hue>` | the fill: switch track, progress bar, selected check | `#5EC4A1` | `#5EC4A1` |
| `--c-<hue>-text` | the accent as text, icon or focus ring on the page | `#5EC4A1` | `#387661` |
| `--c-<hue>-on-solid` | text on a filled primary button | `#062117` | `#062117` |
| `--c-<hue>-rgb` | bare channels, for washes | `94 196 161` | `94 196 161` |

`--accent-solid` becomes `var(--accent)` in both themes (the canvas fills the primary button with
the bright hue and puts dark ink on it), and `--accent-on-solid` resolves from the new handle. A
hue whose fill cannot carry dark ink at 4.5:1 gets light ink instead; that is a per-hue value, not
a rule in code.

Washes use the established `rgb(var(--accent-rgb) / a)` form — **never `color-mix()`**, which the
WebKitGTK builds MixLab ships on do not all support. Soft fill `0.12`, soft hover `0.22`, soft
pressed `0.30`, soft border `0.45`, focus halo `0.20` dark / `0.30` light.

### D3 — Liquid glass is removed

`glass.css`, `GlassFilter`, `glass.test.ts`, the Appearance switch and its four i18n keys, the
`data-glass` attribute and every `glass` hook in component and module stylesheets are deleted.
`--rail-bg` and `--rail-filter` collapse to plain colours. On first load after the change
`theme.ts` removes the `<old>-glass` key from `localStorage` so nothing stale is left behind.

### D4 — Geist replaces Fira Code and the system sans

`@fontsource-variable/geist` and `@fontsource-variable/geist-mono` are bundled; the four
`@fontsource/fira-code` imports and the dependency go. `--font-ui` becomes
`"Geist Variable", "Segoe UI", system-ui, sans-serif`; `--font-mono` becomes
`"Geist Mono Variable", ui-monospace, Consolas, monospace`. The *role* boundary `App.css` documents
is unchanged: mono where character-by-character comparison is real — values from a database,
paths, ports, versions, domains, commands, connection strings. The terminal's font setting keeps
its list; its default becomes Geist Mono.

### D5 — Two densities, chosen by the region and not by the component

The canvas is roomy: 44px page actions, 42px fields, 64px table rows. That suits screens a person
manages things on and wastes a result grid, a schema tree, an editor and a terminal, where rows
are the point. So there are two densities, and **components never choose one**: they read size
tokens, and a region sets `data-density="compact"` on its root to redefine those tokens beneath
it.

| Token | Comfortable (default) | Compact |
| --- | --- | --- |
| `--control-h-lg` (page-level actions) | 44px | 30px |
| `--control-h` (fields, dialog buttons) | 40px | 28px |
| `--control-h-sm` (row actions) | 36px | 24px |
| `--control-h-xs` (chips, inline toggles) | 32px | 22px |
| `--row-h` (table row) | 60px | 28px |
| `--list-row-h` (sidebar or list row with a badge) | 48px | 28px |
| `--text-body` | 13.5px | 13px |

Compact regions: the `db` workspace once connected (tree, grid, editor, structure, result panes),
REST's request list, key-value tables and response pane, the terminal view, and the log view in
MixEngine's Logs screen. Everything else — the tab bar, sidebars, MixEngine screens, every form
and dialog, the `db` connection editor, Tools — is comfortable.

### D6 — Focus stays drawn inside the border

The canvas draws focus as an outline 2px outside the control. `App.css` records why MixLab moved
away from exactly that: scroll panes nest inside `overflow: hidden` workspaces and an outset ring
loses edges. The reason still holds, so `--focus-ring` stays inset, now
`inset 0 0 0 2px var(--accent-text)` — solid, because a half-transparent mint fails 3:1 on a white
card. A text field shows focus as its border turning `--accent-text` plus
`inset 0 0 0 1px var(--accent-text)`, so it reads as the canvas's 2px edge without the outer halo.

### D7 — New scales replace the four-step type scale and the three-step radii

`App.css` currently says a font size outside 12/13/14/16px, or a radius outside 6/8/12px, is a
mistake. The canvas needs more steps than that, so the scales are replaced, and the statement
moves with them: a size or radius outside the new sets is still a mistake.

**Type** (`--text-*`): `2xs` 11.5px (badge sub-lines), `xs` 12px (column heads, captions, group
labels), `sm` 12.5px (field labels, secondary lines), `md` 13.5px (body, controls), `lg` 15px
(card titles), `xl` 18px (dialog titles), `2xl` 28px (page titles). Weights 400, 500, 600, 700.
Page titles take `letter-spacing: -0.025em`.

**Radius** (`--radius-*`): `xs` 6px (tags, code, badges), `sm` 8px (row-sized buttons, options,
list rows), `md` 10px (fields and page buttons), `lg` 12px (popovers, inset panels, segmented
tracks), `xl` 16px (cards), `2xl` 18px (dialogs), `pill` 999px (chips, status pills, switches).

**Spacing**: page padding `28px 30px 36px 36px`; gap between page sections 22px; card padding
22px; card header to body 16px; field label to field 7px; between fields 14px.

**Motion**: hover and colour transitions 120–140ms `ease`; popover in 150ms and dialog in 200ms on
`cubic-bezier(.2,.8,.2,1)`; switch knob 180ms; pressed buttons move 1px down. All of it is off
under `prefers-reduced-motion: reduce`, as today.

### D8 — Colour belongs to `App.css`

After this work no stylesheet other than `App.css` contains a colour literal (hex, `rgb()`,
`rgba()`, `hsl()`), and no TypeScript file does except where a library demands concrete colours
(xterm) — and there they are read from the computed tokens, not written. A test enforces it on the
same `?raw` glob `fonts.test.ts` already uses.

### D9 — A package's badge is computed from its name, not looked up

The canvas gives each service, runtime and blueprint a two-letter badge in a tinted square (`Ph`,
`Ma`, `Re`). Mapping package names to hues or labels in the client is what ADR 0026 warns against,
so the badge is **derived**: the letters are the name's first letter upper-cased and its next
letter lower-cased; the hue is one of eight categorical tokens chosen by a stable hash of the full
name. No table of names exists anywhere in the client. An empty name gets `··` on the neutral
tint.

## Tokens

Every value, both themes. Names are the ones `App.css` will carry; where a name exists today it is
kept and only its value changes.

### Surfaces

| Token | Dark | Light | Used for |
| --- | --- | --- | --- |
| `--page-bg` | `#0E131C` | `#F5F7FA` | the window behind content |
| `--chrome-bg` | `#0B1017` | `#EDF0F4` | the tab bar |
| `--sidebar-bg` | `#111722` | `#FAFBFC` | module sidebars |
| `--surface-bg` | `#131A25` | `#FFFFFF` | cards, dialogs |
| `--surface-sunken` | `#0F151F` | `#F3F6F9` | a note or group inset inside a card |
| `--table-head-bg` | `#111823` | `#F7F9FB` | table header rows, dialog footers |
| `--control-bg` | `#0B1017` | `#FFFFFF` | fields, select triggers |
| `--button-bg` | `#151C28` | `#FFFFFF` | secondary buttons, the active tab |
| `--popover-bg` | `#161E2B` | `#FFFFFF` | menus, listboxes, popovers |
| `--track-bg` | `#0B1017` | `#EEF1F5` | segmented-control tracks, progress tracks |
| `--neutral-bg` | `#1A2230` | `#EEF1F5` | neutral tags, switch-off track, neutral pills |
| `--hover-bg` | `#161E2B` | `#F3F6F9` | hovered rows, options, nav items |
| `--active-bg` | `#1C2534` | `#E3E8EF` | pressed rows and nav items |
| `--choice-active-bg` | `#1C2533` | `#FFFFFF` | the selected segment |
| `--option-selected-bg` | `#1C2533` | `#EEF1F5` | the selected option in a listbox |
| `--scrim` | `rgb(4 7 11 / 0.68)` | `rgb(15 23 42 / 0.32)` | behind a dialog |

### Lines

| Token | Dark | Light | Used for |
| --- | --- | --- | --- |
| `--border-soft` | `#1B2330` | `#EEF1F5` | row dividers |
| `--border` | `#1F2836` | `#E2E7EE` | card and table borders |
| `--chrome-border` | `#1C2432` | `#DDE3EA` | tab bar and sidebar edges, disabled outlines |
| `--control-border` | `#222B3A` | `#D5DCE5` | fields |
| `--border-strong` | `#2A3446` | `#D5DCE5` | secondary buttons, popovers, active tab |
| `--border-hover` | `#3A4760` | `#B9C3D0` | hovered fields, buttons, chips |
| `--scrollbar-thumb` | `#243041` | `#C5CDD8` | scrollbar at rest |
| `--scrollbar-thumb-hover` | `#34425A` | `#AAB4C2` | scrollbar hovered |

### Text

| Token | Dark | Light | Used for |
| --- | --- | --- | --- |
| `--text` | `#E6EAF0` | `#141A23` | content |
| `--text-secondary` | `#A7B0BF` | `#4F5B6C` | descriptions, idle nav, secondary cells |
| `--text-muted` | `#8391A5` | `#626E80` | captions, column heads, group labels |
| `--text-faint` | `#6B7688` | `#7A8596` | placeholders and the `—` of an empty cell only |
| `--text-disabled` | `#5A6679` | `#A3ADBA` | disabled labels |

### Status

Each tone has a text colour, a solid (dot, bar, border) and bare channels for its washes (pill fill
`0.12`, border `0.40`, hover fill `0.22`).

| Tone | Text dark / light | Solid dark / light | Channels dark / light |
| --- | --- | --- | --- |
| `success` | `#7DD8AA` / `#1C7A4E` | `#5CCB93` / `#2FA36B` | `92 203 147` / `47 163 107` |
| `warning` | `#E9B54A` / `#946300` | `#E9B54A` / `#946300` | `233 181 74` / `217 154 30` |
| `danger` | `#F08A80` / `#C0392E` | `#E5675C` / `#D64A3F` | `229 103 92` / `214 74 63` |

`--danger`, `--danger-text` and `--danger-rgb` keep their names and take the `danger` row.

### Categorical

Eight hues for badges and chart series, text on a `0.16` (dark) or `0.12` (light) wash of itself:
`teal #6FD0C4 / #176E65`, `sand #C9A27E / #7F5B39`, `sky #5DB0D6 / #23709A`,
`periwinkle #A9B0F5 / #4F59C2`, `coral #F2806A / #B2412A`, `blue #8EA8F2 / #4460BD`,
`purple #B39AF2 / #7152CC`, `green #5CCB93 / #1B7048`. The light casts are darker than the
2026-09-17 light draft's, which measured 2.8–4.4:1 on their own wash.

### Elevation

| Token | Dark | Light |
| --- | --- | --- |
| `--shadow-card` | `none` | `0 1px 2px rgb(15 23 42 / 0.05)` |
| `--shadow-popover` | `0 20px 48px -14px rgb(0 0 0 / 0.75)` | `0 20px 48px -14px rgb(15 23 42 / 0.16)` |
| `--shadow-dialog` | `0 40px 100px -20px rgb(0 0 0 / 0.85)` | `0 40px 100px -20px rgb(15 23 42 / 0.22)` |
| `--shadow-primary` | `0 6px 20px -8px rgb(var(--accent-rgb) / 0.45)` | same |

### Code and terminal colours

The SQL editor already draws from `--sql-*` tokens (`SqlEditor/theme.ts` names only CSS
variables), so a theme change reaches it on its own; those tokens are re-valued for both themes and
its `HighlightStyle` moves onto them too. The terminal's sixteen ANSI colours, background,
foreground, selection and cursor get `--ansi-*` tokens per theme. Every foreground in both sets
clears 4.5:1 against the background it is drawn on, except the ANSI colours that exist to be the
background's own shade: `black` in the dark theme, `white` and `brightWhite` in the light one. `TerminalView` reads the computed `--ansi-*`
values when it creates a terminal and re-applies them when `data-theme` changes or, under
*system*, when `prefers-color-scheme` does.

## Components

All in `src/components/`, one folder each, CSS Modules, per
[component-structure](../standards/desktop/component-structure.md). Sizes come from
the density tokens in D5, so none of them takes a `size` prop for density.

### Restyled

- **Button** — variants `primary` (accent fill, `--accent-on-solid` ink, `--shadow-primary`),
  `secondary` (`--button-bg`, `--border-strong`), `soft` (accent wash, `--accent-text` ink, accent
  border at 0.45), `ghost` (transparent, `--text-muted`, `--hover-bg` on hover), `danger`
  (transparent, `--danger-text`, danger wash on hover), and `positive` (success wash, success text —
  the *Start* button). Heights `lg`, default, `sm`, `xs` map to the control-height tokens. A `busy`
  label replaces the content with three pulsing dots and the label in `warning` tone, disables the
  button and keeps its width. Pressed moves 1px down; primary hover is `filter: brightness(1.1)`
  in dark and `brightness(0.95)` in light.
- **Input / Textarea** — `--control-bg`, `--control-border`, `--radius-md`; hover
  `--border-hover`; focus per D6; `aria-invalid` turns the border `danger` with a danger message
  line below. A `mono` prop switches to `--font-mono`. Optional leading icon (search) and trailing
  slot (reveal toggle, Browse button).
- **Select** — trigger as Input with a chevron that turns 180° when open; listbox on
  `--popover-bg`, `--border-strong`, `--radius-lg`, `--shadow-popover`, 6px padding. An option may
  carry a description line in `--text-muted`; the selected one shows `--option-selected-bg` and an
  accent check. A footer slot takes a link-styled action (*Install another PHP version*).
- **Checkbox** — 18px box, `--radius-xs`, accent fill with `--accent-on-solid` check when checked;
  the whole row highlights on hover.
- **Modal / ConfirmDialog / NameDialog / CellDialog** — `--surface-bg`, `--border-strong`,
  `--radius-2xl`, `--shadow-dialog`, over `--scrim`. Header: `xl` title, optional `sm` subtitle,
  ghost close button. Footer on `--table-head-bg` with the actions right-aligned.
- **TabStrip** — the shell's tabs: 36px pills with `--radius-sm`. Idle: transparent, a 7px dot in
  the tab badge's tone (neutral when there is none), `--text-secondary`. Active: `--button-bg`,
  `--border-strong`, the module's icon in `--accent-text`, `--text` at weight 500. A 26px ghost
  close button on each. `TabBadge` states map onto the dot's tone.
- **ContextMenu** — a menu on the Select listbox's surface; items 36px with a leading icon in
  `--text-secondary`, separators as a 1px `--border-strong` line, a `danger` item in
  `--danger-text`, disabled items at `--text-disabled`.
- **Tooltip, ErrorBanner, LoadingOverlay, ItemList, Pagination, Splitter, ActionBar, JsonView** —
  take the tokens; ErrorBanner becomes a `danger`-tone inset panel with its dismiss button;
  ItemList rows take `--list-row-h`, `--radius-sm`, and the accent wash with an accent border
  (0.45) when current; JsonView's colours move to the `--sql-*` syntax tokens.

### New

- **Switch** — 46×28 track (38×22 in a table row, via a `small` prop that is about placement, not
  density), 22px knob; off: `--neutral-bg`, `--border-strong`, knob `--text-muted`; on: accent
  fill and border, knob `--accent-on-solid`. A `<button aria-pressed>` labelled by its row.
- **SegmentedControl** — a track (`--track-bg`, `--border`, `--radius-lg`, 3–4px padding) of
  segments; the selected one on `--choice-active-bg` with `--border-strong` (and `--shadow-card`
  in light), the others `--text-muted`, turning `--text` on hover. Each segment may carry an icon
  and a count in `--text-faint`. Renders as `role="tablist"` when it switches a view and as a
  group of `aria-pressed` buttons when it filters.
- **FilterChip** — a 30–32px pill: `--table-head-bg`, `--border-strong`, `--text-secondary`;
  pressed: accent wash and accent border, `--text`. Optional count.
- **StatusPill** — pill with a 7px dot and label in tone `success`, `warning`, `danger` or
  `neutral`; `pulse` animates the dot for in-flight states.
- **Card** — `--surface-bg`, `--border`, `--radius-xl`, `--shadow-card`. Optional header with an
  `lg` title, a `sm` description in `--text-muted`, a count after the title, and an actions slot.
  A `flush` variant has no body padding, for a table that runs edge to edge.
- **PageHeader** — `2xl` title, optional badges after it, a description line in
  `--text-secondary`, an actions slot aligned to the bottom right.
- **Table** — a styled semantic `<table>`: header row on `--table-head-bg` in `xs` weight 500
  `--text-muted`; rows `--row-h` tall, `--border-soft` dividers, `--hover-bg` on hover; an
  actions column right-aligned. Used by every MixEngine table. The `db` result grid keeps its own
  virtualised grid and takes tokens only.
- **MonogramBadge** — D9's badge at 28, 30, 34, 38 or 50px, `--radius-sm` scaled with it.
- **Popover** — the anchored surface under Select and ContextMenu, exported for panels that are
  neither (the database engine picker).
- **EmptyState** — a centred title, a `--text-muted` line and an optional action, drawn inside a
  card or on a dashed `--border-strong` outline.

The [using-shared-components](../../.claude/skills/using-shared-components/) skill's catalogue is
updated with every new component in the task that adds it.

## Shell

- **Tab bar** — 56px on `--chrome-bg` with a `--chrome-border` bottom edge. At the left, MixLab's
  existing logo mark (`public/logo.svg`) at 28px and the wordmark *MixLab* (16px, weight 700),
  closed off by a vertical `--chrome-border` rule. Then the TabStrip, then a 34px dashed-outline
  ghost button for **+**, whose module menu is a ContextMenu. At the right, a 38px secondary icon
  button for Settings. Dragging, closing, badges and the module menu behave as today.
- **Settings modal** — the shell's own dialog on Modal, its sections separated as cards.
  *Appearance* offers theme as a SegmentedControl (*System / Light / Dark*) and accent as swatches,
  mint first; the glass section is gone.
- **FirstRun and TabNotice** — take Card, Button and the page tokens; content unchanged.

## MixEngine module

Every screen gets PageHeader, content in Cards, tables on Table, and the comfortable density. The
module's gate (not running, not answering, not installed, and the storage picker) is a centred Card
with the same content and buttons it has today.

**Sidebar** — 248px on `--sidebar-bg` with a `--chrome-border` right edge; group labels in `xs`
weight 500 `--text-muted`, no longer upper-cased; items 40px, `--radius-sm`, a 16px icon then the
label in `md` weight 500 `--text-secondary`; the current item takes the accent wash, accent border
at 0.45, weight 600, `--text`, and its icon in `--accent-text`. Settings sits below a
`--chrome-border` rule at the bottom. New icons join `src/icons/`: dashboard, metrics, logs,
runtimes, puzzle (PHP extensions), server (services), layout (blueprints), add-ons; projects,
sites and domains use the existing folder, globe and lock.

**Dashboard**
- Header: *Dashboard*, a neutral badge *MixEngine {version}* from `daemon.status`, and a summary
  StatusPill — *N changing* (warning) when any service is mid-transition, else *N of M running*
  (success) when any runs, else *All services stopped* (neutral). Below it, the home path from
  `daemon.status` in mono with a ghost Copy button that reads *Copied* for 1.6s.
- To the right, above the actions: a daemon usage strip — *Daemon* with a live dot and its state,
  *CPU* with the percentage and a 52px bar, *Memory* with the RSS — drawn from the same reading the
  current `daemonUsage` line uses. Actions: Reload (secondary), Stop all (danger outline, disabled
  when nothing runs), New service (primary).
- QuickStart, when `shouldOfferQuickStart` says so, is a Card above Services. The jobs list and the
  *N waiting for an administrator* line render as an inset panel inside the Services card header
  when present.
- **Services** card: SegmentedControl *All / Running / Stopped* with counts. Columns *Service*
  (MonogramBadge, the id with `@instance` in `--text-muted`, and a *Front end* sub-line when
  `role` is `front_end`), *State* (StatusPill; transitions pulse), *Autostart* (small Switch),
  *Port* (mono, `—` in `--text-faint` when none), *CPU*, *Memory*, *Actions*. Actions: a 110px
  fixed-width Start (positive) / Stop (secondary with a danger square; turns danger on hover) /
  busy button per the *Service action states* board, then Restart (secondary, disabled unless
  running), then a ⋯ ghost button opening a ContextMenu with today's items — autostart on/off,
  *Explore data*, *Open in {client}*, *Get the password*, *Reset the password…* — plus
  **View logs** (opens Logs with the service chosen) and **Copy port** (disabled with no port).
  An empty filter shows EmptyState with *Show all services*.
- **Disk usage** card: title with *measured at {time}*, the total and `other_bytes`; Refresh
  (secondary, spins while refreshing) and Clean up (soft) opening today's CleanupDialog. A 10px
  stacked bar, one categorical hue per category, then one row per category: swatch, name, size,
  a static one-line description per category, and a tag drawn from `reclaim` (reclaimable size in
  accent wash, *Protected* with a lock in warning tone when the category cannot be reclaimed,
  *Nothing to reclaim* in `--text-faint`). An `unreadable` note shows under its row in warning tone.

**Projects** — header with *New project* (primary). Card with Table: *Name* (a folder
MonogramBadge-style tile and the name as a link to Sites filtered to it), *Root* (mono, truncated,
with a ghost copy button), *mixengine.toml*, *Keep warm*, *Actions* (Sites, Edit, Delete). The
effective runtime pins detail keeps its current place and content, restyled. ProjectForm on Modal.

**Sites** — header with a project filter Select (*All projects*, then each project) and *New
site*. Card with Table: *Domain* (a lock tile in success wash when HTTPS is on, neutral when off,
then the first domain), *Owner* (folder icon and project name, or *Add-on: {id}*), *Kind* (mono
tag), *Routes* (count, *None* in `--text-faint`), *HTTPS* (*On* in success text, with *redirect*
when set; *Off* faint), *State* (StatusPill *Enabled* success / *Disabled* neutral), *Sharing*
(a soft *Share* button, or *On the LAN* / *until {time}* with the button to manage it — both open
ShareDialog), *Actions* (Open with its current hint, Edit — disabled with the existing tooltip for
an add-on's site).

**Site dialog** (create and edit) on Modal, 660px, fields in this order: *Project* (create only),
*Domains* (mono Textarea, the current input format and hint), *acceptRiskyTld* checkbox when the
domains need it, *Doc root* (mono Input with Browse, full path below), a two-column row of *Kind*
(Select with a description per kind) and the kind's own field — *PHP pool* (Select, including
*Resolved automatically*) or *Upstream URL* and *Port* — then *Routes* (hint; each row path Input,
target-kind Select, target Input or Select, danger icon button; a dashed *Add a route*), then
*Services* as a two-column Checkbox grid in a sunken box with an *N of M selected* count, then an
inset panel of Switch rows *HTTPS*, *Redirect HTTP to HTTPS* (disabled unless HTTPS) and
*Enabled*. Footer Cancel / Save.

**Domains & TLS** — header with *Add domain*. A *Certificate authority* card (shield tile, title,
description) holding a sunken two-row grid: *System store* with its trust StatusPill and
explanation, *Browsers* with its StatusPill, explanation and *Fix browser trust*. A *Domains* card
with Table *Domain, Site, Hosts entry, Wildcard, Server answers, Resolves to* (mono, with a success
check when it matches)*, Reason, Actions (Remove)*. A *Certificates* card with Table *Domain, Names
covered* (mono tags)*, Days left, Status* (StatusPill from the outcome)*, Actions* (Reissue with a
busy state). Removal keeps today's behaviour. AddDomainDialog on Modal.

**Runtimes** — header. SegmentedControl tabs *Languages, Web servers, Databases, Cache & queues,
Other*, each with an icon. *Installed* card with count and Table: *Runtime* (MonogramBadge, name),
*Version* (mono), *Channel* (neutral tag), *Installed*, *Used by*, *Needs* (only when some row has
one), *Default* (accent pill *Default*, or ghost *Set as default*), *Actions* (Uninstall, danger).
*Available* card with count, the stale badge when stale, a search Input, and a scrolling list of
rows with *Install* (soft, fixed 104px) / *Installing* busy. The extensions panel stays inside
Runtimes for a PHP, drawn with the same tile grid as the PHP extensions screen.

**PHP extensions** — header with the intro. A row of: PHP version Select (options carry *Default
version* where true; footer link *Install another PHP version* to Runtimes), search Input, and a
filtering SegmentedControl *All / On / Off* with counts. An *Optional for PHP {version}* card with
*N of M on* and a five-column grid of 46px tiles — mono name and a small Switch, the whole tile a
button, dimmed when off. A *Built in* card (lock icon, *Compiled into PHP and always on*) of
`linkage === "static"` names as mono chips, hidden when the filter is *Off*. The restart-required
notice with *Restart pool*, the *applies next start* note, and the *no PHP installed* EmptyState
stay.

**Services** — a 260px list pane on `--sidebar-bg`: title with count, a dashed *New service*
button, rows with MonogramBadge, id with `@instance`, and a dot plus the service's **real** state
label; the current row takes the accent wash. The detail pane: a 50px badge, the id as title, its
StatusPill, *Front end* when it is one and `port {n}` when it has one; *Delete service* as a danger
outline button with today's force-delete flow. Then cards: **Database** (when the service has
one) — an inset line naming the credential-store key with a copy button, *Database name* and
*Account name* (mono Inputs, the account placeholder following the database name) and *Create*,
with the created/existed result below; **Start with MixEngine** — title, description, Switch, and a
sunken note with the two explanatory sentences; **Idle shutdown** — three radio cards (*Use the
recipe's own setting*, *Never stop for being idle*, *Stop after this many minutes idle*) with the
minutes Input enabled only for the third, and *Save*; **Limits** — today's panel, restyled on the
same card, field and Switch conventions. The port-moved notice shows as an inset panel above the
cards.

**Blueprints** — header with *Import…* (secondary) and *Capture project…* (primary). A search
Input filtering on name and description. A three-column grid of cards: MonogramBadge from the name,
name, source (*Built in / Captured / Imported*) in `--text-muted`, trust badge (*Trusted* success,
*Untrusted* warning, *Signature does not verify* danger), description, and *Apply* (soft) at the
bottom right. No match shows EmptyState. ApplyDialog, CaptureDialog, ImportDialog and AfterApply on
Modal with their content unchanged.

**Metrics** — header; a Card holding the subject Select, the retention note and the chart; the
chart's series take categorical tokens, its peak band a 0.16 wash of its series, its axes
`--text-muted` on `--border-soft` gridlines.

**Logs** — header with the service Select and a SegmentedControl *All / stdout / stderr*; the log
itself in a flush Card set `data-density="compact"`, mono, *Load more history* and the skipped-lines
gap in `--text-muted`.

**Add-ons** — header with *Install from folder…*; *Installed* and *Registry* cards with Table,
states as StatusPill, actions as row-sized Buttons. PlanDialog on Modal.

**Settings** — header; each section (General, Default web server, Start at login, Updates,
Diagnostics, Diagnostics bundle, Uninstall) its own Card; Uninstall's card border and title in
danger tone. Content, buttons and flows unchanged.

## `db` module

**Connection editor** (a `db` tab with no live connection):
- A 312px pane on `--sidebar-bg`: *Connections* with a count pill and a soft icon button *New
  connection*; a search Input filtering by name; FilterChips for each engine present among saved
  connections, with counts; the list grouped under *Pinned* (only when something is pinned) and
  *Connections*, each group label with its count. A row is 46px: MonogramBadge for the engine, the
  name, a sub-line *{Engine} · {host or file}*, a *Read-only* warning tag when set, a pin icon when
  pinned. Current row on the accent wash. No match shows EmptyState with *Clear filters*.
- A header across the editor: the engine's 52px badge, the connection name as title, a neutral
  StatusPill *Not connected*, and the actions *Update connection* / *Save connection* (secondary,
  labelled and enabled by the same conditions as today), *Save as new* (secondary), *Connect*
  (primary).
- Below, two columns of cards.
  - **General** — *Connection name*; *Database engine* as a 60px trigger showing badge, engine
    name, a one-line description and *Change*, opening a Popover with the seven engines MixLab
    supports (MySQL, PostgreSQL, SQLite, MongoDB, Redis, ClickHouse, SQL Server) as a two-column
    grid of options with badge, name and description.
  - **Server** (or *File* for SQLite) — today's fields for the chosen engine on a six-column grid:
    host, port, user, password (with the reveal toggle), database or Redis index; SQLite's path
    with *Browse* and *New file*; MongoDB's connection string with its reveal confirmation. The
    existing warnings render as warning-tone inset lines. *Use SSL* as a Switch row in a sunken
    panel, for engines that have it.
  - **Connection method** — SegmentedControl *Direct TCP/IP / SSH tunnel*; for SSH, host, port,
    user, *Authentication* Select (*Password* / *Private key*, each with a description), password
    or key file with Browse and passphrase, then the tunnel status text and *Test tunnel* (soft).
    For SQLite, a sunken note that the file is opened directly.
  - **Route** — a diagram of *This computer* → (*encrypted* dashed accent line → *SSH server* with
    its address → *local* line, when tunnelled) or (*direct* line) or (*file access* dotted line)
    → the engine's badge with its address; below it, a sunken *Connection string* line in mono
    with a Copy button. The string never contains a password: `scheme://user@host:port/database`,
    the file path for SQLite, and for MongoDB the entered URI with its password replaced by `***`.

**Workspace** (connected) — its root sets `data-density="compact"`. The schema tree, tabs, grid,
structure view, query editor and results, FilterBar, Redis and Mongo views, TunnelBanner and
TransferOverlay take the tokens and compact components; their layouts are unchanged. Every `db`
dialog (Column, Database, Dump, Index, InsertDocuments, InsertRows, OrderBy, SkipIndex, Table) is a
comfortable Modal.

## REST, Terminal, Tools

No artboard exists for these, so they take the system and keep their layouts.

- **REST** — the request sidebar is a list pane on `--sidebar-bg` with ItemList rows; UrlBar and
  RequestTabs comfortable; KeyValueTable, MultipartTable, the body editor, ResponsePane, HexView,
  TreeView and HtmlPreview compact; method names as mono tags in categorical hues chosen per method
  (`GET` green, `POST` blue, `PUT` sand, `PATCH` purple, `DELETE` coral, others neutral); status
  codes as StatusPill tones (2xx success, 3xx neutral, 4xx warning, 5xx danger); dialogs on Modal.
- **Terminal** — TargetForm is a comfortable Card form; TerminalView compact, themed per D8's
  terminal tokens; SearchBar a compact field row; TerminalSettings a Modal.
- **Tools** — ToolList is a list pane on `--sidebar-bg`; each tool's form is comfortable on Cards;
  outputs that are data (diff, format, JSON, regex matches) are compact mono panels on
  `--surface-sunken`.

## What the canvas shows that is not built

| Canvas element | Why not |
| --- | --- |
| *Edit service* in the dashboard row menu | No `service.update` exists; a service is fixed at creation |
| *Remove service* in the dashboard row menu | Deletion keeps one home, the Services screen, which owns the force-delete flow |
| *Undo* after removing a domain | No undo exists in the API; re-adding from the client is a second mutation that can fail differently |
| Blueprint category chips, stack tags and "then" hints | `BlueprintSummary` carries none of them, and inferring them in the client is business logic |
| Engines marked *Soon*, and the picker's search and category chips | Not real engines; seven engines need no search |
| *No project* in the Sites filter | Every site has an owner; an add-on's site shows under *All projects* |
| *N sites* under a project's name | Projects does not read `site.list`, and a second call for a caption is not worth it |
| The dashboard's Clean-up confirmation popover | CleanupDialog already lets somebody leave a category alone; the popover cannot |
| *Open in dashboard* on the Services screen | The dashboard is one click away in the sidebar |
| The *Database / Cache / PHP runtime* kind line under a service | Only `front_end` is a role (ADR 0026); anything more is a client mapping names |
| Every service shown as *Stopped* in the Services list | A placeholder in the canvas; the real state is shown |
| The database-cylinder logo mark | MixLab keeps its own mark ([icons.md](../standards/desktop/app-icon.md)) |
| Geist loaded from Google Fonts | The window runs offline under a CSP; the fonts are bundled |
| Focus drawn outside the control | D6 |

## What stays although the canvas leaves it out

QuickStart; the jobs list and the elevation-waiting line; ElevationDialog, RequirementDialog and
the *Needs* column; CredentialDialog and reset-password; *Explore data* and *Open in {client}*;
CleanupDialog's per-category choices; the stale badge on Runtimes; *Used by*; the *Other* runtimes
tab; the extensions panel inside Runtimes; the restart-pool notice; the Limits panel; the port-moved
notice; the Sites *Open* action, *Owner* column, add-on sites and the LAN sharing state; the risky
TLD acknowledgement; project runtime pins; Metrics, Logs, Add-ons and Settings in full; every `db`,
REST, Terminal and Tools feature.

## Rules this work does not bend

- **No business logic in the client.** Counts, filters and sorting over a list the daemon returned
  are presentation; anything that decides what a thing *is* comes from the daemon (D9, the table
  above).
- **Every string through `t()`**, in `en.ts` and `vi.ts` together. Strings the redesign removes
  (glass, the *Explore data* wording if it changes, any column head that goes) are deleted from
  both.
- **Contrast**: text 4.5:1 against the surface it sits on, large text and UI edges 3:1, in both
  themes — asserted by test, not by eye (see Testing).
- **Keyboard**: every control reachable and operable as today; Switch and SegmentedControl use
  real buttons with `aria-pressed` or tab semantics; popovers close on Escape and return focus.
- **Reduced motion** honoured everywhere.
- **Nothing outside `src/modules/` imports a module** (`npm run lint`); the new components know no
  module.
- **CHANGELOG**: one `### Changed` line per user-visible area as each task lands, under
  `## [Unreleased]` in the root `CHANGELOG.md`.

## Tasks

Phase 20, *MixLab redesigned*. Each task gets its own implementation plan, its own branch and its
own pull request; the window is allowed to look half-restyled between them, because the tokens
land first and every later task only moves a screen further onto them.

- **T157** — Foundation: the ADR (below); tokens for both themes and both densities; mint and the
  four-handle accents; Geist; glass removed; the colour-literal test and the contrast test.
- **T158** — Components: every restyle and every new component above, with the skill catalogue
  updated.
- **T159** — Shell: tab bar, Settings modal, FirstRun, TabNotice.
- **T160** — MixEngine I: gate, sidebar, Dashboard, Projects, Sites and the site dialog, Domains &
  TLS.
- **T161** — MixEngine II: Runtimes, PHP extensions, Services, Blueprints, Metrics, Logs, Add-ons,
  Settings, and the module's remaining dialogs.
- **T162** — `db`: the connection editor, the compact workspace, the SQL editor's syntax tokens,
  its dialogs.
- **T163** — REST, Terminal (with its theme), Tools.

**M20**: every screen of every module renders in both themes with no colour literal outside
`App.css`, the contrast test green, and the connection editor, Dashboard and Interaction states
matching their artboards side by side.

The ADR is `docs/decisions/desktop/2026-09-17-mixlab-redesign-tokens-themes-densities.md`,
recording D1–D9 and superseding what `App.css`'s comments say about the four-step type scale, the
three radii and the glass materials. [css-modules](../standards/desktop/css-modules.md)
and [component-structure](../standards/desktop/component-structure.md) are updated
where they describe tokens.

## Testing

- **`npm run build`, `npm test`, `npm run lint`** green after every task.
- **Colour literals** (new, T157): a vitest over the `?raw` stylesheet glob counts hex, `rgb`,
  `rgba` and `hsl` literals per file outside `App.css`, and a second pass does the same for hex
  literals in `src/**/*.ts{,x}`. Both compare against a baseline written in the test. A file above
  its baseline, or a file not in it, fails; a file below it fails too, until the baseline is
  lowered to match — so the count can only go down. T157 writes the baseline as it finds it, each
  later task lowers the entries for what it touched, and T163 leaves it empty.
- **Contrast** (new, T157): a vitest parses both theme blocks of `App.css` and asserts a declared
  list of pairs at 4.5:1 — `--text`, `--text-secondary` and `--text-muted` on `--page-bg`,
  `--surface-bg`, `--surface-sunken`, `--table-head-bg`, `--sidebar-bg` and `--hover-bg`; every
  status and categorical text on its own wash over `--surface-bg`; every hue's `-text` on those
  surfaces and its `-on-solid` on its fill; every `--sql-*` and `--ansi-*` foreground on its
  background — and `--accent-text` as a focus ring against the same surfaces at 3:1.
  `--text-faint` (placeholders and empty-cell dashes only) and `--text-disabled` are exempt, as
  WCAG exempts them. `--control-border` is exempt too, and that is a deliberate departure from
  WCAG 1.4.11 the canvas makes and this design accepts: a field is recognised by its label and its
  place in a form, and the edge it shows on focus (`--accent-text`) does clear 3:1.
- **Fonts**: `fonts.test.ts` updated for the Geist stacks; still no stylesheet names a font.
- **Glass**: `glass.test.ts` deleted with the feature; a test asserts `theme.ts` clears
  `<old>-glass`.
- **Badge** (T158): MonogramBadge's letters and hue are a pure function with unit tests — stable
  across calls, eight hues reachable, empty name handled.
- **Connection string** (T162): a pure function with unit tests per engine, asserting no password
  ever appears.
- **By hand, per task**: `npm run dev:app` on Windows, each screen the task touches in *Light* and
  *Dark*, each accent's primary button and focus ring on one screen, keyboard traversal of every new
  component, and reduced motion. The three-OS CI `desktop` job before the pull request, WebKitGTK
  being where a CSS feature is most likely to be missing.

## Out of scope

- A new app icon or logo.
- Layout changes to the `db` workspace, REST, Terminal and Tools beyond what the components and
  densities bring.
- A user-selectable density. The region decides (D5); a setting can come later if somebody asks.
- New daemon or proto fields. Everything here renders what the API already answers.
