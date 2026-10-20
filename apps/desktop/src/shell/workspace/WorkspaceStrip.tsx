import { useState } from "react";
import Button from "../../components/Button";
import ContextMenu from "../../components/ContextMenu";
import { Tab, TabAction, tabKeyDown, TabStrip, TabTitle, useTabReorder, type DropSide } from "../../components/TabStrip";
import { CloudDownloadIcon, CloudUploadIcon, PlusIcon, SettingsIcon } from "../../icons";
import { useTranslation } from "../../i18n";
import type { ModuleDefinition } from "../module";
import { defaultModuleId } from "../profiles";
import { moduleById } from "../registry";
import { useSyncActivity } from "../sync/activity";
import type { TabInfo } from "../tabs";

interface WorkspaceStripProps {
  tabs: TabInfo[];
  activeId: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onReorder: (fromId: string, toId: string, side: DropSide) => void;
  /** The modules a new tab can still be opened of — `openableModules`. */
  openable: ModuleDefinition[];
  /** Opens a tab of `moduleId`. */
  onOpen: (moduleId: string) => void;
  /** The release on the Settings button's dot, or `null`. */
  offered: string | null;
  onOpenSettings: () => void;
}

/**
 * The app's own tab bar, and the `[+]` menu that opens from it.
 *
 * Always drawn, so the menu's position can live here: it is mounted exactly as long as the
 * workspace is.
 */
function WorkspaceStrip({
  tabs,
  activeId,
  onSelect,
  onClose,
  onReorder,
  openable,
  onOpen,
  offered,
  onOpenSettings,
}: WorkspaceStripProps) {
  const { t } = useTranslation();
  const { direction: syncDirection } = useSyncActivity();
  /* Where the `[+]` menu was asked for, while it is open. Never set with one module: the button
     opens a tab outright then, exactly as it did before there was a registry. */
  const [moduleMenu, setModuleMenu] = useState<{ x: number; y: number } | null>(null);
  const reorder = useTabReorder(onReorder);

  return (
    <>
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
            onClick={onOpenSettings}
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
                  onOpen(defaultModuleId(openable));
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
              onClose={() => onClose(tab.id)}
              /* The last tab is not closed by this button, it is replaced — `closeTab` puts a fresh
                 one where it was, because a window with no tabs is not a window. So on that one tab
                 the button says what it actually does. True in every profile, not only the
                 single-module one: it is a property of being the last tab. */
              closeLabel={tabs.length === 1 ? t("app.reloadTab") : t("app.closeTab")}
              onClick={() => onSelect(tab.id)}
              onKeyDown={tabKeyDown(() => onSelect(tab.id))}
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
                onOpen(m.id);
              }}
            >
              <m.Icon size={14} />
              {t(m.labelKey)}
            </button>
          ))}
        </ContextMenu>
      )}
    </>
  );
}

export default WorkspaceStrip;
