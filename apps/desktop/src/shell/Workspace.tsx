import { useMemo, useState } from "react";
import ClosingStrip from "./components/ClosingStrip";
import SettingsModal from "./components/SettingsModal";
import UpdatePanel from "./components/UpdatePanel";
import RemoveDialog from "./components/RemoveDialog";
import { useScrollAcceleration } from "../core/scroll";
import { useShortcut, useShortcutDispatcher, useShortcuts } from "../core/shortcuts";
import { useTheme } from "./theme";
import { useUpdates } from "./update";
import { firstTabOfModule, idsKey, idsOf, openableModules, tabIdAtOffset } from "./tabs";
import { MODULES } from "./registry";
import { defaultModuleId, visibleModules } from "./profiles";
import { newModuleTabId, shortcutsFor } from "./shortcuts";
import { closingSoon } from "./sync/closing";
import { traySections } from "./tray/sections";
import { useSyncClosing } from "./workspace/useSyncClosing";
import { useWindowChrome } from "./workspace/useWindowChrome";
import { forgetNotice, useTabs } from "./workspace/useTabs";
import { useTabRequests } from "./workspace/useTabRequests";
import TabPanes from "./workspace/TabPanes";
import WorkspaceStrip from "./workspace/WorkspaceStrip";

interface WorkspaceProps {
  /** The module ids this window draws — `shell/profiles.ts`. */
  enabled: string[];
  onEnabledChange: (enabled: string[]) => void;
}

function Workspace({ enabled, onEnabledChange }: WorkspaceProps) {
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
  /* MixLab's own updater. Here rather than in the pane, so the Settings button can say that
     a release is waiting whatever modules are visible, and the pane reads the same state. */
  const updates = useUpdates(settingsOpen);
  /* MixLab ▸ Remove MixLab from this Mac…. The menu item and its words are
     `useWindowChrome`'s; the dialog it opens is drawn here. */
  const [removeOpen, setRemoveOpen] = useState(false);
  /* The release on the Settings button's dot: offered, downloading or ready to install. *Later*
     hides the corner panel and not the dot, which interrupts nothing. */
  const pendingView = updates.view === "offer" || updates.view === "downloading" || updates.view === "ready";
  const offered = pendingView ? (updates.status?.feed?.version ?? null) : null;

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

  useTabRequests({ enabled, onEnabledChange, openTab, setEnabledFor });

  useWindowChrome({ hasTraySection, onRemoveRequested: () => setRemoveOpen(true) });

  return (
    <main className="app">
      <WorkspaceStrip
        tabs={tabs}
        activeId={activeId}
        onSelect={setActiveId}
        onClose={closeTab}
        onReorder={reorderTabs}
        openable={openable}
        onOpen={openTab}
        offered={offered}
        onOpenSettings={() => {
          setSettingsSection(undefined);
          setSettingsOpen(true);
        }}
      />

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

      <TabPanes
        tabs={tabs}
        mounted={mounted}
        activeId={activeId}
        enabledFor={enabledFor}
        visibleIds={visibleIds}
        onTitleChange={renameTab}
        onBadgesChange={setTabBadges}
        onStateChange={setTabState}
        onDismissNotice={(id) => setEnabledFor((prev) => forgetNotice(prev, id))}
      />

      {/* MixLab's own release, offered in the corner. Under Settings when that is open, and drawn
          whatever modules are visible (ADR 0056). The strip once shown per release is gone: one
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
