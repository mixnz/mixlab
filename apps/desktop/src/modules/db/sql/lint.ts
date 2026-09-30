/**
 * Reading a script closely enough to say what is wrong with it, without a server.
 *
 * This is the instant half of the Query tab's error checking. The accurate half is
 * `mysql_validate_sql`, which hands the statement to MySQL itself — but that costs a round trip and
 * so runs on a delay, and it only ever looks at one statement. This one runs on every keystroke
 * over the whole script, and knows the one thing the server does not: which tables and columns the
 * user has actually got, from the outline completion is already working from.
 *
 * **Everything schema-related here is a warning, deliberately.** A script may create the table it
 * then uses, an outline is a snapshot, and only what the connected user may see is in it. The one
 * thing this file is ever certain about is text that cannot be read at all — an unclosed quote, an
 * unbalanced bracket — and those alone are errors.
 *
 * The findings carry translation keys rather than sentences: the editor is bilingual and this file
 * is not where either language lives.
 */

import type { SQLDialect } from "@codemirror/lang-sql";
import type { SqlDialect } from "./dialect";
import { dollarTag, opensEscapeString, type SqlSyntax } from "./syntax";
import type { TranslationKey } from "../../../i18n";
import type { SqlSchemaOutline } from "../types";
import type { SqlStatement } from "../sql/statements";

/**
 * Every word the engine owns, lower-cased — its keywords and its type names, read off the same
 * CodeMirror dialect the editor highlights with, so the two can never drift apart.
 *
 * The dialect's `builtin` list is left out on purpose: on MySQL it holds the *command-line
 * client's* words (`edit`, `pager`, `status`, `source`), and treating those as reserved would
 * quietly stop a column actually named `status` from ever being checked.
 *
 * Built-in *functions* are mostly not in here either, and do not need to be: a name followed by `(`
 * is never checked against the schema.
 */
export function reservedWords(dialect: SQLDialect): ReadonlySet<string> {
  return new Set(
    `${dialect.spec.keywords ?? ""} ${dialect.spec.types ?? ""}`
      .split(/\s+/)
      .filter((word) => word !== "")
  );
}

export type TokenKind = "word" | "quoted" | "string" | "number" | "variable" | "punct" | "comment";

/** One piece of a statement, and where it sits in the script. */
export interface Token {
  kind: TokenKind;
  /** The text exactly as written, quotes and all. */
  raw: string;
  /** An identifier's name with its quoting undone; the raw text for everything else. */
  value: string;
  from: number;
  to: number;
  /** Set on a string, a quoted identifier or a block comment the text ran out inside of. */
  open?: boolean;
}

/** Whether the character can open an unquoted identifier. MySQL allows `$` and anything above
 *  ASCII, which is how a column named in Vietnamese or Japanese is written unquoted. */
function isIdentStart(c: string): boolean {
  return /[A-Za-z_$\u0080-\uffff]/.test(c);
}

function isIdentPart(c: string): boolean {
  return /[A-Za-z0-9_$\u0080-\uffff]/.test(c);
}

/**
 * Breaks SQL into its tokens, keeping every one's place in the original text.
 *
 * Comments are emitted rather than dropped so that an unterminated one can be pointed at; callers
 * that only care about code filter them out. `offset` is added to every position, so a statement
 * can be tokenised on its own text and still report where it sits in the whole script.
 */
export function tokenize(sql: string, syntax: SqlSyntax, offset = 0): Token[] {
  const tokens: Token[] = [];
  let i = 0;

  const push = (kind: TokenKind, from: number, to: number, value?: string, open?: boolean) => {
    const raw = sql.slice(from, to);
    tokens.push({
      kind,
      raw,
      value: value ?? raw,
      from: from + offset,
      to: to + offset,
      ...(open ? { open: true } : {}),
    });
  };

  while (i < sql.length) {
    const c = sql[i];

    if (/\s/.test(c)) {
      i += 1;
      continue;
    }

    // Comments open exactly as the splitter has them — see {@link SqlSyntax}, which both read from.
    if (
      (c === "-" &&
        sql[i + 1] === "-" &&
        (!syntax.dashCommentNeedsSpace || i + 2 >= sql.length || /\s/.test(sql[i + 2]))) ||
      (c === "#" && syntax.hashComments)
    ) {
      const start = i;
      while (i < sql.length && sql[i] !== "\n") i += 1;
      push("comment", start, i);
      continue;
    }

    if (c === "/" && sql[i + 1] === "*") {
      const start = i;
      let depth = 0;
      let closed = false;
      while (i < sql.length) {
        if (sql[i] === "/" && sql[i + 1] === "*") {
          depth += 1;
          i += 2;
          continue;
        }
        if (sql[i] === "*" && sql[i + 1] === "/") {
          depth -= 1;
          i += 2;
          if (depth === 0 || !syntax.nestedBlockComments) {
            closed = true;
            break;
          }
          continue;
        }
        i += 1;
      }
      push("comment", start, i, undefined, !closed);
      continue;
    }

    // A dollar-quoted body is a string that may hold anything — quotes and semicolons included.
    if (syntax.dollarQuoting) {
      const tag = dollarTag(sql, i);
      if (tag !== null) {
        const start = i;
        const close = `$${tag}$`;
        const end = sql.indexOf(close, i + close.length);
        const closed = end !== -1;
        const inner = closed ? sql.slice(i + close.length, end) : sql.slice(i + close.length);
        i = closed ? end + close.length : sql.length;
        push("string", start, i, inner, !closed);
        continue;
      }
    }

    const openQuote = syntax.identifierQuote?.open;
    if (c === "'" || c === '"' || c === openQuote) {
      const start = i;
      i += 1;
      let value = "";
      let closed = false;
      const isIdentOpen = c === openQuote;
      // Which of the two this run is, which decides both how it escapes and what it means.
      const isName = isIdentOpen || (c === '"' && syntax.doubleQuoteIsIdentifier);
      // The character that ends this run — see `statements.ts`'s identical branch for why it can
      // differ from the one that opened it.
      const closeChar = isIdentOpen ? syntax.identifierQuote!.close : c;
      // And whether a backslash escapes inside it: always on MySQL, only in a PostgreSQL
      // `E'...'` — see `opensEscapeString`. Never inside a quoted name.
      const escapes =
        !isName &&
        (syntax.backslashEscapes ||
          (syntax.escapeStringPrefix && c === "'" && opensEscapeString(sql, start)));
      while (i < sql.length) {
        const ch = sql[i];
        i += 1;
        if (ch === "\\" && escapes) {
          if (i < sql.length) {
            value += sql[i];
            i += 1;
          }
          continue;
        }
        if (ch === closeChar) {
          if (sql[i] === closeChar) {
            value += closeChar;
            i += 1;
            continue;
          }
          closed = true;
          break;
        }
        value += ch;
      }
      push(isName ? "quoted" : "string", start, i, value, !closed);
      continue;
    }

    // `@user`, `@@session.x`, and the `:name` a P3 parameter will use. Named apart so none of them
    // is ever mistaken for a column.
    if (c === "@" || (c === ":" && isIdentStart(sql[i + 1] ?? ""))) {
      const start = i;
      i += 1;
      while (i < sql.length && (isIdentPart(sql[i]) || sql[i] === "@" || sql[i] === ".")) i += 1;
      push("variable", start, i);
      continue;
    }

    if (/[0-9]/.test(c) || (c === "." && /[0-9]/.test(sql[i + 1] ?? ""))) {
      const start = i;
      while (i < sql.length && /[0-9.]/.test(sql[i])) i += 1;
      // An exponent, and the sign it may carry.
      if (/[eE]/.test(sql[i] ?? "")) {
        i += 1;
        if (/[+-]/.test(sql[i] ?? "")) i += 1;
        while (i < sql.length && /[0-9]/.test(sql[i])) i += 1;
      }
      push("number", start, i);
      continue;
    }

    if (isIdentStart(c)) {
      const start = i;
      while (i < sql.length && isIdentPart(sql[i])) i += 1;
      push("word", start, i);
      continue;
    }

    push("punct", i, i + 1);
    i += 1;
  }

  return tokens;
}

/** One thing wrong with the script, carrying a key rather than a sentence — see the file's note. */
export interface LintFinding {
  from: number;
  to: number;
  /** `error` only for text that cannot be read; everything the schema has an opinion about is a
   *  warning. */
  severity: "error" | "warning";
  /** A key under `lint.*` in the translations. Typed as a real key, so a message that was never
   *  written does not compile. */
  code: TranslationKey;
  /** What the message interpolates. */
  params?: Record<string, string | number>;
  /** A name close enough to what was written to be worth offering as a one-press fix. */
  suggestion?: string;
}

/** The schema in the shape the checks want it: everything folded to lower case, because MySQL
 *  matches column names that way and matches table names that way on Windows and macOS. Comparing
 *  case-insensitively is the direction that cannot invent a warning. */
interface SchemaIndex {
  database: string;
  /** Lower-cased table name → its lower-cased column names. */
  tables: Map<string, Set<string>>;
  /** The spellings as the database has them, for suggesting. */
  tableNames: string[];
  columnNames: Map<string, string[]>;
}

function indexOutline(outline: SqlSchemaOutline | null): SchemaIndex | null {
  if (!outline || outline.tables.length === 0) return null;
  const tables = new Map<string, Set<string>>();
  const columnNames = new Map<string, string[]>();
  for (const table of outline.tables) {
    const key = table.name.toLowerCase();
    tables.set(key, new Set(table.columns.map((column) => column.name.toLowerCase())));
    columnNames.set(key, table.columns.map((column) => column.name));
  }
  return {
    database: outline.database,
    tables,
    tableNames: outline.tables.map((table) => table.name),
    columnNames,
  };
}

/** How far apart two names may be and still be offered as "did you mean". Two edits catches the
 *  transposition, the doubled letter and the dropped one; three starts matching unrelated words. */
const MAX_EDITS = 2;

/** Levenshtein distance, giving up as soon as it passes `max` — the answer is only ever compared
 *  against a small bound, and most candidates are nowhere near. */
function distance(a: string, b: string, max: number): number {
  if (Math.abs(a.length - b.length) > max) return max + 1;
  let previous = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i += 1) {
    const row = [i];
    let best = i;
    for (let j = 1; j <= b.length; j += 1) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      const value = Math.min(previous[j] + 1, row[j - 1] + 1, previous[j - 1] + cost);
      row.push(value);
      best = Math.min(best, value);
    }
    if (best > max) return max + 1;
    previous = row;
  }
  return previous[b.length];
}

/** The candidate closest to `name`, when one is close enough to be worth offering. */
function closest(name: string, candidates: readonly string[]): string | undefined {
  const wanted = name.toLowerCase();
  let best: string | undefined;
  let bestDistance = MAX_EDITS + 1;
  for (const candidate of candidates) {
    const d = distance(wanted, candidate.toLowerCase(), MAX_EDITS);
    if (d < bestDistance) {
      bestDistance = d;
      best = candidate;
      if (d === 1) break;
    }
  }
  return bestDistance <= MAX_EDITS ? best : undefined;
}

/** The statements whose table and column names name things that already exist. A `CREATE TABLE`
 *  names one that does not yet, which is not a mistake. */
const CHECKED_VERBS = new Set(["SELECT", "UPDATE", "DELETE", "INSERT", "REPLACE", "WITH"]);

/** Words that end a table list rather than continuing it. */
const AFTER_TABLES = new Set([
  "WHERE", "ON", "USING", "SET", "GROUP", "ORDER", "HAVING", "LIMIT", "UNION", "VALUES",
  "SELECT", "LEFT", "RIGHT", "INNER", "CROSS", "OUTER", "JOIN", "STRAIGHT_JOIN", "NATURAL",
  "FOR", "INTO", "PARTITION", "WINDOW", "PROCEDURE", "LOCK", "OFFSET", "DUAL",
]);

/** What a comma may not sit in front of. */
const NOT_AFTER_COMMA = new Set(["FROM", "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "SET"]);

/** One table named in a statement, and where its name is written. */
export interface TableRef {
  /** The database it was qualified with, when it was qualified. */
  database: string;
  /** The name as written — backticks and all, so a suggested replacement can keep the quoting. */
  token: Token;
  from: number;
  to: number;
}

/** A suggested name, written the way the name it replaces was written. Swapping a quoted identifier
 *  for a bare one would be a fix that breaks anything needing the quotes — and quoting it with the
 *  wrong engine's quote would be a fix that does not parse, so the pair comes from the dialect:
 *  MySQL's backtick, PostgreSQL's `"`, SQL Server's `[`/`]`. The close character is what escapes a
 *  literal one inside the name, doubled — for a symmetric pair that is also what opens it, but for
 *  SQL Server's it is only ever `]`. */
function asWritten(original: Token, name: string, dialect: SqlDialect): string {
  if (original.kind !== "quoted") return name;
  // PostgreSQL's own `identifierQuote` is null — it reads `"` through `doubleQuoteIsIdentifier`
  // instead — so a symmetric `"` pair stands in for "quote it the standard way" here, the same
  // default the single-character version this replaces fell back to.
  const { open, close } = dialect.syntax.identifierQuote ?? { open: '"', close: '"' };
  return `${open}${name.split(close).join(close + close)}${close}`;
}

/** The token at `i` as an upper-cased keyword, or undefined when it is not a bare word. */
function wordAt(code: readonly Token[], i: number): string | undefined {
  return code[i]?.kind === "word" ? code[i].value.toUpperCase() : undefined;
}

/**
 * What a statement says about where its names come from.
 *
 * Read once and used twice: the checks below warn about a name none of these tables has, and
 * [reference.ts](./reference.ts) answers what the name under the pointer actually *is*. Both have
 * to resolve `u.id` to the same column, so both ask this.
 */
export interface StatementScope {
  /** Every table the statement names, in the order it names them. */
  tables: TableRef[];
  /** Alias, lower-cased, → the table it stands for, lower-cased. */
  aliases: Map<string, string>;
  /** Token positions the table list has already accounted for, so a table name is not then read a
   *  second time as a column. */
  consumed: Set<number>;
  /** Names introduced by the statement itself — `AS total`, and a bare alias in a select list. A
   *  later clause may refer to one, and the schema has never heard of it. */
  declared: Set<string>;
  /** Set when something in the statement gives names a scope this does not model: a subquery, a
   *  derived table, a `UNION`. What was collected is still true — it is just no longer everything,
   *  which is why the checks stop here and the hover does not. */
  opaque: boolean;
}

/**
 * Reads the tables and aliases a statement brings into scope.
 *
 * `code` is the statement's tokens with the comments already dropped, and `verb` its opening
 * keyword — which is what tells `INSERT INTO t` from `SELECT ... INTO OUTFILE`.
 */
export function readScope(
  code: readonly Token[],
  verb: string,
  dialect: SqlDialect
): StatementScope {
  const tables: TableRef[] = [];
  const aliases = new Map<string, string>();
  const consumed = new Set<number>();
  const declared = new Set<string>();
  let opaque = false;

  const word = (i: number) => wordAt(code, i);

  /** Reads the comma-separated table list starting at `i`, and answers where it ended. */
  function readTables(i: number): number {
    for (;;) {
      const token = code[i];
      if (!token) return i;
      // A derived table or a subquery: everything inside it has a scope of its own.
      if (token.raw === "(") {
        opaque = true;
        return i;
      }
      if (token.kind !== "word" && token.kind !== "quoted") return i;
      // A keyword here is the next clause, not a table — `FROM DUAL`, `DELETE FROM ... WHERE`.
      if (token.kind === "word" && AFTER_TABLES.has(token.value.toUpperCase())) return i;

      let database = "";
      let name = token;
      consumed.add(i);
      if (code[i + 1]?.raw === "." && (code[i + 2]?.kind === "word" || code[i + 2]?.kind === "quoted")) {
        database = token.value;
        name = code[i + 2];
        consumed.add(i + 1);
        consumed.add(i + 2);
        i += 2;
      }
      // The range is the name alone, not the `db.` in front of it: that part is not what is
      // unknown, and a fix that replaced the whole thing would drop the qualifier.
      tables.push({ database, token: name, from: name.from, to: name.to });
      i += 1;

      // `PARTITION (p0, p1)` sits between the table and its alias, and holds partition names this
      // knows nothing about.
      if (word(i) === "PARTITION" && code[i + 1]?.raw === "(") {
        opaque = true;
        return i;
      }

      let alias: Token | undefined;
      if (word(i) === "AS") {
        consumed.add(i);
        alias = code[i + 1];
        consumed.add(i + 1);
        i += 2;
      } else if (
        code[i] &&
        (code[i].kind === "quoted" ||
          (code[i].kind === "word" && !dialect.reserved.has(code[i].value.toLowerCase())))
      ) {
        alias = code[i];
        consumed.add(i);
        i += 1;
      }
      if (alias) aliases.set(alias.value.toLowerCase(), name.value.toLowerCase());

      if (code[i]?.raw === ",") {
        i += 1;
        continue;
      }
      return i;
    }
  }

  let depth = 0;
  for (let i = 0; i < code.length; i += 1) {
    const token = code[i];
    if (token.raw === "(") {
      depth += 1;
      continue;
    }
    if (token.raw === ")") {
      depth -= 1;
      continue;
    }
    if (token.kind !== "word") continue;
    const w = token.value.toUpperCase();

    // A nested SELECT — in a subquery, or the second arm of a UNION — brings names this cannot
    // see, and unknown ones would be reported against the wrong scope.
    if (depth > 0 && (w === "SELECT" || w === "FROM")) opaque = true;
    if (depth === 0 && (w === "UNION" || w === "WITH")) opaque = true;

    if (w === "AS" && code[i + 1]) declared.add(code[i + 1].value.toLowerCase());

    if (depth !== 0) continue;
    const introduces =
      w === "FROM" ||
      w === "JOIN" ||
      // Only as the statement's own opening word: the `UPDATE` of `ON DUPLICATE KEY UPDATE` is
      // followed by assignments, and reading those as tables would flag every column in them.
      (w === "UPDATE" && i === 0) ||
      // `INSERT INTO t`, but not `SELECT ... INTO OUTFILE` or `INTO @variable`.
      (w === "INTO" &&
        (verb === "INSERT" || verb === "REPLACE") &&
        code[i + 1]?.kind !== "variable" &&
        word(i + 1) !== "OUTFILE" &&
        word(i + 1) !== "DUMPFILE");
    if (introduces) i = readTables(i + 1) - 1;
  }

  return { tables, aliases, consumed, declared, opaque };
}

/**
 * Checks one statement, appending whatever it finds.
 *
 * The structural checks always run. The schema checks run only over a statement that reads or
 * writes existing tables, and only when the statement can be understood well enough for a warning
 * to mean something — a subquery, a `UNION` or a derived table gives every name its own scope, and
 * rather than model that, this stops looking. Silence is the right failure here: a checker that
 * cries wolf on valid SQL gets switched off, and then it catches nothing at all.
 */
function checkStatement(
  statement: SqlStatement,
  index: SchemaIndex | null,
  out: LintFinding[],
  dialect: SqlDialect,
) {
  const all = tokenize(statement.text, dialect.syntax, statement.from);
  const code = all.filter((token) => token.kind !== "comment");

  let unreadable = false;
  for (const token of all) {
    if (!token.open) continue;
    unreadable = true;
    out.push({
      from: token.from,
      to: token.to,
      severity: "error",
      code:
        token.kind === "comment"
          ? "lint.openComment"
          : token.kind === "quoted"
            ? "lint.openIdentifier"
            : "lint.openString",
    });
  }

  const open: Token[] = [];
  for (const token of code) {
    if (token.raw === "(") open.push(token);
    else if (token.raw === ")") {
      if (open.length === 0) {
        out.push({ from: token.from, to: token.to, severity: "error", code: "lint.strayBracket" });
      } else {
        open.pop();
      }
    }
  }
  for (const token of open) {
    out.push({ from: token.from, to: token.to, severity: "error", code: "lint.unclosedBracket" });
  }

  for (let i = 0; i < code.length; i += 1) {
    if (code[i].raw !== ",") continue;
    const next = code[i + 1];
    const dangling =
      next === undefined ||
      next.raw === ")" ||
      (next.kind === "word" && NOT_AFTER_COMMA.has(next.value.toUpperCase()));
    if (dangling) {
      out.push({ from: code[i].from, to: code[i].to, severity: "error", code: "lint.danglingComma" });
    }
  }

  // Past here the text has to be readable and the statement has to be about tables that exist.
  if (unreadable || index === null || !CHECKED_VERBS.has(statement.verb)) return;

  const { tables, aliases, consumed, declared, opaque } = readScope(code, statement.verb, dialect);

  const scope: string[] = [];
  let complete = tables.length > 0;
  for (const table of tables) {
    if (table.database !== "" && table.database.toLowerCase() !== index.database.toLowerCase()) {
      // Another database entirely. The outline covers one, so there is nothing to check against.
      complete = false;
      continue;
    }
    const key = table.token.value.toLowerCase();
    if (index.tables.has(key)) {
      scope.push(key);
      continue;
    }
    complete = false;
    const near = closest(table.token.value, index.tableNames);
    out.push({
      from: table.from,
      to: table.to,
      severity: "warning",
      code: "lint.unknownTable",
      params: { name: table.token.value },
      ...(near ? { suggestion: asWritten(table.token, near, dialect) } : {}),
    });
  }

  if (opaque || !complete) return;

  /** Every column any table in scope has, and where each spelling came from. */
  const inScope = new Set<string>();
  const spellings: string[] = [];
  for (const key of scope) {
    for (const column of index.tables.get(key) ?? []) inScope.add(column);
    for (const column of index.columnNames.get(key) ?? []) spellings.push(column);
  }
  const tableOf = (name: string): string | undefined => {
    const key = name.toLowerCase();
    const aliased = aliases.get(key);
    if (aliased && index.tables.has(aliased)) return aliased;
    return scope.includes(key) ? key : undefined;
  };

  for (let i = 0; i < code.length; i += 1) {
    const token = code[i];
    if (token.kind !== "word" && token.kind !== "quoted") continue;
    if (consumed.has(i)) continue;

    // `alias.column` — the surest check there is. What the alias stands for is known exactly, so
    // an unknown name here is unknown, full stop.
    if (code[i + 1]?.raw === "." && code[i + 2]) {
      const owner = tableOf(token.value);
      const column = code[i + 2];
      i += 2;
      if (!owner || (column.kind !== "word" && column.kind !== "quoted")) continue;
      if (index.tables.get(owner)?.has(column.value.toLowerCase())) continue;
      const near = closest(column.value, index.columnNames.get(owner) ?? []);
      out.push({
        from: column.from,
        to: column.to,
        severity: "warning",
        code: "lint.unknownColumn",
        params: { name: column.value, table: token.value },
        ...(near ? { suggestion: asWritten(column, near, dialect) } : {}),
      });
      continue;
    }

    if (token.kind === "word" && dialect.reserved.has(token.value.toLowerCase())) continue;
    // A name followed by `(` is a function, and MixLab does not carry a list of every function
    // MySQL and its plugins have.
    if (code[i + 1]?.raw === "(") continue;
    if (code[i - 1]?.raw === "." || wordAt(code, i - 1) === "AS") continue;
    const key = token.value.toLowerCase();
    if (inScope.has(key) || declared.has(key) || aliases.has(key) || scope.includes(key)) continue;

    // An unqualified name that is not a column could be any number of things this does not model:
    // a function without brackets, a unit in an `INTERVAL`, an alias written without `AS`. So it
    // is only worth saying anything when there is a column near enough that a typo is the likely
    // explanation — which is also the only case where there is anything useful to say.
    const near = closest(token.value, spellings);
    if (!near) continue;
    out.push({
      from: token.from,
      to: token.to,
      severity: "warning",
      code: "lint.unknownName",
      params: { name: token.value },
      suggestion: asWritten(token, near, dialect),
    });
  }
}

/**
 * Everything worth saying about a script, in the order it is written.
 *
 * The script is passed as the statements it was already split into rather than as its text: the
 * editor splits the same text anyway, to mark where one statement ends, and each statement
 * carries where it sits so every finding can point back into the script.
 */
export function lintScript(
  statements: readonly SqlStatement[],
  outline: SqlSchemaOutline | null,
  dialect: SqlDialect
): LintFinding[] {
  const index = indexOutline(outline);
  const findings: LintFinding[] = [];
  for (const statement of statements) checkStatement(statement, index, findings, dialect);
  return findings;
}

/**
 * Where in the script to draw what the server said about a statement.
 *
 * MySQL reports a syntax error as `... at line N`, counted from the start of the statement it was
 * given rather than from the start of the script. That line, trimmed of its indentation, is the
 * range worth underlining; a statement the server pointed at no particular line in gets underlined
 * whole, which is honest — it is all that is known.
 */
export function problemRange(
  statement: SqlStatement,
  line: number | null
): { from: number; to: number } {
  const whole = { from: statement.from, to: statement.to };
  if (line === null || line < 1) return whole;
  const lines = statement.text.split("\n");
  if (line > lines.length) return whole;

  let offset = 0;
  for (let i = 0; i < line - 1; i += 1) offset += lines[i].length + 1;
  const text = lines[line - 1];
  const trimmed = text.trim();
  // A blank line carries nothing to underline, and a zero-width mark is invisible.
  if (trimmed === "") return whole;
  const lead = text.length - text.trimStart().length;
  return {
    from: statement.from + offset + lead,
    to: statement.from + offset + lead + trimmed.length,
  };
}
