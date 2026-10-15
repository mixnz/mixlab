import { useCallback, useRef, useState } from "react";

/**
 * How many times this app has changed the shape of a database, counted per thing changed — and
 * everything that follows from a change: the caches emptied, and the panes told to read again.
 *
 * Written once because the SQL and Mongo workspaces each had their own copy of it, differing in
 * nothing but the word for what a database holds. The mechanism is the part that is easy to get
 * subtly wrong — a count bumped without the entry dropped leaves a page of rows nobody returns to;
 * an entry dropped without the count bumped reaches no pane at all, because a Map is the same
 * object before and after and the pane on screen files its own copy straight back on the way out.
 */

/** The key one table's or one collection's cache entries are filed under.
 *
 *  Here rather than written out at each of the six places that needs it: the workspace empties the
 *  cache and the pane fills it, and a separator they disagreed about would leave the workspace
 *  dropping entries nobody had filed. */
export function cacheKey(database: string, item: string): string {
  return `${database} :: ${item}`;
}

/** Anything filed under a database at all — what `cacheKey` puts before every one of its items. */
export function databasePrefix(database: string): string {
  return `${database} :: `;
}

/** A cache this hook empties. Only the keys are its business, never what is filed under them. */
type Keyed = Map<string, unknown>;

export interface SchemaTokens {
  /**
   * What the panes showing one item watch: the database's count and the item's own, added, so that
   * either one moving is a change they have to notice. Both only ever go up, so their sum does too
   * — which is all the panes ask of it, since they only compare it against the one their own entry
   * was filed under.
   */
  schemaToken: number;
  /** The same count for an item that is not the one on show. Wanted when the workspace files
   *  something away on another item's behalf — the SQL grid's "show me the rows this key points
   *  at" writes a filter for a table the user has not opened yet, and it has to be filed under the
   *  token that table's own pane will compare it against. */
  tokenFor: (item: string) => number;
  /**
   * What the Statistics pane watches. Counted apart from the above, and not merely under another
   * key in it, because the figures answer to something different: they are about the database as a
   * whole, so a single item changed moves them just as a restore does — and because a database with
   * an item actually named `stats` must not be able to collide with them.
   */
  statsToken: number;
  /** Moves the count against each of `keys`, and the one the figures are read under. Every path
   *  below ends here: whatever changed, the database now holds something else. */
  bumpTokens: (database: string, keys: string[]) => void;
  /**
   * Everything remembered about these items, let go, because this app has just changed them —
   * created, renamed, dropped, or a column altered. Both names are given for a rename, since the
   * item has left one and arrived at the other.
   *
   * Waiting for the user to press reload is right for a change somebody else made on the server; it
   * is wrong for one made from in here, where what is on screen is knowably about something that no
   * longer exists in that form. A name is the sharp end of it: an item dropped and made again under
   * the same name is a different item, and the entry filed under that name would otherwise be
   * handed to it.
   */
  forget: (...items: string[]) => void;
  /** The same, for a change no single item can be named for — a dump restored over the database, or
   *  the database itself dropped — and for the sidebar's reload, which is the plainest way for the
   *  user to say "forget what you were told about this database". */
  forgetDatabase: () => void;
  /**
   * Rows or documents written, rather than the shape of anything changed.
   *
   * Nothing remembered about an item is wrong for this: the columns are where they were, and the
   * pane that did the writing has read its own page again already. What has moved is what the
   * database holds, and that is the one thing the figures on the Statistics tab are — so only their
   * count is bumped, and a session spent editing rows never costs a re-read of anything else.
   */
  contentsChanged: () => void;
}

export interface SchemaTokensOptions {
  /** The database on show, or "" before one has been chosen. Nothing is forgotten without one. */
  database: string;
  /** The item on show, or null when the workspace is showing the database itself. */
  selected: string | null;
  /** The caches filed under {@link cacheKey}, emptied whenever what they describe has changed. */
  caches: Keyed[];
  /** Anything else that has to be let go of alongside them — the SQL workspace drops the shape the
   *  Query tab completes from, which is kept per connection rather than in a cache here. */
  onForget?: (database: string) => void;
}

export function useSchemaTokens({
  database,
  selected,
  caches,
  onForget,
}: SchemaTokensOptions): SchemaTokens {
  const [schemaTokens, setSchemaTokens] = useState<Record<string, number>>({});
  const [statsTokens, setStatsTokens] = useState<Record<string, number>>({});

  /* The caches arrive as a fresh array every render even though every Map in it is the same one for
     the life of the workspace. Read through a ref so the callbacks below can stay stable — a
     `forget` that changed identity each render would be a new dependency for everything holding
     it. */
  const held = useRef(caches);
  held.current = caches;

  const tokenFor = useCallback(
    (item: string) => (schemaTokens[database] ?? 0) + (schemaTokens[cacheKey(database, item)] ?? 0),
    [schemaTokens, database],
  );

  const schemaToken = selected === null ? (schemaTokens[database] ?? 0) : tokenFor(selected);

  const bumpTokens = useCallback((db: string, keys: string[]) => {
    setSchemaTokens((tokens) => {
      const next = { ...tokens };
      for (const key of keys) next[key] = (next[key] ?? 0) + 1;
      return next;
    });
    setStatsTokens((tokens) => ({ ...tokens, [db]: (tokens[db] ?? 0) + 1 }));
  }, []);

  const forget = useCallback(
    (...items: string[]) => {
      if (!database) return;
      onForget?.(database);
      const keys = items.map((item) => cacheKey(database, item));
      for (const cache of held.current) for (const key of keys) cache.delete(key);
      bumpTokens(database, keys);
    },
    [database, onForget, bumpTokens],
  );

  const forgetDatabase = useCallback(() => {
    if (!database) return;
    onForget?.(database);
    const prefix = databasePrefix(database);
    for (const cache of held.current) {
      for (const key of cache.keys()) if (key.startsWith(prefix)) cache.delete(key);
    }
    bumpTokens(database, [database]);
  }, [database, onForget, bumpTokens]);

  const contentsChanged = useCallback(() => {
    if (!database) return;
    setStatsTokens((tokens) => ({ ...tokens, [database]: (tokens[database] ?? 0) + 1 }));
  }, [database]);

  return {
    schemaToken,
    tokenFor,
    statsToken: statsTokens[database] ?? 0,
    bumpTokens,
    forget,
    forgetDatabase,
    contentsChanged,
  };
}
