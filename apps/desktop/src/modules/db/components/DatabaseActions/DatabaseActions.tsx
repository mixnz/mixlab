import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import ActionBar from "../../../../components/ActionBar";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import DumpDialog from "../DumpDialog";
import { DownloadIcon, TrashIcon, UploadIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { toolsDownloadable, toolsInstall, toolsReady, type ToolSuite } from "../../tools";
import { mongoDropDatabase, mongoDump, mongoRestore } from "../../mongo/api";
import { isMongoSystemDatabase } from "../../mongo/system";
import { useOptionalSql } from "../../sql/context";
import type { SqlContextValue } from "../../sql/context";
import type { SqlDumpMode } from "../../sql/api";

/** What the workspace is told happened, so it can reload what the change touched. */
export type DatabaseChange = "restored" | "dropped";

interface Props {
  kind: "mysql" | "postgres" | "mongo" | "sqlite" | "clickhouse" | "mssql";
  connectionId: string;
  /** The database the three actions act on; empty when none is selected, which disables them. */
  database: string;
  disabled?: boolean;
  /** Drop specifically. Dump and restore keep `disabled`: an engine can have its schema open while
   *  it still has no dump tool at all (ClickHouse). */
  schemaDisabled?: boolean;
  onError: (message: string) => void;
  onChanged: (change: DatabaseChange) => void;
  /** What is running, for the workspace's overlay — the empty string when nothing is. */
  onBusyChange: (label: string) => void;
}

/** Which action was interrupted by the tools not being there, to be resumed once they are. */
type Pending = "dump" | "restore";

/**
 * Dump, restore and drop, for the database as a whole — the right-hand end of the sidebar's
 * action bar, and every dialog those three need.
 *
 * Dumping and restoring are done by the database's own command-line tools rather than by this app
 * (see the Rust side for why), so each action first makes sure they are there and offers to fetch
 * them when they are not.
 */
function DatabaseActions({
  kind,
  connectionId,
  database,
  disabled,
  schemaDisabled,
  onError,
  onChanged,
  onBusyChange,
}: Props) {
  const { t } = useTranslation();
  const optionalSql = useOptionalSql();
  const [choosingMode, setChoosingMode] = useState(false);
  const [installFor, setInstallFor] = useState<Pending | null>(null);
  const [restoreFrom, setRestoreFrom] = useState<string | null>(null);
  const [dropping, setDropping] = useState(false);
  const [running, setRunning] = useState(false);

  /** The command-line tools this kind's dump and restore run through, or `null` for one that needs
   *  none. One suite per kind, named the same — see `ToolSuite`.
   *
   *  SQLite is the `null`: there is no `sqlitedump` to go and fetch, and the format is simple
   *  enough that MixLab writes the SQL itself. Everything below that would install or check for
   *  tools is skipped for it rather than asking about a download that does not exist.
   *
   *  ClickHouse is `null` too, for the same shape of reason: dump/restore runs entirely over the
   *  HTTP interface every other ClickHouse call already uses — see
   *  `docs/specs/2026-09-04-clickhouse-dump-restore-design.md` — so there is no tool
   *  suite to name, not because the buttons are closed.
   *
   *  SQL Server is `null` on the same grounds: Microsoft ships no free, pinnable equivalent of
   *  `pg_dump`/`mysqldump`, so the dump is written against the driver instead — see
   *  `docs/specs/2026-09-05-mssql-support-design.md`'s D10. */
  const suite: ToolSuite | null =
    kind === "sqlite" || kind === "clickhouse" || kind === "mssql" ? null : kind;

  /** Whether databases are objects on a server that can be created and dropped.
   *
   *  False for SQLite, where a database is a file: dropping one is deleting a file, which is the
   *  operating system's job and not a button in a database tool that has the file open. */
  const serverDatabases = kind !== "sqlite";

  /** The SQL workspace this is rendered in. Every caller sits on a branch `kind` has already
   *  settled, which is what makes the connection certain to be there — see {@link useOptionalSql}. */
  function sql(): SqlContextValue {
    if (optionalSql === null) throw new Error(`DatabaseActions: no SQL connection for "${kind}"`);
    return optionalSql;
  }

  /** A database the server keeps for itself: none of the three has anything sensible to do to one,
   * and dropping it would break the server, so all three are switched off over it. */
  const system =
    database !== "" &&
    (kind === "mongo"
      ? isMongoSystemDatabase(database)
      : (optionalSql?.dialect.isSystemDatabase(database) ?? false));
  const busy = disabled || running || database === "" || system;
  /** The same for the Drop button, which is gated on the schema rather than on dump/restore. */
  const dropBusy = schemaDisabled || running || database === "" || system;

  /** The button's tooltip, replaced over a system database by what is stopping it. */
  function label(key: "dump.dump" | "dump.restore" | "dump.drop") {
    return system ? t("dump.systemDatabase", { database }) : t(key);
  }

  /** Runs one tool with the overlay up, and reports whatever it says went wrong. */
  async function withBusy(label: string, work: () => Promise<void>): Promise<boolean> {
    setRunning(true);
    onBusyChange(label);
    try {
      await work();
      return true;
    } catch (e) {
      onError(errorMessage(t, e));
      return false;
    } finally {
      setRunning(false);
      onBusyChange("");
    }
  }

  /** Whether the tools are there. When they are not, asks about downloading them and remembers
   * what the user was trying to do, so the answer can carry it on — unless this platform has
   * nothing to download, in which case it says where the tools have to come from instead of
   * offering a button that could only fail. */
  async function toolsPresent(pending: Pending): Promise<boolean> {
    if (suite === null) return true;
    try {
      if (await toolsReady(suite)) return true;
      if (!(await toolsDownloadable(suite))) {
        // Which packages to install differs by engine, so the message does too.
        onError(t(kind === "postgres" ? "dump.noDownloadPostgres" : "dump.noDownload"));
        return false;
      }
    } catch (e) {
      onError(errorMessage(t, e));
      return false;
    }
    setInstallFor(pending);
    return false;
  }

  async function startDump() {
    if (!(await toolsPresent("dump"))) return;
    // Only a SQL dump has anything to decide; a mongodump archive is the whole database or nothing.
    if (kind !== "mongo") {
      setChoosingMode(true);
    } else {
      await runDump("all");
    }
  }

  async function runDump(mode: SqlDumpMode) {
    setChoosingMode(false);
    const stamp = new Date().toISOString().slice(0, 16).replace(/[-:T]/g, "");
    const extension = kind === "mongo" ? "archive" : "sql";
    const path = await save({
      defaultPath: `${database}-${stamp}.${extension}`,
      filters: [
        {
          name: t(kind === "mongo" ? "dump.archiveFilter" : "dump.sqlFilter"),
          extensions: [extension],
        },
      ],
    });
    // The picker was dismissed: nothing to report, and nothing to run.
    if (typeof path !== "string") return;
    await withBusy(t("dump.dumping", { database }), () =>
      kind === "mongo"
        ? mongoDump(connectionId, database, path)
        : sql().api.dump(connectionId, database, mode, path),
    );
  }

  async function startRestore() {
    if (!(await toolsPresent("restore"))) return;
    const extension = kind === "mongo" ? "archive" : "sql";
    const path = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: t(kind === "mongo" ? "dump.archiveFilter" : "dump.sqlFilter"),
          extensions: [extension],
        },
      ],
    });
    if (typeof path !== "string") return;
    setRestoreFrom(path);
  }

  async function runRestore(path: string) {
    setRestoreFrom(null);
    const ok = await withBusy(t("dump.restoring", { database }), () =>
      kind === "mongo"
        ? mongoRestore(connectionId, database, path)
        : sql().api.restore(connectionId, database, path),
    );
    // Even a failed restore may have got part of the way through, so the lists are stale either
    // way and are reloaded regardless.
    onChanged("restored");
    return ok;
  }

  async function install() {
    const pending = installFor;
    setInstallFor(null);
    if (suite === null) return;
    const ok = await withBusy(t("dump.installing"), () => toolsInstall(suite));
    if (!ok) return;
    if (pending === "dump") await startDump();
    if (pending === "restore") await startRestore();
  }

  async function drop() {
    setDropping(false);
    const ok = await withBusy(t("dump.dropping", { database }), () =>
      kind === "mongo"
        ? mongoDropDatabase(connectionId, database)
        : sql().api.dropDatabase(connectionId, database),
    );
    if (ok) onChanged("dropped");
  }

  return (
    <>
      <ActionBar
        actions={[
          {
            key: "dump",
            icon: DownloadIcon,
            label: label("dump.dump"),
            disabled: busy,
            onClick: () => void startDump(),
          },
          {
            key: "restore",
            icon: UploadIcon,
            label: label("dump.restore"),
            disabled: busy,
            onClick: () => void startRestore(),
          },
          /* Absent rather than disabled where a database is a file: the button would not be
             "temporarily unavailable", it would be an offer to delete a file, which this is not
             the tool for. */
          ...(serverDatabases
            ? [
                {
                  key: "drop",
                  icon: TrashIcon,
                  label: label("dump.drop"),
                  danger: true,
                  disabled: dropBusy,
                  onClick: () => setDropping(true),
                },
              ]
            : []),
        ]}
      />

      {choosingMode && (
        <DumpDialog
          database={database}
          onCancel={() => setChoosingMode(false)}
          onSubmit={(mode) => void runDump(mode)}
        />
      )}

      {installFor !== null && (
        <ConfirmDialog
          title={t("dump.installTitle")}
          message={t(
            kind === "mongo"
              ? "dump.installMongo"
              : kind === "postgres"
                ? "dump.installPostgres"
                : "dump.installMysql",
          )}
          confirmLabel={t("dump.installConfirm")}
          onConfirm={() => void install()}
          onCancel={() => setInstallFor(null)}
        />
      )}

      {restoreFrom !== null && (
        <ConfirmDialog
          title={t("dump.restoreTitle", { database })}
          message={t(kind === "mongo" ? "dump.restoreMongo" : "dump.restoreMysql", {
            file: restoreFrom,
            database,
          })}
          confirmLabel={t("dump.restoreConfirm")}
          danger
          onConfirm={() => void runRestore(restoreFrom)}
          onCancel={() => setRestoreFrom(null)}
        />
      )}

      {dropping && (
        <ConfirmDialog
          title={t("dump.dropTitle")}
          message={t(kind === "mongo" ? "dump.dropMongoMessage" : "dump.dropMysqlMessage", {
            database,
          })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => void drop()}
          onCancel={() => setDropping(false)}
        />
      )}
    </>
  );
}

export default DatabaseActions;
