export type DbKind =
  | "mysql"
  | "postgres"
  | "mongo"
  | "redis"
  | "sqlite"
  | "clickhouse"
  | "mssql";

/* The SSH server is `core/ssh.ts`'s, not this module's: the backend has one `SshConfig` for both
   a tunnelled connection and a terminal session, and this was one of two mirrors of it. Re-exported
   rather than imported everywhere, so nothing that already says `from "./types"` has to change. */
import type { SshConfig } from "../../core/ssh";
export type { SshAuth, SshConfig } from "../../core/ssh";

export interface ConnectionConfig {
  kind: DbKind;
  host: string;
  port: number;
  username?: string;
  password?: string;
  database?: string;
  /** MongoDB only: a full `mongodb://` / `mongodb+srv://` connection string. It carries host,
   *  port, credentials and options in one value, so those fields are ignored for that kind. */
  uri?: string;
  /** SQLite only, and its whole address: the path of the database file on this machine. There is
   *  no server, so `host`, `port`, `username`, `password`, `ssh` and `use_ssl` are ignored for that
   *  kind — the same way `uri` above replaces them for MongoDB. */
  path?: string;
  ssh?: SshConfig;
  use_ssl?: boolean;
}

export interface SavedConnection {
  id: string;
  name: string;
  config: ConnectionConfig;
  sidebarWidth?: number;
  /** Redis only: how many keys the sidebar reads before it stops walking the keyspace. The right
   *  number is a property of the server rather than of the app — a cache with millions of keys
   *  and a small working set want different ceilings — so it is remembered per connection.
   *  Unset falls back to the workspace's own default. */
  redisScanLimit?: number;
  /** Kept at the head of the connection list. The few servers someone opens daily shouldn't have
   *  to be found alphabetically among every one they have ever saved. Absent means not pinned, so
   *  a list written before pinning existed reads correctly. */
  pinned?: boolean;
  /** Marks the connection as one nothing is to be written to. Everything that would change
   *  something is closed off — the Query tab refuses to send such a statement, and every button
   *  that would insert, edit, rename, drop or delete is greyed out — the point being the production
   *  server sitting one line above the staging one in the sidebar. It is a reminder, not a
   *  permission: the credential decides what the server actually allows, and this only decides what
   *  MixLab will send.
   *
   *  Offered for every kind, because a production Mongo or Redis server is as easy to mistake for
   *  its staging twin as a MySQL one is. */
  readOnly?: boolean;
  /** The key half of this connection's address in MixEngine's keyring entry — `service="mixengine"`
   *  is a constant on the Rust side that reads it and is never stored here (T84's D5); this is the
   *  `<service-id>/<user>` half, e.g. `"mariadb@main/root"`.
   *
   *  Safe to keep in `connections.json` in plain text, unlike a password: it is a name, not a
   *  secret — reading the credential it names still requires the OS credential store to hand it
   *  over. Set means Save must write this address instead of copying MixEngine's password into
   *  MixLab's own vault. The password is resolved from it only to connect
   *  (`withResolvedPassword`), never when the list is read, and reads as empty rather than an
   *  error when MixEngine no longer has that entry. */
  keyringRef?: string;
}

export const DEFAULT_PORTS: Record<DbKind, number> = {
  mysql: 3306,
  postgres: 5432,
  mongo: 27017,
  redis: 6379,
  /* SQLite has no port, and no server to have one. Zero rather than a number that looks plausible:
     the form hides the field for this kind, and a value that ever reaches the backend is ignored
     there. */
  sqlite: 0,
  clickhouse: 8123,
  mssql: 1433,
};

/** The row a foreign key column points at: what it references, not what it is declared as. */
export interface SqlForeignKey {
  table: string;
  column: string;
}

/** What the server knows about one column beyond its name — what a new row has to respect. */
export interface SqlColumnMeta {
  /** The declared type as MySQL spells it, e.g. `varchar(255)` or `int unsigned`. */
  dataType: string;
  nullable: boolean;
  /** The column's DEFAULT, or null when it has none. An expression default (`CURRENT_TIMESTAMP`,
   *  `(uuid())`) arrives here unquoted, indistinguishable from a literal on its own — `extra` is
   *  what tells the two apart. */
  defaultValue: string | null;
  /** `SHOW COLUMNS`' Extra: `auto_increment`, `DEFAULT_GENERATED`, `STORED GENERATED`, ... */
  extra: string;
  /** What this column references, or null when it is not part of a foreign key. */
  foreignKey: SqlForeignKey | null;
}

export interface SqlTablePage {
  columns: string[];
  /** Keyed by column name; `columns` is what carries their order. */
  columnMeta: Record<string, SqlColumnMeta>;
  primaryKey: string[];
  /** The AUTO_INCREMENT column, or null when the table has none — only such a table has a
   *  counter that resetting after a delete would mean anything for. */
  autoIncrementColumn: string | null;
  rows: Record<string, unknown>[];
  total: number;
}

/** One column as the table currently declares it — a row of the Structure tab's column grid. */
export interface SqlStructureColumn {
  name: string;
  /** The full declared type as MySQL spells it: `varchar(255)`, `int unsigned`, `enum('a','b')`. */
  dataType: string;
  nullable: boolean;
  /** The DEFAULT, or null when the column has none — which is also how `DEFAULT NULL` reads. */
  defaultValue: string | null;
  /** Whether the default above is an expression (`uuid()`) rather than a literal. */
  defaultIsExpression: boolean;
  autoIncrement: boolean;
  onUpdateCurrentTimestamp: boolean;
  /** A column MySQL computes from the others. Its expression is not read, so such a column can
   *  only be dropped here, never redefined. */
  generated: boolean;
  collation: string | null;
  comment: string;
  /** `PRI`, `UNI`, `MUL` or empty: which kind of key this column leads. */
  key: string;
  /** `SHOW COLUMNS`' Extra, verbatim — shown as-is so nothing unmodelled disappears. */
  extra: string;
}

/** One collation the connected server has. Read from the server, so the list matches that MySQL
 *  version exactly rather than whatever this client was built knowing about. */
export interface SqlCollation {
  name: string;
  charset: string;
  /** Whether this is its character set's default — what a column gets without a `COLLATE`. */
  isDefault: boolean;
}

/** What a column is to be declared as: the write-side counterpart of {@link SqlStructureColumn},
 *  carrying only the parts that go into an `ADD`/`CHANGE COLUMN` clause. */
export interface SqlColumnSpec {
  name: string;
  dataType: string;
  nullable: boolean;
  /** null writes no DEFAULT clause at all. */
  defaultValue: string | null;
  defaultIsExpression: boolean;
  autoIncrement: boolean;
  onUpdateCurrentTimestamp: boolean;
  collation: string | null;
  comment: string;
  /** Left out, the column stays where it is (or a new one is appended); `""` puts it first, and a
   *  column name puts it directly after that column. */
  after?: string;
}

export interface SqlIndexColumn {
  /** null for a functional index, which indexes an expression rather than a column. */
  name: string | null;
  /** How many leading characters are indexed, when only a prefix of the column is. */
  prefixLength: number | null;
}

export interface SqlTableIndex {
  name: string;
  unique: boolean;
  primary: boolean;
  /** `BTREE`, `HASH`, `FULLTEXT` or `SPATIAL` as MySQL reports it. */
  indexType: string;
  columns: SqlIndexColumn[];
  comment: string;
}

export type SqlIndexKind = "index" | "unique" | "fulltext" | "spatial" | "primary";

export interface SqlIndexColumnSpec {
  name: string;
  prefixLength: number | null;
}

export interface SqlIndexSpec {
  /** Left empty, MySQL names the index after its first column. Ignored for a primary key. */
  name: string;
  kind: SqlIndexKind;
  /** `BTREE`/`HASH`, or null for the engine's own default. Only meaningful for a plain or unique
   *  index — full-text and spatial indexes have one structure each. */
  indexType: string | null;
  columns: SqlIndexColumnSpec[];
  comment: string;
}

/** One data skipping index — ClickHouse's only secondary index. Not a lookup structure: an
 *  approximate filter that lets the server skip whole blocks a `WHERE` clause cannot match. See
 *  `docs/specs/2026-09-04-clickhouse-index-ddl-design.md`. */
export interface SqlSkipIndex {
  name: string;
  /** An expression, not necessarily a bare column name — `lower(note)` is as common as `note`. */
  expr: string;
  /** `minmax`, `set`, `bloom_filter`, `ngrambf_v1` or `tokenbf_v1`. */
  indexType: string;
  /** In the order the TYPE's own syntax takes them — always strings, since they mix integers and
   *  floats and are never computed on, only spliced back into SQL text. */
  args: string[];
  granularity: number;
}

export interface SqlSkipIndexSpec {
  name: string;
  expr: string;
  indexType: string;
  args: string[];
  granularity: number;
}

export interface SqlTableStructure {
  /** In table order, which is the order a `SELECT *` returns them in. */
  columns: SqlStructureColumn[];
  /** The primary key first, then the rest as the server listed them. */
  indexes: SqlTableIndex[];
  /** Always empty outside ClickHouse — see {@link SqlSkipIndex}. */
  skipIndexes: SqlSkipIndex[];
  /** The table's storage engine, or `null` outside ClickHouse. Used to guard the sorting-key
   *  rebuild against engines it was never verified against (`Replicated*`) — see the design doc's
   *  D11. */
  engine: string | null;
}

/** One column as the Query tab's completion knows it — the name, and enough beside it to tell two
 *  similar columns apart in the list. */
export interface SqlOutlineColumn {
  name: string;
  /** The declared type as MySQL spells it: `varchar(255)`, `int unsigned`. */
  dataType: string;
  nullable: boolean;
  /** `PRI`, `UNI`, `MUL` or empty: which kind of key this column leads. */
  key: string;
  /** `table.column` this one points at, when it is a foreign key. */
  references: string | null;
}

export interface SqlOutlineTable {
  name: string;
  /** In table order. Views are listed here too — their columns complete like any others'. */
  columns: SqlOutlineColumn[];
}

/** Every table and column of one database. Read once and cached, and only ever as much as the
 *  connected user has privileges to see. */
export interface SqlSchemaOutline {
  database: string;
  tables: SqlOutlineTable[];
}

/** How a statement's result is to be read: a result set, a count of rows changed, plain success,
 *  or the reason it failed. */
export type SqlStatementKind = "rows" | "affected" | "ok" | "error";

/** What one statement of the Query tab's script produced. */
export interface SqlStatementResult {
  /** The statement this came from, as the user wrote it. */
  statement: string;
  /** The keyword it opens with, upper-cased. */
  verb: string;
  kind: SqlStatementKind;
  columns: string[];
  /** Positional rather than keyed by column name: an arbitrary SELECT may name the same column
   *  twice, and only a positional row keeps the two apart. */
  rows: unknown[][];
  /** Set when the result set was longer than the client reads in one go. */
  truncated: boolean;
  rowsAffected: number;
  lastInsertId: number | null;
  durationMs: number;
  /** Set when the statement failed, in which case nothing after it ran. */
  error: string | null;
}

/** How much a validation finding is worth trusting. `error` is the server refusing to parse the
 *  text — certain. `warning` is everything else, which may only be wrong from where the check was
 *  standing: it runs on its own connection, so a temporary table or a `USE` from earlier in the
 *  script is invisible to it. */
export type SqlProblemSeverity = "error" | "warning";

/** What the server made of a statement it was asked to parse but not to run. */
export interface SqlProblem {
  /** The server's own words, untranslated. */
  message: string;
  /** MySQL's error number — 1064 is a syntax error, 1146 an unknown table. Zero when none came. */
  number: number;
  /** The 1-based line *within the statement* MySQL pointed at, when it pointed at one. */
  line: number | null;
  severity: SqlProblemSeverity;
}

export interface MongoCollectionPage {
  documents: Record<string, unknown>[];
  total: number;
}

/** What one table or collection weighs — a row of the Statistics tab. The same shape for both
 *  databases, so the one grid reads either without knowing which it is looking at. */
export interface TableStats {
  /** The table's or the collection's name. */
  name: string;
  /** Rows or documents. MySQL's InnoDB tables report an estimate rather than a `COUNT(*)`, so this
   *  can be well off on a large table — and so can {@link avgRecordSize}, derived from it. */
  rows: number;
  /** The bytes the rows or documents themselves take, indexes excluded. */
  dataSize: number;
  /** The bytes every index on it takes together. */
  indexSize: number;
  /** The average bytes of one row or document, as the server reports it — 0 when it holds none. */
  avgRecordSize: number;
}
