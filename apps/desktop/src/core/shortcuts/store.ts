import type { ShortcutGroup } from "./types";

/** One handler, listening. Held as an object so a registration can be cancelled by identity —
 *  two panes may be listening for the same id at once, and removing "the one with this id" would
 *  take the wrong one down. */
export interface Registration {
  id: string;
  handler: () => void;
}

/**
 * Who is listening, and what is standing over them.
 *
 * A module-level singleton rather than a React context, the same shape `core/reload.ts` already
 * has: nothing here belongs in the render tree, and a provider wrapped round `App` would be
 * ceremony for a list two entries long.
 */
const registrations: Registration[] = [];
/** One entry per dialog or menu up. `visible` is how one that belongs to a tab says whether that
 *  tab is the one on screen; absent, it is up for the whole window. */
const modals = new Set<{ visible?: () => boolean }>();
let catalogue: ShortcutGroup[] = [];

/** Starts listening; the returned function stops. Order matters — see {@link enabledIds}. */
export function register(registration: Registration): () => void {
  registrations.push(registration);
  return () => {
    const at = registrations.indexOf(registration);
    if (at >= 0) registrations.splice(at, 1);
  };
}

/** The ids listening, oldest first — the order `decide` breaks ties on. */
export function enabledIds(): string[] {
  return registrations.map((registration) => registration.id);
}

/** Runs the newest handler registered under `id`, which is the one `decide` picked. */
export function run(id: string): void {
  for (let i = registrations.length - 1; i >= 0; i -= 1) {
    if (registrations[i].id === id) {
      registrations[i].handler();
      return;
    }
  }
}

/** Marks a dialog or menu as up; the returned function marks it down again. Idempotent, so a
 *  disposer called twice — which is what StrictMode does to an effect — cannot unbalance the
 *  count.
 *
 *  `visible` is for a dialog that belongs to a tab: it holds the keyboard only while that tab is
 *  on screen, so one left open in a tab out of sight does not silence the tab in front. */
export function enterModal(visible?: () => boolean): () => void {
  const entry = { visible };
  modals.add(entry);
  return () => {
    modals.delete(entry);
  };
}

export function modalDepth(): number {
  let depth = 0;
  for (const entry of modals) if (entry.visible === undefined || entry.visible()) depth += 1;
  return depth;
}

/**
 * Remembers the catalogue the dispatcher was handed, and hands it back.
 *
 * Here rather than passed around, because one pane has to answer "would the app take this key?"
 * before the dispatcher ever sees the event — see `isClaimed`. It asks this same list, so the
 * catalogue stays the one place a chord is written down.
 */
export function setCatalogue(groups: ShortcutGroup[]): void {
  catalogue = groups;
}

export function currentCatalogue(): ShortcutGroup[] {
  return catalogue;
}
