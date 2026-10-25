import type { DatabaseClientReport } from "@mixengine/api";
import ContextMenu from "../../../../components/ContextMenu";
import { copyText } from "../../../../core/clipboard";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import { CopyIcon, DatabaseGenericIcon, LockIcon, LogIcon, PowerIcon, ReloadIcon } from "../../../../icons";
import * as api from "../../api";
import type { ServiceRow } from "../../daemonState";
import { DATABASE_MODULE_ID, openChoices, opensADatabase } from "../ServicesDetail/openChoices";

interface Props {
  /** The row whose menu is open, and where; `null` when none is. */
  menu: { id: string; x: number; y: number } | null;
  rows: ServiceRow[];
  databases: Record<string, DatabaseClientReport>;
  isModuleVisible: (moduleId: string) => boolean;
  onClose: () => void;
  onViewLogs: (serviceId: string) => void;
  setAutostart: (id: string, autostart: boolean) => Promise<void>;
  showCredentials: (id: string) => Promise<void>;
  setResetTarget: (id: string | null) => void;
  setError: (message: string) => void;
}

/** A service row's ⋮ menu. */
export default function ServiceRowMenu({
  menu,
  rows,
  databases,
  isModuleVisible,
  onClose,
  onViewLogs,
  setAutostart,
  showCredentials,
  setResetTarget,
  setError,
}: Props) {
  const { t } = useTranslation();
  /** The row whose menu is open. The menu only lives with one `menu.id`, but the table updates from
   *  the stream, so read it again from `rows` instead of snapshotting the row when it opened — the
   *  autostart label has to follow the column next to it. */
  const menuRow = menu === null ? undefined : rows.find((row) => row.id === menu.id);

  /** The answer for the row whose menu is open, bound to a name: reading `databases[menu.id]` twice
   *  makes TypeScript lose the type narrowing on `client`.
   *
   *  `opensADatabase` filters right here rather than at the ⋮ button: `database.client` also
   *  answers for nginx and for a php-fpm pool, just with `protocol: null`, so being present in
   *  `databases` does not mean there is anything to open. */
  const menuReport =
    menuRow === undefined ? undefined : databases[menuRow.id];
  const menuDatabase =
    menuReport !== undefined && opensADatabase(menuReport) ? menuReport : undefined;

  // This menu is never empty: autostart and logs are there for *every* service.
  if (menu === null) return null;
  return (
    <ContextMenu x={menu.x} y={menu.y} onClose={onClose}>
      <button
        type="button"
        onClick={() => {
          const id = menu.id;
          onClose();
          onViewLogs(id);
        }}
      >
        <LogIcon size={14} />
        {t("mixengine.dashboard.viewLogs")}
      </button>

      {menuRow !== undefined && (
        <button
          type="button"
          disabled={menuRow.port === null}
          onClick={() => {
            const port = menuRow.port;
            onClose();
            if (port !== null) void copyText(String(port));
          }}
        >
          <CopyIcon size={14} />
          {t("mixengine.dashboard.copyPort")}
        </button>
      )}

      {/* The label flips with that row's own Autostart column, rather than being a tick. */}
      {menuRow !== undefined && (
        <button
          type="button"
          onClick={() => {
            const id = menuRow.id;
            const wanted = !menuRow.autostart;
            onClose();
            void setAutostart(id, wanted);
          }}
        >
          <PowerIcon size={14} />
          {t(
            menuRow.autostart
              ? "mixengine.dashboard.autostartOff"
              : "mixengine.dashboard.autostartOn",
          )}
        </button>
      )}

      {menuDatabase !== undefined && (
        <>
          <div className="context-menu-separator" />
          {openChoices(menuDatabase.client, isModuleVisible(DATABASE_MODULE_ID)).map((choice) => (
            <button
              key={choice}
              type="button"
              onClick={() => {
                const id = menu.id;
                onClose();
                // A `db` tab in this window; the password travels neither way through here.
                void api.databaseExploreData(id).catch((e: unknown) => setError(errorMessage(t, e)));
              }}
            >
              <DatabaseGenericIcon size={14} />
              {t(
                choice === "builtIn"
                  ? "mixengine.dashboard.exploreData"
                  : "mixengine.dashboard.exploreDataEnabling",
              )}
            </button>
          ))}

          <button
            type="button"
            onClick={() => {
              const id = menu.id;
              onClose();
              void showCredentials(id);
            }}
          >
            <LockIcon size={14} />
            {t("mixengine.dashboard.credentials")}
          </button>

          {/* The ellipsis is a promise: clicking opens a question, it does not run right
              away. */}
          <button
            type="button"
            onClick={() => {
              const id = menu.id;
              onClose();
              setResetTarget(id);
            }}
          >
            <ReloadIcon size={14} />
            {t("mixengine.dashboard.resetCredential")}
          </button>
        </>
      )}
    </ContextMenu>
  );
}
