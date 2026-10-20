import { useCallback, useEffect, useLayoutEffect, useState, type Dispatch, type SetStateAction } from "react";
import { moveTab, type DropSide } from "../../components/TabStrip";
import { useTranslation } from "../../i18n";
import type { ModuleDefinition, TabBadge } from "../module";
import { defaultModuleId } from "../profiles";
import { moduleById } from "../registry";
import { readSession, writeSession } from "../session";
import { rebadgeTab, restateTab, retitleTab, type TabInfo } from "../tabs";

/** `notices` without `tabId`, or `notices` itself when there was nothing under it — so a close
 *  that had no notice to forget does not make React re-render the world. */
export function forgetNotice(notices: Record<string, string>, tabId: string): Record<string, string> {
  if (!(tabId in notices)) return notices;
  const next = { ...notices };
  delete next[tabId];
  return next;
}

interface TabsOptions {
  visible: ModuleDefinition[];
  visibleIds: string[];
}

export interface Tabs {
  tabs: TabInfo[];
  activeId: string;
  setActiveId: Dispatch<SetStateAction<string>>;
  /** The tabs that have been in front at least once this launch — the only ones rendered. */
  mounted: string[];
  /** Tabs that were opened by turning their module on — tab id to module id. */
  enabledFor: Record<string, string>;
  setEnabledFor: Dispatch<SetStateAction<Record<string, string>>>;
  openTab: (moduleId?: string, state?: unknown) => TabInfo;
  closeTab: (id: string) => void;
  reorderTabs: (fromId: string, toId: string, side: DropSide) => void;
  renameTab: (id: string, title: string) => void;
  setTabBadges: (id: string, badges: TabBadge[]) => void;
  setTabState: (id: string, state: unknown) => void;
}

/** The shell's strip: which tabs there are, which is in front, which have been drawn, and the
 *  session that remembers them. */
export function useTabs({ visible, visibleIds }: TabsOptions): Tabs {
  const { t } = useTranslation();

  const newTab = useCallback(
    (moduleId: string = defaultModuleId(visible), state?: unknown): TabInfo => {
      const def = moduleById(moduleId);
      return { id: crypto.randomUUID(), moduleId, title: t(def.defaultTitleKey), badges: [], state };
    },
    [visible, t],
  );

  /* What was open when the app was last closed, read once on the way up. The strip and, per tab,
     one opaque value the module behind it asked to have kept — the shell carries it and does not
     read it. What a module does with its own is up to the module, and it does not do it until the
     tab is first looked at. See `shell/session.ts`. */
  const [restored] = useState(() => readSession(visibleIds));
  const [tabs, setTabs] = useState<TabInfo[]>(() =>
    // Badges are never stored; the module reports its own the moment it mounts.
    restored ? restored.tabs.map((tab) => ({ ...tab, badges: [] })) : [newTab()],
  );
  const [activeId, setActiveId] = useState(() => restored?.activeId ?? tabs[0].id);
  /* The tabs that have been in front at least once this launch — the only ones rendered.
     Restoring six tabs by mounting six panes would open six connection forms and start six
     shells at launch, for tabs the user may never come back to; the rest of them sit on the strip
     and wait, and mount the first time they are looked at. */
  const [mounted, setMounted] = useState<string[]>(() => [activeId]);

  /* Tabs that were opened by turning their module on — tab id to module id. `Workspace` state and
     not the session's per-tab slot: that slot belongs to the module (`shell/module.ts`), and by
     the next launch the sentence is false anyway — the module is simply enabled, and was when the
     session was written. T110's D2. */
  const [enabledFor, setEnabledFor] = useState<Record<string, string>>({});

  /* `state` is only ever given by the backend's tab requests (`useTabRequests`): it is what the
     module behind the tab reads on mount, through the same `restored` prop a tab from the last
     session gets. */
  const openTab = useCallback(
    (moduleId?: string, state?: unknown): TabInfo => {
      const tab = newTab(moduleId, state);
      setTabs((prev) => [...prev, tab]);
      setActiveId(tab.id);
      return tab;
    },
    [newTab],
  );

  const closeTab = useCallback(
    (id: string) => {
      setTabs((prev) => {
        const next = prev.filter((t) => t.id !== id);
        return next.length > 0 ? next : [newTab()];
      });
      setMounted((prev) => prev.filter((mountedId) => mountedId !== id));
      setEnabledFor((prev) => forgetNotice(prev, id));
    },
    [newTab],
  );

  /* Dragging a tab along the strip. The order is the tab list itself, so a move is a new list and
     everything that follows the list follows the move — `Ctrl+Tab` walks the strip as it is drawn,
     and the session file is written the moment it changes. */
  const reorderTabs = useCallback((fromId: string, toId: string, side: DropSide) => {
    setTabs((prev) => moveTab(prev, fromId, toId, side));
  }, []);

  const renameTab = useCallback((id: string, title: string) => {
    setTabs((prev) => retitleTab(prev, id, title));
  }, []);

  const setTabBadges = useCallback((id: string, badges: TabBadge[]) => {
    setTabs((prev) => rebadgeTab(prev, id, badges));
  }, []);

  const setTabState = useCallback((id: string, state: unknown) => {
    setTabs((prev) => restateTab(prev, id, state));
  }, []);

  // Keeps activeId pointing at a real tab whenever the active one disappears
  // (e.g. closing the last remaining tab spawns a fresh one that must be focused).
  useLayoutEffect(() => {
    if (!tabs.some((t) => t.id === activeId)) {
      setActiveId(tabs[tabs.length - 1].id);
    }
  }, [tabs, activeId]);

  // Whatever is in front is mounted, and stays mounted for the rest of the launch — every pane in
  // the app is written on the understanding that leaving a tab does not throw its state away.
  useEffect(() => {
    setMounted((prev) => (prev.includes(activeId) ? prev : [...prev, activeId]));
  }, [activeId]);

  /* A module turned off takes its tabs with it. An effect on the visible list rather than a branch
     inside the Settings handler, because there is more than one way that list changes — the three
     presets, the five checkboxes, and later T110's handoff — and a rule enforced at one call site
     is a rule the second call site breaks. The confirmation happens before the setting changes, in
     the Settings pane; by the time this runs the answer was yes.

     Both updaters return their input unchanged when there is nothing to drop, so this does not
     rewrite the session on every render it happens to run in. Closing the last tab opens a fresh
     one, exactly as `closeTab` does. `activeId` needs nothing: the layout effect above already
     keeps it pointing at a tab that exists. */
  useEffect(() => {
    setTabs((prev) => {
      const kept = prev.filter((tab) => visibleIds.includes(tab.moduleId));
      if (kept.length === prev.length) return prev;
      return kept.length > 0 ? kept : [newTab()];
    });
    setMounted((prev) => {
      const kept = prev.filter((id) =>
        tabs.some((tab) => tab.id === id && visibleIds.includes(tab.moduleId)),
      );
      return kept.length === prev.length ? prev : kept;
    });
    // A notice belongs to a tab; a tab that went with its module takes it along.
    setEnabledFor((prev) => {
      const kept = Object.fromEntries(
        Object.entries(prev).filter(([tabId]) =>
          tabs.some((tab) => tab.id === tabId && visibleIds.includes(tab.moduleId)),
        ),
      );
      return Object.keys(kept).length === Object.keys(prev).length ? prev : kept;
    });
  }, [visibleIds, tabs, newTab]);

  /* Written as it changes rather than on the way out: a desktop app is closed by the window
     manager, by a crash, or by an update restarting it, and only the first of those would ever
     reach a handler. Badge changes bring this round too — they cost a `JSON.stringify` of three
     fields per tab, which is cheaper than working out whether they mattered. */
  useEffect(() => {
    writeSession(tabs, activeId);
  }, [tabs, activeId]);

  return {
    tabs,
    activeId,
    setActiveId,
    mounted,
    enabledFor,
    setEnabledFor,
    openTab,
    closeTab,
    reorderTabs,
    renameTab,
    setTabBadges,
    setTabState,
  };
}
