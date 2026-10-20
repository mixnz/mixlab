import { Suspense, useMemo, useState } from "react";
import Button from "../components/Button";
import LoadingOverlay from "../components/LoadingOverlay";
import { DialogHost } from "../components/Modal";
import ErrorBoundary from "../components/ErrorBoundary";
import ClosingStrip from "./components/ClosingStrip";
import SettingsModal from "./components/SettingsModal";
import TabNotice from "./components/TabNotice";
import UpdatePanel from "./components/UpdatePanel";
import RemoveDialog from "./components/RemoveDialog";
import ContextMenu from "../components/ContextMenu";
import { Tab, TabAction, tabKeyDown, TabStrip, TabTitle, useTabReorder } from "../components/TabStrip";
import { CloudDownloadIcon, CloudUploadIcon, PlusIcon, SettingsIcon } from "../icons";
import { useScrollAcceleration } from "../core/scroll";
import { useShortcut, useShortcutDispatcher, useShortcuts } from "../core/shortcuts";
import { useTheme } from "./theme";
import { useTranslation } from "../i18n";
import { useUpdates } from "./update";
import { firstTabOfModule, idsKey, idsOf, openableModules, tabIdAtOffset } from "./tabs";
import { MODULES, moduleById } from "./registry";
import { defaultModuleId, visibleModules } from "./profiles";
import { newModuleTabId, shortcutsFor } from "./shortcuts";
import { useSyncActivity } from "./sync/activity";
import { closingSoon } from "./sync/closing";
import { traySections } from "./tray/sections";
import { useSyncClosing } from "./workspace/useSyncClosing";
import { useWindowChrome } from "./workspace/useWindowChrome";
import { forgetNotice, useTabs } from "./workspace/useTabs";
import { useTabRequests } from "./workspace/useTabRequests";

interface WorkspaceProps {
  /** The module ids this window draws — `shell/profiles.ts`. */
  enabled: string[];
  onEnabledChange: (enabled: string[]) => void;
}

function Workspace({ enabled, onEnabledChange }: WorkspaceProps) {
  const { t } = useTranslation();
  const { direction: syncDirection } = useSyncActivity();

  /* Which modules this window has, and everything derived from that. Memoized on the ids flattened
     to a string rather than on the array, because the array is a fresh one whenever the setting is
     written: the dispatcher rebinds its listener on identity, and the Settings table is handed the
     very same `shortcuts` value so the two cannot come to disagree. */
  const enabledKey = idsKey(enabled);
  const visible = useMemo(() => visibleModules(idsOf(enabledKey)), [enabledKey]);
  const visibleIds = useMemo(() => visible.map((m) => m.id), [visible]);

  // Whether a visible module lends the tray panel a section — what `useWindowChrome` tells the tray.
  const hasTraySection = traySections(visible).length > 0;

  const syncClosing = useSyncClosing();

  const {
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
  } = useTabs({ visible, visibleIds });

  /* Which of the visible modules a new tab can still be opened of — `shell/tabs.ts`. Everything in
     this file that offers one reads this and not `visible`: the `[+]` button, the menu behind it,
     `Ctrl/Cmd+T`, the number chords, and the table in Settings that lists the last two.

     Memoized on which *modules* have a tab rather than on `tabs`, exactly as `visible` is memoized
     on `enabledKey` and for the same reason: a tab being renamed or badged is a new `tabs` array
     several times a second, and the dispatcher rebinds its listener whenever `shortcuts` changes
     identity. */
  const openModuleKey = useMemo(
    () => idsKey([...new Set(tabs.map((tab) => tab.moduleId))].sort()),
    [tabs],
  );
  const openable = useMemo(() => openableModules(visible, idsOf(openModuleKey)), [visible, openModuleKey]);
  const openableIds = useMemo(() => openable.map((m) => m.id), [openable]);
  const shortcuts = useMemo(() => shortcutsFor(visible, openable), [visible, openable]);
  const { theme, colorTheme, setTheme, setColorTheme } = useTheme();
  const [settingsOpen, setSettingsOpen] = useState(false);
  /** The pane Settings opens on this time; `undefined` is its own default. */
  const [settingsSection, setSettingsSection] = useState<string | undefined>(undefined);
  /* MixLab's own updater — T187. Here rather than in the pane, so the Settings button can say that
     a release is waiting whatever modules are visible, and the pane reads the same state. */
  const updates = useUpdates(settingsOpen);
  /* MixLab ▸ Remove MixLab from this Mac…: T182a. The menu item and its words are
     `useWindowChrome`'s; the dialog it opens is drawn here. */
  const [removeOpen, setRemoveOpen] = useState(false);
  /* The release on the Settings button's dot: offered, downloading or ready to install. *Later*
     hides the corner panel and not the dot, which interrupts nothing (T188 D1). */
  const pendingView = updates.view === "offer" || updates.view === "downloading" || updates.view === "ready";
  const offered = pendingView ? (updates.status?.feed?.version ?? null) : null;
  /* Where the `[+]` menu was asked for, while it is open. Never set with one module: the button
     opens a tab outright then, exactly as it did before there was a registry. */
  const [moduleMenu, setModuleMenu] = useState<{ x: number; y: number } | null>(null);

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
     the second. One `useShortcuts` call for all of them, so no hook is called in a loop.

     Over the registry and not over the visible list: every module has an entry on every render, and
     a hidden module's simply says it is off, which registers nothing. Which *chord* each one carries
     is `shortcuts`' business — see `moduleTabShortcuts`. Reading the list rather than naming the
     modules is what keeps this file from being the second place that knows them. */
  useShortcuts(
    MODULES.map((module) => ({
      id: newModuleTabId(module.id),
      handler: () => openOrGoTo(module.id),
      enabled: visibleIds.includes(module.id),
    })),
  );

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

  const reorder = useTabReorder(reorderTabs);

  useTabRequests({ enabled, onEnabledChange, openTab, setEnabledFor });

  useWindowChrome({ hasTraySection, onRemoveRequested: () => setRemoveOpen(true) });

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

      {syncClosing.closing && !syncClosing.dismissed && closingSoon(syncClosing.closing.on, Date.now()) && (
        <ClosingStrip
          server={syncClosing.closing.server}
          closingOn={syncClosing.closing.on}
          onOpen={() => {
            setSettingsSection("sync");
            setSettingsOpen(true);
          }}
          onDismiss={syncClosing.dismiss}
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
             connection it was holding. `chrome.css` is where the rest of that is written down. */
          const noticeModuleId = enabledFor[tab.id];
          return (
            <div
              key={tab.id}
              className={noticeModuleId ? "tab-panel tab-panel-noticed" : "tab-panel"}
              style={{ display: tab.id === activeId ? "flex" : "none" }}
            >
              {/* The tab's dialogs are drawn over its pane and hide with it — `Modal/host.tsx`. */}
              <DialogHost>
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
              </DialogHost>
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

      {removeOpen && <RemoveDialog onClose={() => setRemoveOpen(false)} />}

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
