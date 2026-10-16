import { invoke } from "@tauri-apps/api/core";
import type {
  SqlCollation,
  SqlColumnSpec,
  SqlIndexSpec,
  SqlProblem,
  SqlSchemaOutline,
  SqlSkipIndexSpec,
  SqlStatementResult,
  SqlTablePage,
  SqlTableStructure,
  TableStats,
} from "../types";
import type { SqlFilter } from "./filters";

/** What the header shows about the server it is connected to. */
export interface SqlServerInfo {
  version: string;
  os: string;
}

/**
 * Which page of a table to read. `sortColumn` orders the whole table before the page is cut out of
 * it, so paging through a sorted table stays in that order; passing null leaves the rows in
 * whatever order the server returns them. `filters` narrows the table down before either happens,
 * ANDed together — the page's `total` counts what is left after them.
 */
export interface SqlPageQuery {
  page: number;
  pageSize: number;
  /** Ignored unless it names a real column of the table. */
  sortColumn?: string | null;
  sortDesc?: boolean;
  filters?: SqlFilter[];
}

/** What of a database a dump carries. */
export type SqlDumpMode = "structure" | "data" | "all";

/**
 * Everything the workspace asks of a SQL server, with one implementation per engine.
 *
 * The workspace and every component under it hold this rather than a set of imported functions, so
 * that what differs between MySQL and PostgreSQL — which command each call reaches, and how the
 * server is asked — is decided once, where the connection is opened, instead of at each call site.
 * Obtained through {@link useSqlApi}; see `src/sql/context.tsx`.
 *
 * Every method takes the connection id first, the way the underlying commands do.
 */
export interface SqlApi {
  listDatabases(id: string): Promise<string[]>;
  listTables(id: string, database: string): Promise<string[]>;
  serverInfo(id: string): Promise<SqlServerInfo>;

  /**
   * What every table in the database weighs, ordered by name. Read from the server's own catalogue,
   * so it costs nothing whatever the tables hold — at the price of the row counts being the
   * estimates that live there rather than exact counts, and the average row sizes following them.
   *
   * Views are left out: one stores nothing of its own, and would read here as an empty table.
   */
  tableStats(id: string, database: string): Promise<TableStats[]>;

  tableData(
    id: string,
    database: string,
    table: string,
    query: SqlPageQuery
  ): Promise<SqlTablePage>;

  updateRow(
    id: string,
    database: string,
    table: string,
    updates: Record<string, string | null>,
    key: Record<string, string | null>
  ): Promise<void>;

  /**
   * Inserts one or more rows in a single transaction — one rejected row means none of them land.
   * Each entry maps a column to the text to write, or to null for an explicit SQL NULL; a column
   * left out of the map is left out of that row's INSERT, so the table's own default fills it.
   */
  insertRows(
    id: string,
    database: string,
    table: string,
    rows: Record<string, string | null>[]
  ): Promise<void>;

  /**
   * Deletes the rows matched by `keys` — or, with `all`, every row in the table regardless of what
   * `keys` holds. `resetAutoIncrement` puts the table's generated-key counter back to 1 afterwards.
   */
  deleteRows(
    id: string,
    database: string,
    table: string,
    keys: Record<string, string | null>[],
    all: boolean,
    resetAutoIncrement: boolean
  ): Promise<void>;

  /** Reads what the table is made of: its columns in table order, and its indexes with the primary
   *  key first. Only what the connected user has privileges to see is reported. */
  tableStructure(id: string, database: string, table: string): Promise<SqlTableStructure>;

  /**
   * Every table and column of one database, for the Query tab's completion.
   *
   * One call covers the whole database — unlike {@link tableStructure}, which answers for a single
   * table and in the detail the Structure tab needs. Only what the connected user has privileges to
   * see is in it, so a missing table means "not visible to you", not "not there".
   */
  schemaOutline(id: string, database: string): Promise<SqlSchemaOutline>;

  /** Every collation this server supports. A property of the server and not of a table, so one read
   *  per connection covers every column editor opened on it. */
  collations(id: string): Promise<SqlCollation[]>;

  /**
   * Writes the database to `path` as SQL, by running the engine's own dump tool against the same
   * server this connection is on (through the same SSH tunnel, when there is one).
   *
   * The file carries no `CREATE DATABASE` or `USE`, only the tables — so it restores into whichever
   * database it is pointed at rather than insisting on the one it came from.
   */
  dump(id: string, database: string, mode: SqlDumpMode, path: string): Promise<void>;

  /** Replays a SQL file through the engine's own client, into `database` — which is where a dump
   *  written here lands, since such a dump names no database of its own. */
  restore(id: string, database: string, path: string): Promise<void>;

  /** Drops a database and every table in it. */
  dropDatabase(id: string, database: string): Promise<void>;

  /** Creates a database. `collation` is its default collation, or null to inherit the server's. */
  createDatabase(id: string, name: string, collation: string | null): Promise<void>;

  /**
   * Creates an empty table, with a generated integer primary key for its one column — every other
   * column is added from the Structure tab afterwards.
   *
   * `collation` names the table's default collation, or null to inherit the database's.
   * `engine` is the ClickHouse table engine, and null everywhere else: the other three have one
   * storage engine's worth of choice at table level and take no such argument. It is the one
   * property of a ClickHouse table that cannot be changed afterwards.
   */
  createTable(
    id: string,
    database: string,
    table: string,
    collation: string | null,
    engine: string | null
  ): Promise<void>;

  /** Renames a table within its database. Atomic: nothing ever sees both names, or neither. */
  renameTable(id: string, database: string, table: string, newName: string): Promise<void>;

  /** Drops a table and every row in it. */
  dropTable(id: string, database: string, table: string): Promise<void>;

  addColumn(id: string, database: string, table: string, spec: SqlColumnSpec): Promise<void>;

  /** Redefines the column currently called `name` — the spec's own name is what it will be called
   *  afterwards, so this is also how a column is renamed. */
  modifyColumn(
    id: string,
    database: string,
    table: string,
    name: string,
    spec: SqlColumnSpec
  ): Promise<void>;

  dropColumn(id: string, database: string, table: string, name: string): Promise<void>;

  addIndex(id: string, database: string, table: string, spec: SqlIndexSpec): Promise<void>;

  /** Replaces the index called `name`, without the table ever spending any time without it. */
  modifyIndex(
    id: string,
    database: string,
    table: string,
    name: string,
    spec: SqlIndexSpec
  ): Promise<void>;

  dropIndex(id: string, database: string, table: string, name: string): Promise<void>;

  addSkipIndex(id: string, database: string, table: string, spec: SqlSkipIndexSpec): Promise<void>;

  /** Replaces the skip index called `name` — ClickHouse has no `MODIFY INDEX`, so this is a drop
   *  and a re-add on the backend. */
  modifySkipIndex(
    id: string,
    database: string,
    table: string,
    name: string,
    spec: SqlSkipIndexSpec
  ): Promise<void>;

  dropSkipIndex(id: string, database: string, table: string, name: string): Promise<void>;

  /** Rebuilds the whole table with a new sorting key — see
   *  `docs/specs/2026-09-04-clickhouse-index-ddl-design.md`. Resolves to the name of a
   *  leftover temporary table when the swap itself succeeded but its own cleanup did not — not a
   *  failure of the rebuild, which has already landed — and to `null` when nothing was left behind. */
  rebuildOrderBy(
    id: string,
    database: string,
    table: string,
    columns: string[]
  ): Promise<string | null>;

  /** A cheap `count()`-equivalent, read before a rebuild so its warning can say how much data it is
   *  about to copy. */
  rowCount(id: string, database: string, table: string): Promise<number>;

  /**
   * Runs user-authored SQL, statement by statement, and reports each statement's outcome.
   * Everything runs on one connection, so a `SET` or a transaction from one statement still holds
   * for the next; a statement that fails stops the script, and the results before it still come
   * back.
   *
   * `database` is what unqualified table names resolve against, the way they do everywhere else in
   * the workspace.
   *
   * `runId` names this run and nothing else, and is what {@link cancelQuery} is given to stop it.
   * A connection is not enough to name it by: one connection can be running two scripts, and then
   * a cancel aimed at either would reach whichever started last.
   */
  runScript(
    id: string,
    runId: string,
    sql: string,
    database?: string,
  ): Promise<SqlStatementResult[]>;

  /**
   * Stops the script this connection is running, by asking the server to kill the statement in
   * flight while leaving the session and everything it has set up intact.
   *
   * The killed statement comes back through {@link runScript} as a failed one, carrying the
   * server's own words — so the results of the statements before it are still shown. Cancelling
   * a run that has already finished — or one that never started — does nothing and reports no
   * error: the button is pressed while the results are on their way back often enough.
   */
  cancelQuery(id: string, runId: string): Promise<void>;

  /**
   * Asks the server what it makes of one statement, without running it.
   *
   * The statement is prepared and the plan immediately thrown away, so nothing it says would happen
   * happens — this is safe to call on a half-typed `DELETE`. `null` means the server had nothing to
   * say about it.
   *
   * The check runs on its own pooled connection, not on the session a script runs on. So anything
   * the script itself would have set up first — a temporary table, a `SET` — is invisible here,
   * which is why almost everything that comes back is a `warning`: only the server refusing to
   * parse the text is certain.
   */
  validateSql(id: string, sql: string, database?: string): Promise<SqlProblem | null>;
}

/**
 * Every SQL engine's side of {@link SqlApi}: one `sql_*` command each, and nothing else (ADR 0062).
 *
 * The backend finds the engine from the connection, so nothing here names one. What tells the
 * engines apart in the window is the dialect: it decides which controls exist, and so which of
 * these an engine is ever asked for. One it does not have answers `error.unsupportedOperation`.
 */
export const sqlApi: SqlApi = {
  listDatabases(id) {
    return invoke<string[]>("sql_list_databases", { id });
  },

  listTables(id, database) {
    return invoke<string[]>("sql_list_tables", { id, database });
  },

  serverInfo(id) {
    return invoke<SqlServerInfo>("sql_server_info", { id });
  },

  tableStats(id, database) {
    return invoke<TableStats[]>("sql_table_stats", { id, database });
  },

  tableData(id, database, table, query: SqlPageQuery) {
    return invoke<SqlTablePage>("sql_table_data", { id, database, table, query });
  },

  updateRow(id, database, table, updates, key) {
    return invoke<void>("sql_update_row", { id, database, table, updates, key });
  },

  insertRows(id, database, table, rows) {
    return invoke<void>("sql_insert_rows", { id, database, table, rows });
  },

  deleteRows(id, database, table, keys, all, resetAutoIncrement) {
    return invoke<void>("sql_delete_rows", {
      id,
      database,
      table,
      keys,
      all,
      resetAutoIncrement,
    });
  },

  tableStructure(id, database, table) {
    return invoke<SqlTableStructure>("sql_table_structure", { id, database, table });
  },

  schemaOutline(id, database) {
    return invoke<SqlSchemaOutline>("sql_schema_outline", { id, database });
  },

  collations(id) {
    return invoke<SqlCollation[]>("sql_collations", { id });
  },

  dump(id, database, mode: SqlDumpMode, path) {
    return invoke<void>("sql_dump", { id, database, mode, path });
  },

  restore(id, database, path) {
    return invoke<void>("sql_restore", { id, database, path });
  },

  dropDatabase(id, database) {
    return invoke<void>("sql_drop_database", { id, database });
  },

  createDatabase(id, name, collation) {
    return invoke<void>("sql_create_database", { id, name, collation });
  },

  createTable(id, database, table, collation, engine) {
    return invoke<void>("sql_create_table", { id, database, table, collation, engine });
  },

  renameTable(id, database, table, newName) {
    return invoke<void>("sql_rename_table", { id, database, table, newName });
  },

  dropTable(id, database, table) {
    return invoke<void>("sql_drop_table", { id, database, table });
  },

  addColumn(id, database, table, spec: SqlColumnSpec) {
    return invoke<void>("sql_add_column", { id, database, table, spec });
  },

  modifyColumn(id, database, table, name, spec: SqlColumnSpec) {
    return invoke<void>("sql_modify_column", { id, database, table, name, spec });
  },

  dropColumn(id, database, table, name) {
    return invoke<void>("sql_drop_column", { id, database, table, name });
  },

  addIndex(id, database, table, spec: SqlIndexSpec) {
    return invoke<void>("sql_add_index", { id, database, table, spec });
  },

  modifyIndex(id, database, table, name, spec: SqlIndexSpec) {
    return invoke<void>("sql_modify_index", { id, database, table, name, spec });
  },

  dropIndex(id, database, table, name) {
    return invoke<void>("sql_drop_index", { id, database, table, name });
  },

  runScript(id, runId, sql, database) {
    return invoke<SqlStatementResult[]>("sql_run_script", { id, runId, sql, database });
  },

  cancelQuery(id, runId) {
    return invoke<void>("sql_cancel_query", { id, runId });
  },

  validateSql(id, sql, database) {
    return invoke<SqlProblem | null>("sql_validate_sql", { id, sql, database });
  },

  addSkipIndex(id, database, table, spec) {
    return invoke<void>("sql_add_skip_index", { id, database, table, spec });
  },

  modifySkipIndex(id, database, table, name, spec) {
    return invoke<void>("sql_modify_skip_index", { id, database, table, name, spec });
  },

  dropSkipIndex(id, database, table, name) {
    return invoke<void>("sql_drop_skip_index", { id, database, table, name });
  },

  rebuildOrderBy(id, database, table, columns) {
    return invoke<string | null>("sql_rebuild_order_by", { id, database, table, columns });
  },

  rowCount(id, database, table) {
    return invoke<number>("sql_row_count", { id, database, table });
  },
};

/**
 * Creates an empty database file at `path`, for the connection form's New button.
 *
 * Apart from {@link sqlApi} because it is not part of {@link SqlApi}: every call there acts on
 * an open connection, and this one runs before there is anything to be connected to.
 *
 * Refuses a path that already holds a file. The save dialog will have asked about replacing one,
 * and a yes there must not reach the backend as "delete that database" — see `create_file`.
 */
export function createSqliteFile(path: string): Promise<void> {
  return invoke<void>("sqlite_create_file", { path });
}
