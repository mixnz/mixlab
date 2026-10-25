import type { TranslationKey } from "../i18n";

/**
 * What sync sees of a module's data, and nothing more (D5 and D7 of
 * `docs/specs/2026-09-20-t177-a-copy-only-you-can-read-design.md`, whose D-numbers these are).
 *
 * **An item is an id and some data**: the shell never learns that one is a connection. `data` is
 * only what should travel — a reader leaves out a secret, a sidebar's width, a path on this
 * machine — and the hash sync keeps of it is what tells a real change from a resize (D4).
 */
export interface SyncItem {
  /** The same on every machine: the saved thing's own id, or for a snippet its name. */
  id: string;
  data: unknown;
}

/** What another machine changed, for a module to apply to what it holds. */
export interface SyncChanges {
  upserts: SyncItem[];
  removed: string[];
}

/**
 * One collection a module lends. **Every one starts off** (D5): whether it is on is this machine's
 * setting (`shell/sync/enabled.ts`), drawn by the account screen (T177e2), not a property of the
 * declaration.
 */
export interface SyncableCollection {
  /** Its name in D5's table. Never on the wire — sync sends `HMAC(K_id, id)`. */
  id: string;
  labelKey: TranslationKey;
  read: () => Promise<SyncItem[]>;
  /**
   * Apply another machine's changes, and resolve to the ids of the upserts `read` will **not**
   * return as sent — data this module cannot read, or credentials parked for an item this machine
   * does not hold. Sync agrees on everything else, and an agreed id `read` leaves out is pushed as
   * a deletion. Resolves only once the disk has what was written, and rejects when it does not.
   */
  write: (changes: SyncChanges) => Promise<string[]>;
  /**
   * The row this one's credentials belong to, for a secret row (D5): it cannot go on before that
   * row, and goes off with it. Absent for every other row.
   */
  belongsTo?: string;
}

/**
 * Apply another machine's changes to a list this machine holds, keeping what belongs here.
 *
 * An upsert replaces the item with its id where it stands, or is appended; a removed id is dropped.
 * `fromSync` builds a local item from a synced one and — when this machine already has that id —
 * the one it replaces, so that what never travels survives. It answers `null` for data it cannot
 * read, and that item is left as it was rather than overwritten with something broken — and its id
 * comes back in `skipped`, so sync does not agree on it.
 */
export function applySyncChanges<T>(
  current: readonly T[],
  changes: SyncChanges,
  idOf: (item: T) => string,
  fromSync: (synced: SyncItem, local: T | undefined) => T | null,
): { items: T[]; skipped: string[] } {
  const removed = new Set(changes.removed);
  const next = current.filter((item) => !removed.has(idOf(item)));
  const skipped: string[] = [];
  for (const synced of changes.upserts) {
    const at = next.findIndex((item) => idOf(item) === synced.id);
    const built = fromSync(synced, at === -1 ? undefined : next[at]);
    if (built === null) {
      skipped.push(synced.id);
      continue;
    }
    if (at === -1) next.push(built);
    else next[at] = built;
  }
  return { items: next, skipped };
}

/** A plain object, or nothing — what a writer checks before trusting another machine's `data`. */
export function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}
