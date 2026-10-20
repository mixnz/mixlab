import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { moveId, type DropSide } from "../../../components/TabStrip";
import { findRequest, isBlank } from "../requests";
import { currentLists, deleteRequest, useRequestListsLoaded } from "../requestsStore";
import { parseRestTabState } from "../tabState";
import type { RequestLists, RestRequest } from "../types";

interface OpenRequestsOptions {
  lists: RequestLists;
  /** What the shell kept for this tab last launch — `ModuleTabProps.restored`. */
  restored: unknown;
  onStateChange: (state: unknown) => void;
}

export interface OpenRequests {
  /** The open tabs, resolved afresh from the store. */
  tabs: RestRequest[];
  /** The request actually on screen, if any. */
  activeRequest: RestRequest | undefined;
  currentId: string | null;
  setActiveId: Dispatch<SetStateAction<string | null>>;
  open: (id: string) => void;
  close: (id: string) => void;
  reorder: (fromId: string, toId: string, side: DropSide) => void;
}

/** The requests open on this tab's strip, which one is in front, and what the shell keeps of them. */
export function useOpenRequests({ lists, restored, onStateChange }: OpenRequestsOptions): OpenRequests {
  const [openIds, setOpenIds] = useState<string[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  /* Which requests were open here last launch, taken once. A snapshot and not a live read: this
     tab writes a new value the moment the strip moves. */
  const [restoredState] = useState(() => parseRestTabState(restored));
  const requestsLoaded = useRequestListsLoaded();
  /** Whether the restore below has had its one turn — and, because the write is an effect, the
   *  gate that keeps an empty strip from being written before that turn comes. */
  const restoreTried = useRef(false);
  /** The ids the restore handed to `setOpenIds`, held until the render that has them. `setOpenIds`
   *  does not take effect in the commit that calls it, so without this the write effect below runs
   *  once more against the empty strip it is about to replace — and writes "forget it" over the
   *  very state that was just read back. */
  const restoreApplied = useRef<string[] | null>(null);

  /* The open tabs, resolved afresh from the store: a request edited anywhere shows its new name
     here, and one deleted from the sidebar takes its tab with it. */
  const tabs = useMemo(
    () => openIds.map((id) => findRequest(lists, id)).filter((r): r is RestRequest => r !== undefined),
    [openIds, lists],
  );
  /**
   * The request actually on screen, and its id.
   *
   * `activeId` is what was last *chosen*, and it can name a tab that has just been closed or a
   * request that has just been deleted — for the one render before the effects below catch up.
   * Resolving through `tabs` and falling back to the last of them means that in-between state is
   * never drawn: without it, closing one of two untitled tabs shows the shell's tab renamed to
   * "New request" for a frame before it settles on the neighbour.
   */
  const activeRequest = tabs.find((r) => r.id === activeId) ?? tabs[tabs.length - 1];
  const currentId = activeRequest?.id ?? null;

  // A tab whose request is gone stops being open, and the keyboard lands on the one beside it.
  useEffect(() => {
    setOpenIds((prev) => {
      const next = prev.filter((id) => findRequest(lists, id) !== undefined);
      // `filter` always allocates; returning that would re-render the workspace on every keystroke,
      // since every edit publishes a new list.
      return next.length === prev.length ? prev : next;
    });
  }, [lists]);
  useEffect(() => {
    if (activeId !== null && !openIds.includes(activeId)) {
      setActiveId(openIds[openIds.length - 1] ?? null);
    }
  }, [openIds, activeId]);

  /* The strip coming back to what was on it, once, when the request list has actually been read.
     Before that the list is empty and every id would look deleted. Ids whose request has gone are
     dropped — including a blank request the store swept away on load, which is the common case —
     and the choice falls to the last tab left when the one that was in front is one of them. */
  useEffect(() => {
    if (restoreTried.current) return;
    // Nothing to restore still has to open the gate, or the write below never runs.
    if (restoredState !== null && !requestsLoaded) return;
    restoreTried.current = true;
    if (restoredState === null) return;
    const ids = restoredState.openIds.filter((id) => findRequest(lists, id) !== undefined);
    // Every one of them gone is a strip that really is empty, and the write below says so.
    if (ids.length === 0) return;
    restoreApplied.current = ids;
    setOpenIds(ids);
    setActiveId(
      restoredState.activeId !== null && ids.includes(restoredState.activeId)
        ? restoredState.activeId
        : ids[ids.length - 1],
    );
  }, [restoredState, requestsLoaded, lists]);

  /* Written from an effect rather than from each of the handlers that move the strip — there are
     six of them, and one forgotten is a tab that comes back wrong. `currentId` and not `activeId`
     because that is the tab actually on screen. `onStateChange` is reached through an effect event
     rather than listed: `App` hands down a fresh closure every render, and the shell compares state
     by identity, so depending on it is the render loop named at the top of `shell/tabs.ts`. */
  const reportState = useEffectEvent((state: unknown) => onStateChange(state));
  useEffect(() => {
    if (!restoreTried.current) return;
    if (restoreApplied.current !== null) {
      // `setOpenIds` was handed exactly this array, so identity is the signal that it has landed.
      if (openIds !== restoreApplied.current) return;
      restoreApplied.current = null;
    }
    reportState(openIds.length === 0 ? undefined : { openIds, activeId: currentId });
  }, [openIds, currentId]);

  const open = useCallback((id: string) => {
    setOpenIds((prev) => (prev.includes(id) ? prev : [...prev, id]));
    setActiveId(id);
  }, []);

  /**
   * Closing a tab, and taking the request with it when there is nothing in it.
   *
   * Where the keyboard lands next is decided here rather than left to the effect that moves the
   * choice, so both pieces of state change in the same commit: the effect would run a render later,
   * and that render is one whose active tab has already gone.
   *
   * The request is read from the store rather than from `lists`: this may run from a shortcut whose
   * handler was made several keystrokes ago, and what matters is the request as it stands now.
   */
  const close = useCallback(
    (id: string) => {
      const next = openIds.filter((openId) => openId !== id);
      setOpenIds(next);
      if (activeId === id) setActiveId(next[next.length - 1] ?? null);
      const request = findRequest(currentLists(), id);
      if (request !== undefined && isBlank(request)) deleteRequest(id);
    },
    [openIds, activeId],
  );

  const reorder = useCallback((fromId: string, toId: string, side: DropSide) => {
    setOpenIds((prev) => moveId(prev, fromId, toId, side));
  }, []);

  return { tabs, activeRequest, currentId, setActiveId, open, close, reorder };
}
