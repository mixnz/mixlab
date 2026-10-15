import { useTranslation } from "../../../i18n";
import { kindLabel } from "../connectionForm";
import { SQL_ENGINES, isSqlKind } from "../engines";
import { DatabaseIcon } from "../icons";
import MongoWorkspace from "../mongo/MongoWorkspace";
import RedisWorkspace from "../redis/RedisWorkspace";
import { updateConnection } from "../savedConnectionsStore";
import SqlWorkspace from "../sql/SqlWorkspace";
import { SqlProvider } from "../sql/context";
import type { DbKind, SavedConnection } from "../types";
import { mongoUriDatabase } from "./mongoUri";

interface Props {
  active: boolean;
  kind: DbKind;
  connectionId: string;
  /** The form's database, what a SQL or Redis workspace opens on. */
  database: string;
  /** The form's Mongo connection string, whose path is the database a Mongo workspace opens on. */
  uri: string;
  tunnelled: boolean;
  status: string;
  error: string;
  /** The saved connection the tab was opened from, when there is one. */
  saved: SavedConnection | undefined;
  onDisconnect: () => void;
}

/** The workspace a connection of `kind` opens: one choice, made here and nowhere else. */
export default function ConnectionWorkspace({
  active,
  kind,
  connectionId,
  database,
  uri,
  tunnelled,
  status,
  error,
  saved,
  onDisconnect,
}: Props) {
  const { t } = useTranslation();

  async function updateSidebarWidth(width: number) {
    const entry = saved;
    if (!entry || entry.sidebarWidth === width) return;
    await updateConnection({ ...entry, sidebarWidth: width });
  }

  async function updateRedisScanLimit(limit: number) {
    const entry = saved;
    if (!entry || entry.redisScanLimit === limit) return;
    await updateConnection({ ...entry, redisScanLimit: limit });
  }

  if (isSqlKind(kind)) {
    const activeSavedConnection = saved;
    // The one place the engine behind a SQL workspace is chosen. Everything below reaches for it
    // through the context rather than importing an engine of its own — see `src/sql/context.tsx`.
    const engine = SQL_ENGINES[kind];
    return (
      <SqlProvider api={engine.api} dialect={engine.dialect}>
        <SqlWorkspace
          active={active}
          connectionId={connectionId}
          initialDatabase={database}
          status={status}
          error={error}
          tunnelled={tunnelled}
          onDisconnect={onDisconnect}
          sidebarWidth={activeSavedConnection?.sidebarWidth}
          onSidebarWidthChange={updateSidebarWidth}
          readOnly={(activeSavedConnection?.readOnly ?? false) || !engine.dialect.writable}
          schemaReadOnly={
            (activeSavedConnection?.readOnly ?? false) || !engine.dialect.ddlWritable
          }
          dataReadOnly={(activeSavedConnection?.readOnly ?? false) || !engine.dialect.rowsWritable}
          dumpRestoreReadOnly={
            (activeSavedConnection?.readOnly ?? false) || !engine.dialect.dumpRestoreWritable
          }
          dmlEvenIfReadOnly={
            !(activeSavedConnection?.readOnly ?? false) && engine.dialect.rowsWritable
          }
          profileId={activeSavedConnection?.id ?? ""}
        />
      </SqlProvider>
    );
  }

  if (kind === "mongo") {
    const activeSavedConnection = saved;
    return (
      <MongoWorkspace
        active={active}
        connectionId={connectionId}
        initialDatabase={mongoUriDatabase(uri)}
        status={status}
        error={error}
        tunnelled={tunnelled}
        onDisconnect={onDisconnect}
        sidebarWidth={activeSavedConnection?.sidebarWidth}
        onSidebarWidthChange={updateSidebarWidth}
        readOnly={activeSavedConnection?.readOnly ?? false}
      />
    );
  }

  if (kind === "redis") {
    const activeSavedConnection = saved;
    return (
      <RedisWorkspace
        active={active}
        connectionId={connectionId}
        initialDatabase={database}
        status={status}
        error={error}
        tunnelled={tunnelled}
        onDisconnect={onDisconnect}
        sidebarWidth={activeSavedConnection?.sidebarWidth}
        onSidebarWidthChange={updateSidebarWidth}
        scanLimit={activeSavedConnection?.redisScanLimit}
        onScanLimitChange={updateRedisScanLimit}
        readOnly={activeSavedConnection?.readOnly ?? false}
      />
    );
  }

  /* A kind with no workspace to open.
   *
   * Redis used to sit here as the unguarded `else`, which made it the workspace for every kind the
   * three branches above did not claim. That was invisible while they covered every kind, and
   * stopped being invisible the moment one did not: a SQL Server connection opened Redis's key
   * browser, which then asked the backend for Redis things and was told "This is not a Redis
   * connection". The kind reaching here is either one this build knows but cannot browse yet, or
   * one saved by a newer version — see
   * docs/specs/2026-09-05-unknown-db-kind-crash-and-error-logging-design.md, which
   * fixed the icon and the label on that path but not this one.
   *
   * Saying so is the whole of it. Rendering some other engine's workspace is worse than rendering
   * nothing, because it fails somewhere the user cannot connect back to what they picked.
   *
   * `engines.test.ts` mirrors this chain and asserts every kind is claimed by exactly one branch;
   * the two must agree, so a branch added or reordered here is a change there too. */
  return (
    <div className="workspace-unavailable">
      <DatabaseIcon kind={kind} size="2.5rem" className={`kind-${kind}`} />
      <p>{t("connection.workspaceUnavailable", { kind: t(kindLabel(kind)) })}</p>
      <button type="button" onClick={onDisconnect}>
        {t("common.disconnect")}
      </button>
    </div>
  );
}
