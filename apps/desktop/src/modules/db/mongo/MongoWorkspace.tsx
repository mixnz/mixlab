import { useCallback, useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { cancelTransfer, onTransferProgress, type TransferProgress } from "../transfer";
import {
  mongoCreateCollection,
  mongoDropCollection,
  mongoListCollections,
  mongoListDatabases,
  mongoRenameCollection,
  mongoServerInfo,
} from "./api";
import { isMongoSystemDatabase } from "./system";
import Select from "../../../components/Select";
import ConfirmDialog from "../../../components/ConfirmDialog";
import DatabaseActions from "../components/DatabaseActions";
import type { DatabaseChange } from "../components/DatabaseActions";
import DatabaseStats from "../components/DatabaseStats";
import type { StatsCache } from "../components/DatabaseStats";
import ServerInfoLabel from "../components/ServerInfoLabel";
import TransferOverlay from "../components/TransferOverlay";
import ErrorBanner from "../../../components/ErrorBanner";
import DisconnectTab from "../components/DisconnectTab";
import TunnelBanner from "../components/TunnelBanner";
import { useWorkspaceError } from "../workspaceError";
import Input from "../../../components/Input";
import NameDialog from "../../../components/NameDialog";
import NoSqlTable from "../components/NoSqlTable";
import type { DocumentCache, FilterCache } from "../components/NoSqlTable";
import ActionBar from "../../../components/ActionBar";
import ItemList from "../../../components/ItemList";
import type { ItemAction } from "../../../components/ItemList";
import itemListStyles from "../../../components/ItemList/ItemList.module.css";
import { PlusIcon, ReloadIcon } from "../../../icons";
import Splitter from "../../../components/Splitter";
import { useSidebarWidth } from "../sidebarWidth";
import { useSchemaTokens } from "../schemaTokens";
import { useSidebarKeyboard } from "../../../core/sidebarKeyboard";
import { useTranslation } from "../../../i18n";
import { errorMessage } from "../../../core/errors";

interface Props {
  /** Whether this connection's tab is the one on show. Passed straight through to the content
   *  panes, which take `Ctrl+R` for their own reload only while the user is looking at them. */
  active: boolean;
  connectionId: string;
  initialDatabase?: string;
  status: string;
  error: string;
  /**
   * This connection goes through an SSH tunnel.
   *
   * Only used to know who tells the story of a lost connection: with a tunnel, TunnelBanner tells
   * it and ErrorBanner stays quiet — see {@link useWorkspaceError}.
   */
  tunnelled: boolean;
  onDisconnect: () => void;
  sidebarWidth?: number;
  onSidebarWidthChange?: (width: number) => void;
  /**
   * The connection is marked read-only, so nothing here may write to the server.
   *
   * Everything that would change it is turned off: collections cannot be created, renamed or
   * dropped, the database tools are closed, and the documents in the Data tab neither open for
   * editing nor take an insert or a delete. What still works is everything that reads, which is
   * the point of the flag.
   */
  readOnly?: boolean;
}

/** The panes the content area can show: the selected collection's documents, or what every
 * collection in the database weighs. */
type ContentMode = "data" | "stats";

/** The tabs in the order they are shown, each with the key that names it. */
const CONTENT_TABS: { mode: ContentMode; labelKey: "mongo.dataTab" | "mongo.statsTab" }[] = [
  { mode: "data", labelKey: "mongo.dataTab" },
  { mode: "stats", labelKey: "mongo.statsTab" },
];

/** The database picker's first entry, which opens the create dialog instead of selecting anything.
 * MongoDB allows no `/` in a database name, so this can never collide with a real one. */
const NEW_DATABASE = "/new";

/** The picker's last entry, which re-reads the list instead of selecting anything. A database
 * created or dropped elsewhere is otherwise only picked up by reconnecting. */
const RELOAD_DATABASES = "/reload";

/** What MongoDB refuses in a database name, checked here because there is no server call to be
 * refused by: a database only reaches the server with its first collection. */
const INVALID_DATABASE_NAME = /[/\\. "$*<>:|?]/;

const DEFAULT_SIDEBAR_WIDTH = 260;
const MIN_SIDEBAR_WIDTH = 140;
const MAX_SIDEBAR_WIDTH = 480;

function MongoWorkspace({
  active,
  connectionId,
  initialDatabase,
  error,
  tunnelled,
  onDisconnect,
  sidebarWidth,
  onSidebarWidthChange,
  readOnly = false,
}: Props) {
  const { t } = useTranslation();
  const [databases, setDatabases] = useState<string[]>([]);
  const [databasesLoading, setDatabasesLoading] = useState(false);
  const [selectedDb, setSelectedDb] = useState(initialDatabase ?? "");
  const [collections, setCollections] = useState<string[]>([]);
  const [collectionsLoading, setCollectionsLoading] = useState(false);
  const [collectionFilter, setCollectionFilter] = useState("");
  const [selectedCollection, setSelectedCollection] = useState<string | null>(null);
  const [contentMode, setContentMode] = useState<ContentMode>("data");
  const [localError, setLocalError] = useWorkspaceError(tunnelled);
  const [serverInfo, setServerInfo] = useState<{ version: string; os: string } | null>(null);
  const [creatingDatabase, setCreatingDatabase] = useState(false);
  const [creatingCollection, setCreatingCollection] = useState(false);
  /** The collection the context menu's rename is open on, and the one its drop is asking about. */
  const [renamingCollection, setRenamingCollection] = useState<string | null>(null);
  const [droppingCollection, setDroppingCollection] = useState<string | null>(null);
  /** What the dump/restore tools are doing, if anything — shown over the whole workspace. */
  const [transferStatus, setTransferStatus] = useState("");
  /** How far the dump or restore behind that overlay has got — dropping a database has nothing to
   * report. Null until the first reading arrives, and again once the overlay goes. */
  const [transferProgress, setTransferProgress] = useState<TransferProgress | null>(null);

  /** The selection as `loadDatabases` needs to read it: through a ref, so reloading the list stays
   * one callback per connection instead of a new one on every change of database. */
  const selectedDbRef = useRef(selectedDb);
  selectedDbRef.current = selectedDb;

  /** The sidebar's search box and the list under it — see {@link useSidebarKeyboard}. */
  const sidebarKeys = useSidebarKeyboard(active, selectedDb);

  /** What each collection's filter bar was carrying when it was last left — a filter is often
   * typed out to look something up, and looking it up is exactly what sends the user off to
   * another collection or to the Stats tab. Kept out here because either move unmounts the
   * document list, and kept for as long as this connection's tab is open. */
  const filterCache = useRef<FilterCache>(new Map()).current;

  /** The page of documents each collection was last left showing, and the figures the Stats tab has
   * read for each database. Both live out here rather than inside the pane that reads them, so that
   * leaving a collection — or leaving the database it is in — is something to come back from rather
   * than something to be read all over again.
   *
   * Nothing in here expires on its own. What is shown is what was last read, and the reload each
   * pane carries (and `Ctrl+R`) is what says otherwise — a client that quietly re-read behind the
   * user's back would be the thing that made a slow server unusable. The one thing that is not
   * waited on is a change this app made itself: see {@link forgetCollection}. */
  const documentCache = useRef<DocumentCache>(new Map()).current;
  const statsCache = useRef<StatsCache>(new Map()).current;

  /* The counts, the caches they guard and everything that empties them — see `schemaTokens.ts`,
     where the same machinery serves the SQL workspace. Mongo keeps no shape for a completion list
     and no structure pane, so there is nothing here beyond the two caches. */
  const {
    schemaToken,
    statsToken,
    forget: forgetCollection,
    forgetDatabase,
    contentsChanged: documentsChanged,
  } = useSchemaTokens({
    database: selectedDb,
    selected: selectedCollection,
    /* The filter bar goes with the documents: its conditions name fields, and the documents under
       a name that has changed hands need carry no field by that name at all. */
    caches: [documentCache, filterCache],
  });

  const { width, splitter } = useSidebarWidth({
    saved: sidebarWidth,
    onChange: onSidebarWidthChange,
    defaultWidth: DEFAULT_SIDEBAR_WIDTH,
    minWidth: MIN_SIDEBAR_WIDTH,
    maxWidth: MAX_SIDEBAR_WIDTH,
    names: collections,
    itemClassName: itemListStyles.item,
  });

  /** Reads the database list. The selected database is kept in it even when the server doesn't
   * list it: MongoDB stores no empty database, so one created in the picker exists here alone
   * until its first collection is made, and a reload would otherwise take it away. */
  const loadDatabases = useCallback(async () => {
    setDatabasesLoading(true);
    try {
      const dbs = await mongoListDatabases(connectionId);
      const selected = selectedDbRef.current;
      setDatabases(selected && !dbs.includes(selected) ? [...dbs, selected] : dbs);
    } catch (e) {
      setLocalError(errorMessage(t, e));
    } finally {
      setDatabasesLoading(false);
    }
  }, [connectionId]);

  useEffect(() => {
    void loadDatabases();
  }, [loadDatabases]);

  // Only while the overlay is up, which is the only time there is anywhere to show a reading. The
  // listener is registered as the transfer begins rather than kept for the life of the tab: what it
  // is listening for happens a few times in a session and reports four times a second while it does.
  useEffect(() => {
    if (transferStatus === "") {
      setTransferProgress(null);
      return;
    }
    let stop: UnlistenFn | null = null;
    let cancelled = false;
    void onTransferProgress(connectionId, setTransferProgress).then((unlisten) => {
      // The transfer ended before the listener was in place; there is nothing left to hear.
      if (cancelled) void unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [transferStatus, connectionId]);

  useEffect(() => {
    let cancelled = false;
    setServerInfo(null);
    mongoServerInfo(connectionId)
      .then((info) => {
        if (!cancelled) setServerInfo(info);
      })
      .catch(() => {
        // Non-critical display info — silently omit it on failure.
      });
    return () => {
      cancelled = true;
    };
  }, [connectionId]);

  /* Which read of the collection list is the current one — see `tablesRun` in `SqlWorkspace`, the
     same race and the same answer. Both this effect and `listCollections` below start one, and
     either answer can land after the database has been changed underneath it. */
  const collectionsRun = useRef(0);

  useEffect(() => {
    setCollectionFilter("");
    const mine = ++collectionsRun.current;
    // Whatever the sidebar was reading is over, and its `finally` is no longer the current run's.
    setCollectionsLoading(false);
    if (!selectedDb) {
      setCollections([]);
      setSelectedCollection(null);
      return;
    }
    setSelectedCollection(null);
    mongoListCollections(connectionId, selectedDb)
      .then((list) => {
        if (collectionsRun.current === mine) setCollections(list);
      })
      .catch((e) => {
        if (collectionsRun.current === mine) setLocalError(errorMessage(t, e));
      });
    return () => {
      collectionsRun.current += 1;
    };
  }, [connectionId, selectedDb]);

  /** Reads the sidebar's list of collections again, and nothing else. What follows a collection
   * created, renamed or dropped: the list is out of date, but every other collection in the
   * database was read just as truthfully a moment ago, and the one that changed has already been
   * let go of by name. */
  const listCollections = useCallback(() => {
    if (!selectedDb) return;
    const mine = ++collectionsRun.current;
    setCollectionsLoading(true);
    mongoListCollections(connectionId, selectedDb)
      .then((list) => {
        // The database changed while this was out: the list belongs to the one the user left.
        if (collectionsRun.current === mine) setCollections(list);
      })
      .catch((e) => {
        if (collectionsRun.current === mine) setLocalError(errorMessage(t, e));
      })
      .finally(() => {
        if (collectionsRun.current === mine) setCollectionsLoading(false);
      });
  }, [connectionId, selectedDb]);

  /** The sidebar's reload button, and what a restore leaves behind: the list read again, and
   * everything remembered about the database let go with it. */
  const reloadCollections = useCallback(() => {
    forgetDatabase();
    listCollections();
  }, [forgetDatabase, listCollections]);

  /** What a restore or a drop of the whole database leaves to be caught up with: a restore has
   * replaced the collections under the list, and a drop has taken the database itself away. */
  async function databaseChanged(change: DatabaseChange) {
    if (change === "restored") {
      reloadCollections();
      return;
    }
    // The database itself has gone; nothing read from it is worth keeping, and a database made
    // again under the same name later is not the one these entries are about.
    forgetDatabase();
    setSelectedDb("");
    setSelectedCollection(null);
    setCollections([]);
    try {
      setDatabases(await mongoListDatabases(connectionId));
    } catch (e) {
      setLocalError(errorMessage(t, e));
    }
  }

  /**
   * Takes the new database's name and switches to it. Nothing is sent to the server: MongoDB has
   * no empty database, and only stores one once it holds a collection — which the sidebar's own
   * add button is what creates. Until then the name lives here alone, and a reconnect forgets it.
   */
  async function createDatabase(name: string) {
    if (INVALID_DATABASE_NAME.test(name)) {
      // Thrown as text rather than as an Error: what the dialog shows is whatever it catches, and
      // everything else it can catch is a message from the backend.
      throw t("mongo.databaseNameInvalid");
    }
    if (databases.includes(name)) throw t("mongo.databaseExists", { database: name });
    setCreatingDatabase(false);
    setDatabases((prev) => [...prev, name]);
    setSelectedDb(name);
    setSelectedCollection(null);
  }

  /** Creates the collection and leaves it selected, empty and ready to be inserted into. Errors
   * reject back into the dialog, which is what shows them and stays open. */
  async function createCollection(name: string) {
    await mongoCreateCollection(connectionId, selectedDb, name);
    setCreatingCollection(false);
    // Cleared so the new collection is visible whatever was being searched for when it was made.
    setCollectionFilter("");
    setSelectedCollection(name);
    // Under this name there may be an older collection of the same one, dropped earlier in the
    // session and remembered still; what was read for it is not what this one holds.
    forgetCollection(name);
    listCollections();
  }

  /** Renames the collection and follows it: whatever was open on it stays open, under the new
   * name. Errors reject back into the dialog, which is what shows them and stays open. */
  async function renameCollection(collection: string, newName: string) {
    await mongoRenameCollection(connectionId, selectedDb, collection, newName);
    setRenamingCollection(null);
    setCollectionFilter("");
    if (selectedCollection === collection) setSelectedCollection(newName);
    // Both ends of the move: the name it has left, and the name it has arrived at — which may have
    // been another collection's until it was dropped.
    forgetCollection(collection, newName);
    listCollections();
  }

  /** Drops the collection the confirmation was asking about. */
  async function dropCollection(collection: string) {
    setDroppingCollection(null);
    try {
      await mongoDropCollection(connectionId, selectedDb, collection);
      if (selectedCollection === collection) setSelectedCollection(null);
      forgetCollection(collection);
      listCollections();
    } catch (e) {
      setLocalError(errorMessage(t, e));
    }
  }

  /** A database the server keeps for itself: its collections are read here like any other, but
   * nothing in it may be created, renamed or dropped, nor anything done to it as a whole. */
  const systemDatabase = selectedDb !== "" && isMongoSystemDatabase(selectedDb);

  /** The two reasons this workspace refuses to change anything, and the one worth saying first.
   * Read-only is a decision someone made about the connection; a system database is a fact about
   * the server, and the one they are more likely to already know. */
  const noWrites = readOnly || systemDatabase;
  const noWritesHint = readOnly
    ? t("common.readOnlyConnection")
    : t("mongo.systemCollection", { database: selectedDb });

  const collectionActions: ItemAction[] = [
    {
      key: "rename",
      label: t("mongo.renameCollection"),
      disabled: noWrites,
      disabledHint: noWritesHint,
      onSelect: setRenamingCollection,
    },
    {
      key: "drop",
      label: t("mongo.dropCollection"),
      danger: true,
      disabled: noWrites,
      disabledHint: noWritesHint,
      onSelect: setDroppingCollection,
    },
  ];

  const filteredCollections = collectionFilter.trim()
    ? collections.filter((c) => c.toLowerCase().includes(collectionFilter.trim().toLowerCase()))
    : collections;

  const collectionsEmptyMessage =
    collections.length === 0
      ? t("mongo.noCollections")
      : filteredCollections.length === 0
        ? t("mongo.noMatchingCollections")
        : undefined;

  return (
    <div className="mongo-workspace">
      <div className="mongo-header">
        <div className="mongo-header-left">
          {serverInfo && (
            <ServerInfoLabel text={t("mongo.serverInfo", { os: serverInfo.os, version: serverInfo.version })} />
          )}
        </div>
        <label className="mongo-db-select">
          {t("mongo.databaseLabel")}{" "}
          <Select
            value={selectedDb}
            onChange={(db) => {
              if (db === NEW_DATABASE) {
                setCreatingDatabase(true);
                return;
              }
              if (db === RELOAD_DATABASES) {
                void loadDatabases();
                return;
              }
              setSelectedDb(db);
              setSelectedCollection(null);
            }}
            placeholder={t("mongo.databasePlaceholder")}
            size="normal"
            searchable
            searchPlaceholder={t("mongo.searchDatabasesPlaceholder")}
            options={[
              {
                value: NEW_DATABASE,
                label: t("mongo.createDatabase"),
                // Shown rather than hidden, so the picker offers the same thing wherever it is
                // opened and the missing entry is not read as a bug.
                disabled: readOnly,
                optionLabel: <span className="select-new-option">+ {t("mongo.createDatabase")}</span>,
              },
              ...databases.map((db) => ({ value: db, label: db })),
              {
                value: RELOAD_DATABASES,
                label: t("mongo.reloadDatabases"),
                // The menu stays open behind it: the reloaded list is the whole point of the
                // click, and closing would hide it until the picker is opened again.
                keepOpen: true,
                disabled: databasesLoading,
                optionLabel: (
                  <span className="select-reload-option">
                    <ReloadIcon
                      size="1em"
                      className={databasesLoading ? "select-reload-option-spinning" : undefined}
                    />
                    {t("mongo.reloadDatabases")}
                  </span>
                ),
              },
            ]}
          />
        </label>
        <div className="method-tabs mongo-content-tabs" role="tablist">
          {CONTENT_TABS.map(({ mode, labelKey }) => (
            <button
              key={mode}
              type="button"
              role="tab"
              aria-selected={contentMode === mode}
              className={`method-tab${contentMode === mode ? " method-tab-active" : ""}`}
              onClick={() => setContentMode(mode)}
            >
              {t(labelKey)}
            </button>
          ))}
          <DisconnectTab onDisconnect={onDisconnect} />
        </div>
      </div>

      <TunnelBanner connectionId={connectionId} onDisconnect={onDisconnect} />

      {(error || localError) && (
        <ErrorBanner message={error || localError} onDismiss={() => setLocalError("")} />
      )}

      <div className="mongo-body">
        <aside className="mongo-sidebar" style={{ flexBasis: width }}>
          <Input
            ref={sidebarKeys.searchRef}
            size="normal"
            className="mongo-sidebar-search"
            placeholder={t("mongo.searchCollectionsPlaceholder")}
            value={collectionFilter}
            onChange={(e) => setCollectionFilter(e.target.value)}
            onKeyDown={sidebarKeys.onSearchKeyDown}
          />
          <ItemList
            ref={sidebarKeys.listRef}
            className="db-sidebar-list"
            items={filteredCollections}
            selectedItem={selectedCollection}
            onSelect={setSelectedCollection}
            emptyMessage={collectionsEmptyMessage}
            actions={collectionActions}
            // The way back: `ArrowUp` off the top row is the search box again, caret and all.
            onLeaveTop={sidebarKeys.focusSearch}
          />
          <div className="mongo-sidebar-actions">
            <ActionBar
              actions={[
                {
                  key: "reload",
                  icon: ReloadIcon,
                  label: t("mongo.reloadCollections"),
                  disabled: !selectedDb || collectionsLoading,
                  busy: collectionsLoading,
                  onClick: reloadCollections,
                },
                {
                  key: "add",
                  icon: PlusIcon,
                  label: systemDatabase
                    ? t("mongo.addCollectionSystem", { database: selectedDb })
                    : t("mongo.addCollection"),
                  disabled: !selectedDb || collectionsLoading || noWrites,
                  // Only for read-only: the system-database case already says so in its label.
                  disabledHint: readOnly ? t("common.readOnlyConnection") : undefined,
                  onClick: () => setCreatingCollection(true),
                },
              ]}
            />
            {/* The database as a whole, kept at the far end: these act on everything the list
                above is showing rather than on anything in it. */}
            {/* Dump, restore and drop. A dump only reads, but restore and drop are here too and
                the component takes one `disabled` for all three — closing the lot is the right way
                round: a read-only connection losing its dump button is a nuisance, and keeping its
                restore button is the thing the flag was set to prevent. */}
            <DatabaseActions
              kind="mongo"
              connectionId={connectionId}
              database={selectedDb}
              disabled={collectionsLoading || readOnly}
              onError={setLocalError}
              onChanged={databaseChanged}
              onBusyChange={setTransferStatus}
            />
          </div>
        </aside>

        <Splitter
          orientation="vertical"
          ariaLabel={t("mongo.resizeSidebar")}
          title={t("mongo.resizeSidebarTooltip")}
          {...splitter}
        />

        {/* Compact only here, as on the SQL side: the header and sidebar keep the app's own size. */}
        <section className="mongo-content" data-density="compact">
          {contentMode === "data" && !selectedCollection && (
            <p className="muted">{t("mongo.selectCollectionPrompt")}</p>
          )}
          {/* Kept mounted while the Stats tab is up, and hidden rather than unmounted: the page of
              documents read, what has been typed into the cards and the conditions in the filter
              bar all outlive a look at what the database as a whole weighs. A collection picked
              while this is hidden costs nothing until the tab is looked at again. */}
          {selectedDb && selectedCollection && (
            <div className={contentMode === "data" ? "mongo-panel" : "mongo-panel-hidden"}>
              <NoSqlTable
                active={active && contentMode === "data"}
                connectionId={connectionId}
                selectedDb={selectedDb}
                selectedCollection={selectedCollection}
                onError={setLocalError}
                layoutWidth={width}
                filterCache={filterCache}
                documentCache={documentCache}
                schemaToken={schemaToken}
                onDocumentsChanged={documentsChanged}
                readOnly={readOnly}
              />
            </div>
          )}
          {contentMode === "stats" && !selectedDb && (
            <p className="muted">{t("mongo.selectDatabaseStatsPrompt")}</p>
          )}
          {/* Kept mounted while the document list is up, and hidden rather than unmounted: the
              figures it has read stay read, so coming back to the tab costs nothing. The same goes
              for coming back to a database, which the cache beside it is what makes free. */}
          {selectedDb && (
            <div className={contentMode === "stats" ? "mongo-panel" : "mongo-panel-hidden"}>
              <DatabaseStats
                kind="mongo"
                connectionId={connectionId}
                database={selectedDb}
                active={active && contentMode === "stats"}
                onError={setLocalError}
                statsCache={statsCache}
                schemaToken={statsToken}
              />
            </div>
          )}
        </section>
      </div>

      {transferStatus !== "" && (
        <TransferOverlay
          label={transferStatus}
          progress={transferProgress}
          onCancel={() => void cancelTransfer(connectionId)}
        />
      )}

      {creatingDatabase && (
        <NameDialog
          title={t("databaseDialog.title")}
          ariaLabel={t("databaseDialog.title")}
          label={t("databaseDialog.name")}
          emptyError={t("databaseDialog.errorName")}
          submitLabel={t("databaseDialog.submit")}
          savingLabel={t("databaseDialog.saving")}
          hint={t("mongo.createDatabaseHint")}
          onCancel={() => setCreatingDatabase(false)}
          onSubmit={createDatabase}
        />
      )}

      {creatingCollection && selectedDb && (
        <NameDialog
          title={t("collectionDialog.title", { database: selectedDb })}
          ariaLabel={selectedDb}
          label={t("collectionDialog.name")}
          emptyError={t("collectionDialog.errorName")}
          submitLabel={t("collectionDialog.submit")}
          savingLabel={t("collectionDialog.saving")}
          onCancel={() => setCreatingCollection(false)}
          onSubmit={createCollection}
        />
      )}

      {renamingCollection !== null && (
        <NameDialog
          title={t("mongo.renameCollectionTitle", { collection: renamingCollection })}
          ariaLabel={renamingCollection}
          label={t("renameDialog.name")}
          initialName={renamingCollection}
          emptyError={t("renameDialog.errorName")}
          submitLabel={t("renameDialog.submit")}
          savingLabel={t("renameDialog.saving")}
          onCancel={() => setRenamingCollection(null)}
          onSubmit={(newName) => renameCollection(renamingCollection, newName)}
        />
      )}

      {droppingCollection !== null && (
        <ConfirmDialog
          title={t("mongo.dropCollectionTitle")}
          message={t("mongo.dropCollectionMessage", { collection: droppingCollection })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => void dropCollection(droppingCollection)}
          onCancel={() => setDroppingCollection(null)}
        />
      )}
    </div>
  );
}

export default MongoWorkspace;
