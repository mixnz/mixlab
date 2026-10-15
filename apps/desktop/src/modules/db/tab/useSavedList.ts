import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "../../../i18n";
import { engineCounts, filterConnections } from "./connectionString";
import { addConnection, updateConnection } from "../saved/savedConnectionsStore";
import { scrollTopFor } from "../saved/savedListScroll";
import type { DbKind, SavedConnection } from "../types";

interface Options {
  savedConnections: SavedConnection[];
  savedConnectionsLoaded: boolean;
  connectionId: string | null;
  /** The saved connection the form holds, marked in the list. */
  editingId: string | null;
}

/** The saved list as this tab shows it: its order, its search and chips, the row menu, and where
 *  it was scrolled to. Held by `DbTab` rather than by `SavedConnectionList`, which a workspace
 *  unmounts: all of it is still there when Disconnect brings the list back, and the effect that
 *  scrolls the list runs where it always ran, keyed on `connectionId`. */
export function useSavedList({ savedConnections, savedConnectionsLoaded, connectionId, editingId }: Options) {
  const { t, lang } = useTranslation();
  const [contextMenu, setContextMenu] = useState<{ id: string; x: number; y: number } | null>(null);

  async function duplicateSavedConnection(id: string) {
    const source = savedConnections.find((c) => c.id === id);
    if (!source) return;
    const entry: SavedConnection = {
      id: crypto.randomUUID(),
      name: t("connection.copySuffix", { name: source.name }),
      config: source.config,
      // Carried over, unlike `pinned`: a copy of a production connection points at the same
      // server, so the mark that says not to write to it has to come along. Losing it would
      // turn "duplicate" into the one click that makes a read-only connection writable.
      readOnly: source.readOnly,
      // Carried over too: the copy is still the same MixEngine account, so it should still save
      // as a reference rather than as a plaintext copy of the password.
      keyringRef: source.keyringRef,
    };
    await addConnection(entry);
  }

  async function togglePinned(id: string) {
    const entry = savedConnections.find((c) => c.id === id);
    if (!entry) return;
    // Unpinning drops the flag rather than writing `false`: absent is the default, so a connection
    // that was pinned once doesn't carry a dead `"pinned": false` in the file forever.
    await updateConnection({ ...entry, pinned: entry.pinned ? undefined : true });
  }

  async function toggleReadOnly(id: string) {
    const entry = savedConnections.find((c) => c.id === id);
    if (!entry) return;
    // Dropped rather than written as `false`, for the same reason `pinned` is.
    await updateConnection({ ...entry, readOnly: entry.readOnly ? undefined : true });
  }

  /**
   * The list as the sidebar shows it: pinned first, alphabetical within each half.
   *
   * Sorted for display only — what is on disk keeps the order it was written in, so renaming a
   * connection doesn't rewrite the file's shape. `Intl.Collator` rather than comparing strings
   * with `<`: it files accented names beside their plain spelling instead of after every
   * unaccented one, orders `db2` before `db10`, and follows the language the app is set to.
   */
  const orderedConnections = useMemo(() => {
    const collator = new Intl.Collator(lang, { sensitivity: "base", numeric: true });
    return [...savedConnections].sort((a, b) => {
      if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
      return collator.compare(a.name, b.name);
    });
  }, [savedConnections, lang]);

  /* The saved list narrowed by a name search and by engine chips — presentation over the list
     already in hand, see `connectionString.ts`. */
  const [listQuery, setListQuery] = useState("");
  const [listKinds, setListKinds] = useState<ReadonlySet<DbKind>>(new Set());
  const engines = useMemo(() => engineCounts(orderedConnections), [orderedConnections]);
  const shownConnections = useMemo(
    () => filterConnections(orderedConnections, { query: listQuery, kinds: listKinds }),
    [orderedConnections, listQuery, listKinds],
  );

  function toggleListKind(engine: DbKind) {
    setListKinds((current) => {
      const next = new Set(current);
      if (next.has(engine)) next.delete(engine);
      else next.add(engine);
      return next;
    });
  }

  function clearListFilters() {
    setListQuery("");
    setListKinds(new Set());
  }
  function openContextMenu(e: React.MouseEvent, id: string) {
    e.preventDefault();
    setContextMenu({ id, x: e.clientX, y: e.clientY });
  }

  function closeContextMenu() {
    setContextMenu(null);
  }

  /* The list, its sticky title and the row marked as the one being edited — read by the effect
     below, which is the only thing that touches them. */
  const savedListRef = useRef<HTMLElement>(null);
  const savedHeaderRef = useRef<HTMLDivElement>(null);
  const activeRowRef = useRef<HTMLLIElement>(null);
  /* Where this tab's list was last scrolled to, kept while a workspace replaces it, and the list
     element the effect below last saw — a different one is the list having been mounted again. */
  const savedListScrollRef = useRef<number | null>(null);
  const mountedListRef = useRef<HTMLElement | null>(null);
  /* What puts the marked row where it is in the list — its group and its name. The effect below
     follows the row when this changes (it was renamed or pinned) and not on any other change to the
     list: pinning, duplicating or deleting another connection, or a sync bringing one in, would
     otherwise pull a list the user had scrolled away back to the marked row. */
  const editedSortKey = useMemo(() => {
    const entry = savedConnections.find((c) => c.id === editingId);
    return entry ? `${entry.pinned ? 1 : 0}|${entry.name}` : null;
  }, [savedConnections, editingId]);

  /* The connection the form is holding, brought into view in the list beside it.

     The app coming back up on a connection saved sixty names down the alphabet renders this list
     from nothing, scrolled to the top, with the marked row somewhere below the fold — the mark is
     there and says nothing to anyone who cannot see it.

     Disconnect mounts the list again too, but that list has been looked at before: it goes back to
     where this tab left it, not to wherever the marked row would put it. The row is already on
     screen if that is where the user had it, and if they had scrolled away, that was their choice.

     A layout effect, so the list is already in the right place the first time it is painted rather
     than jumping there afterwards. `connectionId` is a dependency because leaving a workspace is
     what mounts this list again; `savedConnectionsLoaded` because the list is empty until the file
     has been read, and the row to scroll to does not exist before then. `scrollTopFor` answers
     `null` for a row already in view, so clicking around the list scrolls nothing. */
  useLayoutEffect(() => {
    const list = savedListRef.current;
    if (list === null) {
      mountedListRef.current = null;
      return;
    }
    if (mountedListRef.current === null) {
      mountedListRef.current = list;
      if (savedListScrollRef.current !== null) {
        list.scrollTop = savedListScrollRef.current;
        return;
      }
    }
    const row = activeRowRef.current;
    if (row === null) return;
    const top = row.getBoundingClientRect().top - list.getBoundingClientRect().top + list.scrollTop;
    const target = scrollTopFor(
      { top, height: row.offsetHeight },
      { scrollTop: list.scrollTop, height: list.clientHeight },
      savedHeaderRef.current?.offsetHeight ?? 0,
    );
    if (target !== null) list.scrollTop = target;
  }, [connectionId, editingId, savedConnectionsLoaded, editedSortKey]);

  return {
    ordered: orderedConnections,
    query: listQuery,
    setQuery: setListQuery,
    kinds: listKinds,
    toggleKind: toggleListKind,
    clearFilters: clearListFilters,
    engines,
    shown: shownConnections,
    contextMenu,
    openContextMenu,
    closeContextMenu,
    listRef: savedListRef,
    headerRef: savedHeaderRef,
    activeRowRef,
    scrollMemory: savedListScrollRef,
    duplicate: duplicateSavedConnection,
    togglePinned,
    toggleReadOnly,
  };
}

export type SavedList = ReturnType<typeof useSavedList>;
