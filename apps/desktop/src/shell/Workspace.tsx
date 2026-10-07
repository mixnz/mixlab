import { Suspense, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import Button from "../components/Button";
import LoadingOverlay from "../components/LoadingOverlay";
import ErrorBoundary from "../components/ErrorBoundary";
import ClosingStrip from "./components/ClosingStrip";
import SettingsModal from "./components/SettingsModal";
import TabNotice from "./components/TabNotice";
import UpdatePanel from "./components/UpdatePanel";
import ContextMenu from "../components/ContextMenu";
import { moveTab, Tab, TabAction, tabKeyDown, TabStrip, TabTitle, useTabReorder } from "../components/TabStrip";
import { CloudDownloadIcon, CloudUploadIcon, PlusIcon, SettingsIcon } from "../icons";
import { isBlockedReload } from "../core/reload";
import { configureTray } from "../core/window";
import { logError } from "../core/log";
import { useScrollAcceleration } from "../core/scroll";
import { useShortcut, useShortcutDispatcher } from "../core/shortcuts";
import { useTheme } from "./theme";
import { useTranslation } from "../i18n";
import { useUpdates } from "./update";
import type { TabBadge } from "./module";
import { onTabRequest, takeTabRequests } from "./launch";
import { readSession, writeSession } from "./session";
import {
  firstTabOfModule,
  openableModules,
  rebadgeTab,
  restateTab,
  retitleTab,
  tabIdAtOffset,
  type TabInfo,
} from "./tabs";
import { MODULES, moduleById } from "./registry";
import { defaultModuleId, visibleModules, withModule } from "./profiles";
import { newModuleTabId, shortcutsFor } from "./shortcuts";
import { startSync, SYNC_NOW_EVENT } from "./sync";
import { useSyncActivity } from "./sync/activity";
import { syncClosingHere, syncStatus } from "./sync/api";
import { closingSoon } from "./sync/closing";
import { traySections } from "./tray/sections";

interface WorkspaceProps {
  /** The module ids this window draws — `shell/profiles.ts`. */
  enabled: string[];
  onEnabledChange: (enabled: string[]) => void;
}

/** `notices` without `tabId`, or `notices` itself when there was nothing under it — so a close
 *  that had no notice to forget does not make React re-render the world. */
function forgetNotice(notices: Record<string, string>, tabId: string): Record<string, string> {
  if (!(tabId in notices)) return notices;
  const next = { ...notices };
  delete next[tabId];
  return next;
}

function Workspace({ enabled, onEnabledChange }: WorkspaceProps) {
  const { t, lang } = useTranslation();
  const { direction: syncDirection } = useSyncActivity();

  /* Which modules this window has, and everything derived from that. Memoized on the ids flattened
     to a string rather than on the array, because the array is a fresh one whenever the setting is
     written: the dispatcher rebinds its listener on identity, and the Settings table is handed the
     very same `shortcuts` value so the two cannot come to disagree. */
  const enabledKey = enabled.join(",");
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `enabledKey` is `enabled` flattened; depending on the array itself is the thing this exists to avoid.
  const visible = useMemo(() => visibleModules(enabled), [enabledKey]);
  const visibleIds = useMemo(() => visible.map((m) => m.id), [visible]);

  /* The tray icon is MixLab's and goes up whatever modules are visible (ADR 0058); what this window
     tells it is whether a visible module lends the panel a section, and the words for its menu —
     again on a language switch. `lang` is listed beside `t` because `t` is one function for the
     life of the app. */
  const hasTraySection = traySections(visible).length > 0;

  // The main window syncs; the tray panel, which never mounts a workspace, does not.
  /** The signed-in server and its announced end, when there is one (D4b). */
  const [closing, setClosing] = useState<{ server: string; on: number } | null>(null);
  /** Dismissed for this run; the next launch asks again. */
  const [closingDismissed, setClosingDismissed] = useState(false);

  useEffect(() => startSync(), []);

  // The signed-in server's closing date, read at launch and again after a sign-in or a move —
  // `requestSync` is what both of those fire.
  useEffect(() => {
    const read = () => {
      void Promise.all([syncStatus(), syncClosingHere()])
        .then(([status, on]) => setClosing(status.server && on !== null ? { server: status.server, on } : null))
        .catch(() => setClosing(null));
    };
    read();
    window.addEventListener(SYNC_NOW_EVENT, read);
    return () => window.removeEventListener(SYNC_NOW_EVENT, read);
  }, []);

  useEffect(() => {
    void configureTray(hasTraySection, {
      openPanel: t("tray.openPanel"),
      openMain: t("tray.openMain"),
      quit: t("tray.quit"),
    }).catch((e: unknown) => void logError("tray", e));
  }, [hasTraySection, lang, t]);

  function newTab(moduleId: string = defaultModuleId(visible), state?: unknown): TabInfo {
    const def = moduleById(moduleId);
    return { id: crypto.randomUUID(), moduleId, title: t(def.defaultTitleKey), badges: [], state };
  }

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

  /* Which of the visible modules a new tab can still be opened of — `shell/tabs.ts`. Everything in
     this file that offers one reads this and not `visible`: the `[+]` button, the menu behind it,
     `Ctrl/Cmd+T`, the number chords, and the table in Settings that lists the last two.

     Memoized on which *modules* have a tab rather than on `tabs`, exactly as `visible` is memoized
     on `enabledKey` and for the same reason: a tab being renamed or badged is a new `tabs` array
     several times a second, and the dispatcher rebinds its listener whenever `shortcuts` changes
     identity. */
  const openModuleKey = useMemo(
    () => [...new Set(tabs.map((tab) => tab.moduleId))].sort().join(","),
    [tabs],
  );
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `openModuleKey` is all of `tabs` that matters here; depending on the array itself is the thing this exists to avoid.
  const openable = useMemo(() => openableModules(visible, tabs), [visible, openModuleKey]);
  const openableIds = useMemo(() => openable.map((m) => m.id), [openable]);
  const shortcuts = useMemo(() => shortcutsFor(visible, openable), [visible, openable]);
  const { theme, colorTheme, setTheme, setColorTheme } = useTheme();
  const [settingsOpen, setSettingsOpen] = useState(false);
  /** The pane Settings opens on this time; `undefined` is its own default. */
  const [settingsSection, setSettingsSection] = useState<string | undefined>(undefined);
  /* MixLab's own updater — T187. Here rather than in the pane, so the Settings button can say that
     a release is waiting whatever modules are visible, and the pane reads the same state. */
  const updates = useUpdates(settingsOpen);
  /* The release on the Settings button's dot: offered, downloading or ready to install. *Later*
     hides the corner panel and not the dot, which interrupts nothing (T188 D1). */
  const pendingView = updates.view === "offer" || updates.view === "downloading" || updates.view === "ready";
  const offered = pendingView ? (updates.status?.feed?.version ?? null) : null;
  /* Where the `[+]` menu was asked for, while it is open. Never set with one module: the button
     opens a tab outright then, exactly as it did before there was a registry. */
  const [moduleMenu, setModuleMenu] = useState<{ x: number; y: number } | null>(null);

  /* Tabs that were opened by turning their module on — tab id to module id. `Workspace` state and
     not the session's per-tab slot: that slot belongs to the module (`shell/module.ts`), and by
     the next launch the sentence is false anyway — the module is simply enabled, and was when the
     session was written. T110's D2. */
  const [enabledFor, setEnabledFor] = useState<Record<string, string>>({});

  /* The drain below listens once, on purpose, so the `enabled` array in its closure is the one
     from mount. A ref written every render is the only thing in that closure that is current. */
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  useScrollAcceleration();
  useShortcutDispatcher(shortcuts);
  /* Always listening — the tab bar is there on every screen the app has. `app.newTab` is the one
     exception, and only where there is nothing left for it to open: it is off the catalogue then
     too, so the table in Settings does not offer a key that answers nothing. */
  useShortcut("app.newTab", () => openTab(defaultModuleId(openable)), openable.length > 0);
  useShortcut("app.closeTab", () => closeTab(activeId), true);
  useShortcut("app.nextTab", () => setActiveId(tabIdAtOffset(tabs, activeId, 1)), true);
  useShortcut("app.prevTab", () => setActiveId(tabIdAtOffset(tabs, activeId, -1)), true);
  /* One number key per **visible** module — `Ctrl/Cmd+1` for the first one on the strip, `2` for
     the second. Hooks in a loop, which is safe here and only here: the loop is over `MODULES`, a
     module-level constant, so the count and the order are fixed for the life of the app.

     Over the registry and not over the visible list, and that is not a detail: hooks may not change
     in number between renders, and the visible list shrinks with the profile. So all five
     registrations are made on every render and a hidden module's simply passes `false`, which
     registers nothing. Which *chord* each one carries is `shortcuts`' business — see
     `moduleTabShortcuts`. Reading the list rather than naming the modules is what keeps this file
     from being the second place that knows them. */
  for (const module of MODULES) {
    // eslint-disable-next-line react-hooks/rules-of-hooks -- module-level constant, see above: same count, same order, every render, for the life of the app.
    useShortcut(newModuleTabId(module.id), () => openOrGoTo(module.id), visibleIds.includes(module.id));
  }

  /* `state` is only ever given by the backend's tab requests below: it is what the module behind
     the tab reads on mount, through the same `restored` prop a tab from the last session gets. */
  function openTab(moduleId?: string, state?: unknown): TabInfo {
    const tab = newTab(moduleId, state);
    setTabs((prev) => [...prev, tab]);
    setActiveId(tab.id);
    return tab;
  }

  /* What a module's own number key does: open a tab of it, or go to the tab it already has and
     cannot have a second of. The second branch is only ever reached by a `singleTab` module — the
     other four are openable whatever is on the strip — and it is what keeps `Ctrl/Cmd+1` from being
     a dead key in a window showing MixEngine alone. The row in Settings says which of the two it
     is, from the same flag: see `moduleTabShortcuts`. */
  function openOrGoTo(moduleId: string) {
    if (openableIds.includes(moduleId)) {
      openTab(moduleId);
      return;
    }
    const open = firstTabOfModule(tabs, moduleId);
    if (open !== undefined) setActiveId(open);
  }

  function closeTab(id: string) {
    setTabs((prev) => {
      const next = prev.filter((t) => t.id !== id);
      return next.length > 0 ? next : [newTab()];
    });
    setMounted((prev) => prev.filter((mountedId) => mountedId !== id));
    setEnabledFor((prev) => forgetNotice(prev, id));
  }

  /* Dragging a tab along the strip. The order is the tab list itself, so a move is a new list and
     everything that follows the list follows the move — `Ctrl+Tab` walks the strip as it is drawn,
     and the session file is written the moment it changes. */
  const reorder = useTabReorder((fromId, toId, side) => {
    setTabs((prev) => moveTab(prev, fromId, toId, side));
  });

  function renameTab(id: string, title: string) {
    setTabs((prev) => retitleTab(prev, id, title));
  }

  function setTabBadges(id: string, badges: TabBadge[]) {
    setTabs((prev) => rebadgeTab(prev, id, badges));
  }

  function setTabState(id: string, state: unknown) {
    setTabs((prev) => restateTab(prev, id, state));
  }

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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `newTab` is rebuilt every render and closes over `visible`, which is in the list already.
  }, [visibleIds, tabs]);

  /* Written as it changes rather than on the way out: a desktop app is closed by the window
     manager, by a crash, or by an update restarting it, and only the first of those would ever
     reach a handler. Badge changes bring this round too — they cost a `JSON.stringify` of three
     fields per tab, which is cheaper than working out whether they mattered. */
  useEffect(() => {
    writeSession(tabs, activeId);
  }, [tabs, activeId]);

  /* Tabs the backend asks for — see `shell/launch.ts` for what they are and why they are drained
     rather than delivered. No "cancelled" guard around the drain: what has been taken from the
     backend's queue is gone from it, and a request dropped because StrictMode remounted this
     component between the call and its answer would be a tab that never opens. */
  useEffect(() => {
    /* All five ids, not the visible ones: what has been taken from the backend's queue is gone from
       it, and filtering here would leave a `mixlab://` handoff for a hidden module rotting in a queue
       nothing ever drains again. Such a tab opens and draws — `MODULES` is unchanged, so
       `moduleById` finds it — and its module is turned on below so that the visibility effect above
       does not drop it on the same commit. */
    const ids = MODULES.map((m) => m.id);
    async function drain() {
      const requests = await takeTabRequests(ids).catch(() => []);
      if (requests.length === 0) return;

      /* A module this window is not drawing is turned on for the tab that arrived — T110's D1.
         One union across the whole batch and one call: two requests for two hidden modules would
         otherwise each compute their next set from the same stale `enabled`, and the second would
         undo the first.

         Before the tabs are opened, and in the same synchronous block: `enabled` is `App`'s state
         and `tabs` is this component's, and React batches every update made in one tick — after an
         `await` included — into one commit, so `visibleIds` and `tabs` reach the visibility effect
         together and the new tab is never a tab of a hidden module. */
      const before = enabledRef.current;
      let next = before;
      for (const request of requests) next = withModule(next, request.moduleId, ids);
      if (next !== before) onEnabledChange(next);

      for (const request of requests) {
        const tab = openTab(request.moduleId, request.state);
        // Hidden when the request arrived — which is the whole of what the notice says.
        if (!before.includes(request.moduleId)) {
          setEnabledFor((prev) => ({ ...prev, [tab.id]: request.moduleId }));
        }
      }
    }
    const unlisten = onTabRequest(() => void drain());
    void drain();
    return () => {
      void unlisten.then((stop) => stop());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `openTab` is rebuilt every render and only ever calls the two stable setters, `onEnabledChange` is a `useState` setter, and the current profile is read through `enabledRef`; listening once is the point.
  }, []);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Reloading the webview takes every open connection down with it, so no keystroke is left
      // able to ask for one. What `Ctrl+R` means instead is decided by the pane on screen, which
      // claims the key for its own reload button — see `useReloadShortcut`.
      //
      // The last chord not on the registry, and it is not a command: nothing is being asked for
      // here, only refused. See `isBlockedReload` for what differs between builds.
      if (isBlockedReload(e)) e.preventDefault();
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  return (
    <main className="app">
      {/* The one strip in the app that was reachable by mouse only. Every other one — the REST
          requests, the tabs inside the response pane — already says what it is and takes Enter and
          Space; this one is the app's own tab bar, so being the exception was the wrong way round.
          `Ctrl+Tab` moved between tabs all along, but only once one was already open and focused. */}
      <TabStrip
        role="tablist"
        aria-label={t("app.tabs")}
        {...reorder.strip}
        /* The app's mark and name, closed off from the tabs by a rule. Not a control: Settings has
           a button of its own at the far end. */
        leading={
          <div className="brand">
            <img className="brand-mark" src="/logo.svg" alt="" width={28} height={28} />
            <span className="brand-name">MixLab</span>
          </div>
        }
        /* In `end`, so that a window full of tabs cannot scroll the way into Settings off the strip
           — it is the one control that is there on every screen the app has. */
        end={
          <Button
            className={offered !== null ? "brand-settings has-update" : "brand-settings"}
            onClick={() => {
              setSettingsSection(undefined);
              setSettingsOpen(true);
            }}
            title={
              syncDirection === "up"
                ? t("app.settingsUploading")
                : syncDirection === "down"
                  ? t("app.settingsDownloading")
                  : offered !== null
                    ? t("update.available", { version: offered })
                    : t("app.settings")
            }
            aria-label={t("app.settings")}
          >
            {/* Down while sync reads the server, up only while this machine's changes are really
                being sent: a pull must never look like something leaving. */}
            {syncDirection === "up" ? (
              <CloudUploadIcon size={17} moving />
            ) : syncDirection === "down" ? (
              <CloudDownloadIcon size={17} moving />
            ) : (
              <SettingsIcon size={17} />
            )}
          </Button>
        }
        /* Not drawn at all once there is nothing left to open — a window holding the one tab its
           one module has. A `[+]` that could only ever open what is already in front of you is a
           control that lies about what the app can do, and the close button beside it is what such
           a window uses instead: closing its last tab puts a fresh one in its place, which is how
           the module is reloaded. See `openableModules`. */
        trailing={
          openable.length > 0 ? (
            <TabAction
              onClick={(e) => {
                // One module to offer and a menu would be a list of one, so the button just opens
                // it — which is what it did before there was a registry at all.
                if (openable.length < 2) {
                  openTab(defaultModuleId(openable));
                  return;
                }
                const rect = e.currentTarget.getBoundingClientRect();
                setModuleMenu({ x: rect.left, y: rect.bottom });
              }}
              title={t("app.newConnectionTab")}
              aria-label={t("app.newConnectionTab")}
            >
              <PlusIcon />
            </TabAction>
          ) : undefined
        }
      >
        {tabs.map((tab) => {
          const def = moduleById(tab.moduleId);
          return (
            <Tab
              key={tab.id}
              active={tab.id === activeId}
              role="tab"
              aria-selected={tab.id === activeId}
              tabIndex={0}
              className={tab.badges.map((b) => b.tabClassName).filter(Boolean).join(" ")}
              onClose={() => closeTab(tab.id)}
              /* The last tab is not closed by this button, it is replaced — `closeTab` puts a fresh
                 one where it was, because a window with no tabs is not a window. So on that one tab
                 the button says what it actually does. True in every profile, not only the
                 single-module one: it is a property of being the last tab. */
              closeLabel={tabs.length === 1 ? t("app.reloadTab") : t("app.closeTab")}
              onClick={() => setActiveId(tab.id)}
              onKeyDown={tabKeyDown(() => setActiveId(tab.id))}
              {...reorder.tab(tab.id)}
            >
              {/* A tab with nothing of its own to say still says which module it is, and most of
                  them have a spell of having nothing to say: a database tab wears no engine until
                  it is connected to one, a terminal none until a shell is picked, and a tab
                  restored from the last session has no module running behind it at all until it is
                  first looked at — it is a name on the strip and nothing else. So the module's own
                  mark stands in, the one the [+] menu opens it from, dimmer than a badge that was
                  actually asked for. */}
              {tab.badges.length === 0 && (
                <span className="tab-badge tab-module" title={t(def.labelKey)}>
                  <def.Icon size={14} />
                  <span className="visually-hidden">{t(def.labelKey)}</span>
                </span>
              )}
              {/* Ahead of the name, where the eye lands first: a mark is there to be seen before a
                  statement is typed, not after the connection has been identified. What each one
                  means is the module's business — the shell only puts it where it goes. The tab is
                  not a control with a name of its own, so the word travels with the mark for anyone
                  who can't see it. */}
              {tab.badges.map((badge) => (
                <span
                  key={badge.id}
                  className={badge.className ? `tab-badge ${badge.className}` : "tab-badge"}
                  title={badge.title}
                >
                  {badge.icon}
                  <span className="visually-hidden">{badge.label}</span>
                </span>
              ))}
              <TabTitle>{tab.title}</TabTitle>
            </Tab>
          );
        })}
      </TabStrip>

      {/* The modules that can still take a new tab, in the registry's order. Unreachable while
          there is one of them — the button above opens it outright rather than showing a menu of
          one — and unreachable while there are none, where there is no button at all. */}
      {moduleMenu && (
        <ContextMenu x={moduleMenu.x} y={moduleMenu.y} onClose={() => setModuleMenu(null)}>
          {openable.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => {
                setModuleMenu(null);
                openTab(m.id);
              }}
            >
              <m.Icon size={14} />
              {t(m.labelKey)}
            </button>
          ))}
        </ContextMenu>
      )}

      {closing && !closingDismissed && closingSoon(closing.on, Date.now()) && (
        <ClosingStrip
          server={closing.server}
          closingOn={closing.on}
          onOpen={() => {
            setSettingsSection("sync");
            setSettingsOpen(true);
          }}
          onDismiss={() => setClosingDismissed(true)}
        />
      )}

      <div className="tab-content">
        {/* Only the tabs that have been looked at. One restored from the last session is drawn on
            the strip above and has no pane down here until it is picked. */}
        {tabs.filter((tab) => mounted.includes(tab.id)).map((tab) => {
          const { Tab } = moduleById(tab.moduleId);
          /* Each module's workspace arrives on first use — see the note beside its `Tab`.
             One boundary per tab and not one around the list: a tab still loading must not
             take the panes beside it off screen while it does. The Error Boundary follows
             the same rule, and for the same reason: a tab that crashes must not take the
             panes beside it, the tab strip, or the Settings dialog (all of them in App,
             outside every boundary) down with it. Keyed on tab.id so closing a crashed tab and
             opening a new one is a fresh boundary, not the old one still remembering the error. */
          const pane = (
            <ErrorBoundary key={tab.id}>
              <Suspense fallback={<LoadingOverlay />}>
                <Tab
                  active={tab.id === activeId}
                  isModuleVisible={(id) => visibleIds.includes(id)}
                  onTitleChange={(title) => renameTab(tab.id, title)}
                  onBadgesChange={(badges) => setTabBadges(tab.id, badges)}
                  restored={tab.state}
                  onStateChange={(state) => setTabState(tab.id, state)}
                />
              </Suspense>
            </ErrorBoundary>
          );
          /* The module this tab was opened by turning on, if it was — T110. The two wrappers are
             always here and are boxes only while there is a notice: a pane that changed depth when
             one appeared or was dismissed would be a fresh mount, and a database tab would drop the
             connection it was holding. `App.css` is where the rest of that is written down. */
          const noticeModuleId = enabledFor[tab.id];
          return (
            <div
              key={tab.id}
              className={noticeModuleId ? "tab-panel tab-panel-noticed" : "tab-panel"}
              style={{ display: tab.id === activeId ? "flex" : "none" }}
            >
              <div className="tab-panel-stack">
                {noticeModuleId && (
                  <TabNotice
                    message={t("profiles.turnedOn", {
                      module: t(moduleById(noticeModuleId).labelKey),
                    })}
                    onDismiss={() => setEnabledFor((prev) => forgetNotice(prev, tab.id))}
                  />
                )}
                <div className="tab-panel-body">{pane}</div>
              </div>
            </div>
          );
        })}
      </div>

      {/* MixLab's own release, offered in the corner: T188. Under Settings when that is open, and
          drawn whatever modules are visible (ADR 0056). T187's once-per-release strip is gone: one
          notice for one release. */}
      <UpdatePanel
        updates={updates}
        onInstallerOpened={() => {
          setSettingsSection("update");
          setSettingsOpen(true);
        }}
      />

      {settingsOpen && (
        <SettingsModal
          theme={theme}
          onThemeChange={setTheme}
          colorTheme={colorTheme}
          onColorThemeChange={setColorTheme}
          shortcuts={shortcuts}
          modules={{
            enabled,
            onChange: onEnabledChange,
            openIds: [...new Set(tabs.map((tab) => tab.moduleId))],
          }}
          initialSection={settingsSection}
          onClose={() => setSettingsOpen(false)}
          updates={updates}
        />
      )}
    </main>
  );
}

export default Workspace;
