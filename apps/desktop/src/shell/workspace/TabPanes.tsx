import { Suspense } from "react";
import ErrorBoundary from "../../components/ErrorBoundary";
import LoadingOverlay from "../../components/LoadingOverlay";
import { DialogHost } from "../../components/Modal";
import { useTranslation } from "../../i18n";
import TabNotice from "../components/TabNotice";
import type { TabBadge } from "../module";
import { moduleById } from "../registry";
import type { TabInfo } from "../tabs";

interface TabPanesProps {
  tabs: TabInfo[];
  /** The tabs that have been in front at least once this launch. */
  mounted: string[];
  activeId: string;
  /** Tabs that were opened by turning their module on — tab id to module id. */
  enabledFor: Record<string, string>;
  visibleIds: string[];
  onTitleChange: (id: string, title: string) => void;
  onBadgesChange: (id: string, badges: TabBadge[]) => void;
  onStateChange: (id: string, state: unknown) => void;
  onDismissNotice: (id: string) => void;
}

/** One pane per tab that has been looked at, each the module's own `Tab`, and only the one in front
 *  shown. */
function TabPanes({
  tabs,
  mounted,
  activeId,
  enabledFor,
  visibleIds,
  onTitleChange,
  onBadgesChange,
  onStateChange,
  onDismissNotice,
}: TabPanesProps) {
  const { t } = useTranslation();
  return (
    <div className="tab-content">
      {/* Only the tabs that have been looked at. One restored from the last session is drawn on
          the strip (`WorkspaceStrip`) and has no pane here until it is picked. */}
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
                onTitleChange={(title) => onTitleChange(tab.id, title)}
                onBadgesChange={(badges) => onBadgesChange(tab.id, badges)}
                restored={tab.state}
                onStateChange={(state) => onStateChange(tab.id, state)}
              />
            </Suspense>
          </ErrorBoundary>
        );
        /* The module this tab was opened by turning on, if it was. The two wrappers are
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
                    onDismiss={() => onDismissNotice(tab.id)}
                  />
                )}
                <div className="tab-panel-body">{pane}</div>
              </div>
            </DialogHost>
          </div>
        );
      })}
    </div>
  );
}

export default TabPanes;
