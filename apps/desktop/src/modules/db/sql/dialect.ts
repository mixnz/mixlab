import type { SqlColumnMeta, SqlIndexKind } from "../types";
import type { SQLDialect } from "@codemirror/lang-sql";
import type { SqlSyntax } from "./syntax";

/** One type the column editor offers, and what the box beside it holds — the argument that goes
 *  inside the type's parentheses. */
export interface SqlTypeSpec {
  name: string;
  /** What to suggest for the argument: `null` for a type that takes none (the box is then closed),
   *  and `""` for one that accepts an argument no column really needs to give. On a `required` type
   *  this is more than a suggestion — an empty box is declared with it. */
  arg: string | null;
  /** Not valid without an argument: `varchar` has no length of its own to fall back on, so an empty
   *  box takes `arg`. Except on a `list` type, where `arg` is only a sample and has to be filled. */
  required?: boolean;
  /** The argument is a list of values rather than a number, so it is not checked as one. */
  list?: boolean;
  /** UNSIGNED means something here. Only read where the engine has it at all. */
  numeric?: boolean;
}

/**
 * What the Structure tab's two dialogs offer, which is where the engines differ most.
 *
 * Every field is a clause one engine has and the other does not, so each of them is the answer to
 * "should this control be here at all". Kept together in one shape rather than spread across
 * {@link SqlDialect}, because they are read in one place each and always together.
 */
export interface SqlEditing {
  /** The types a column may be declared as, each family in the order it is usually reached for. */
  columnTypes: readonly SqlTypeSpec[];
  /** `UNSIGNED` exists — MySQL only. */
  unsigned: boolean;
  /** A column can be put somewhere in particular (`FIRST`, `AFTER x`). PostgreSQL appends, always,
   *  and has no statement that moves a column. */
  columnPosition: boolean;
  /** `ON UPDATE CURRENT_TIMESTAMP` exists — MySQL only. The same effect on PostgreSQL is a
   *  trigger, which is not a property of the column. */
  onUpdateCurrentTimestamp: boolean;
  /** The engine numbers a column itself — `AUTO_INCREMENT`, `GENERATED ... AS IDENTITY`,
   *  `AUTOINCREMENT`. False on ClickHouse, which has no counterpart at all, so the checkbox is
   *  absent there rather than disabled — the same way `onUpdateCurrentTimestamp` is elsewhere. */
  autoIncrement: boolean;
  /** A database carries a collation of its own — the default every table created in it inherits
   *  unless it says otherwise. False on PostgreSQL: there a database's collation is a locale of the
   *  host operating system rather than a name off a list, so `CREATE DATABASE`'s own `collation`
   *  parameter is accepted and ignored (see `postgres_ddl::create_database`). */
  databaseCollation: boolean;
  /** A table carries a collation of its own, independent of its columns' — MySQL only. Split off
   *  `databaseCollation` because SQL Server sits between the two answers that one flag used to
   *  conflate: a SQL Server database has a collation and a SQL Server table does not, so one boolean
   *  could not say both without lying to one of `DatabaseDialog`/`TableDialog`. */
  tableCollation: boolean;
  /** A default is either an expression or a literal, and which one it is is worth saying in the
   *  column grid: `uuid()` and the text `uuid()` are stored the same way there and read alike.
   *  PostgreSQL keeps every default as an expression and reports it cast — `'new'::text` — so
   *  there the mark would sit on every row and set nothing apart. */
  markExpressionDefaults: boolean;
  /** The index kinds on offer, in the order they are shown. */
  indexKinds: readonly SqlIndexKind[];
  /** The access methods an index may be built with, upper-cased. */
  indexMethods: readonly string[];
  /** An index can cover just the first few characters of a column — MySQL only. */
  indexPrefix: boolean;
  /** What a primary key is always called, or null where it can be named. */
  primaryKeyName: string | null;
}

/**
 * What one SQL engine does differently, in the places the shared workspace has to know about it.
 *
 * Everything here is a question the UI asks about a server or a column and gets a different answer
 * to depending on the engine — never a call to one. Calls live on {@link SqlApi}. Both are handed
 * down together; see `src/sql/context.tsx`.
 *
 * It is deliberately small: a question only belongs here once two engines answer it differently.
 */
export interface SqlDialect {
  /** Which engine this is, for the few places that have to name it — a label, an icon, a wording. */
  kind: "mysql" | "postgres" | "sqlite" | "clickhouse" | "mssql";

  /** How SQL text is read: which quotes, comments and escapes this engine has. What the statement
   *  splitter and the editor's tokenizer work from — see {@link SqlSyntax}. */
  syntax: SqlSyntax;

  /** The CodeMirror dialect the editor highlights and completes with. Also where
   *  {@link reserved} comes from, so the words the checker skips are exactly the words the editor
   *  paints as keywords. */
  cmDialect: SQLDialect;

  /** Every word this engine owns, lower-cased — see `reservedWords`. Held on the dialect rather
   *  than derived at each call: it is one set per engine, and building it is a string split over
   *  a few hundred words. */
  reserved: ReadonlySet<string>;

  /** What the column and index dialogs offer. */
  editing: SqlEditing;

  /** Whether the database belongs to the server rather than to the user. Its tables are read like
   *  any other's, but nothing in it may be created, renamed or dropped, nor anything done to it as
   *  a whole: the server rebuilds these from its own data directory, and dropping one breaks it. */
  isSystemDatabase(database: string): boolean;

  /** The column the server numbers itself. */
  isAutoIncrement(meta: SqlColumnMeta): boolean;

  /** A column the server computes from the others. */
  isGenerated(meta: SqlColumnMeta): boolean;

  /** A column the server fills in itself, and that an INSERT therefore must not name at all. */
  isServerAssigned(meta: SqlColumnMeta): boolean;

  /**
   * A column holding bytes rather than text.
   *
   * These are the columns the backend hands over base64-encoded, since bytes have no representation
   * of their own in JSON, and so the ones whose values cannot be written back out as the text they
   * arrive as.
   */
  isBinary(meta: SqlColumnMeta): boolean;

  /**
   * A running statement can be stopped from another connection.
   *
   * True on both servers, which have a session to reach in and kill. False on SQLite, where the
   * statement runs inside this process against a file and there is nothing to send a signal to —
   * so the Query tab's Cancel button is closed rather than left to be pressed and do nothing,
   * which is the worse of the two. Also false on ClickHouse in v1: it does have a session to kill
   * (`KILL QUERY`), but reaching it needs a `query_id` tracked per request, which nothing here does
   * yet — see the plan this kind was built from.
   */
  cancellable: boolean;

  /**
   * The Query tab may send writing statements — DDL and anything beyond the four DML verbs
   * `rowsWritable` already covers.
   *
   * Narrower than it once was twice over: DDL got `ddlWritable` of its own (ClickHouse can have the
   * Structure tab open while the Query tab stays shut — its guard does not tell DDL from DML, so
   * opening this flag to reach `ALTER TABLE` would open hand-typed `INSERT` along with it), and
   * dump/restore got `dumpRestoreWritable` of its own for the same shape of reason — see that
   * field's own doc and
   * `docs/specs/2026-09-04-clickhouse-dump-restore-design.md`.
   *
   * Row-level writes — the Data tab's grid inserting, updating or deleting rows — are gated
   * separately by `rowsWritable`, since an engine can have one open without the other: ClickHouse's
   * row writes shipped before its DDL did, see
   * `docs/specs/2026-09-04-clickhouse-row-writes-design.md`.
   *
   * True on every engine but ClickHouse. `DbTab` folds this into the same `readOnly` a connection
   * can be marked with by hand: see `SqlWorkspace`'s `readOnly` prop.
   *
   * The Query tab's own four DML verbs are an exception to this false: they may be sent even while
   * this is `false`, as long as `rowsWritable` is `true` and the connection itself is not marked
   * read-only by hand — see `SqlWorkspaceProps.dmlEvenIfReadOnly` and
   * `docs/specs/2026-09-04-clickhouse-query-dml-design.md`.
   */
  writable: boolean;

  /**
   * The database as a whole may be dumped and restored — `DatabaseActions`' Dump/Restore buttons.
   *
   * Split off `writable` (which now gates only the Query tab) because ClickHouse can have this open
   * without the Query tab: dump/restore runs entirely over the same HTTP interface every other
   * ClickHouse read/write already uses, with no hand-typed-DDL risk the way opening `writable`
   * itself would carry. See
   * `docs/specs/2026-09-04-clickhouse-dump-restore-design.md`'s D8.
   *
   * True on every engine, ClickHouse included. `DbTab` folds this into `SqlWorkspace`'s
   * `dumpRestoreReadOnly` prop, which only `DatabaseActions`' dump/restore buttons read (the Drop
   * button stays on `ddlWritable`/`schemaReadOnly`).
   */
  dumpRestoreWritable: boolean;

  /**
   * The Structure tab may write, and tables and databases may be created, renamed and dropped.
   *
   * Independent of `writable` for the reason that flag's own doc gives. True on every engine,
   * ClickHouse included — see
   * `docs/specs/2026-09-04-clickhouse-ddl-design.md`. `DbTab` folds this into
   * `SqlWorkspace`'s `schemaReadOnly` prop, which the Structure tab, the sidebar's "Add table" and
   * the Drop button read.
   */
  ddlWritable: boolean;

  /**
   * The Data tab's grid may insert, update and delete rows, and the Query tab may send `INSERT`/
   * `UPDATE`/`DELETE`/`TRUNCATE` — independent of `writable`, which gates DDL/dump/restore instead.
   *
   * True on every engine. `DbTab` folds this into `SqlWorkspace`'s `dataReadOnly` prop, which only
   * the Data tab's grid reads — the Query tab is not wired to this flag yet (ClickHouse's still
   * closed there; see the design doc above's "What is left out").
   */
  rowsWritable: boolean;

  /**
   * The filter bar may offer `REGEXP` / `NOT REGEXP`.
   *
   * False on SQLite: it parses the word but ships no implementation, so the operator would always
   * come back as "no such function: regexp". An operator that cannot succeed does not belong in
   * the dropdown — see `build_where` in `drivers/sqlite.rs`, which has no arm for it either.
   */
  regexpFilter: boolean;
}
