/**
 * The handful of lexical rules that differ between the SQL engines — everything a reader of SQL
 * text has to know before it can tell code from a string, a comment or a name.
 *
 * Both the statement splitter and the tokenizer work from this rather than from the engine's name,
 * so a rule is stated once and the code that depends on it stays shared. It is deliberately about
 * *lexing* only: which words mean what is the dialect's business, not this file's.
 *
 * Each field mirrors the same field of `SqlSyntax` in
 * `src-tauri/src/modules/db/drivers/sql/syntax.rs`, which the backend splits with, and the two
 * must agree, since a statement the editor highlights has to be the one the server ends up running.
 */
export interface SqlSyntax {
  /** `#` opens a comment to the end of the line. MySQL only; in PostgreSQL it is an operator. */
  hashComments: boolean;

  /** Whether `--` needs whitespace after it to open a comment. MySQL insists, so `5--3` there is
   *  arithmetic; PostgreSQL treats any `--` as a comment. */
  dashCommentNeedsSpace: boolean;

  /** Block comments nest, so `/* a /* b *␘/ c *␘/` is one comment. PostgreSQL only — stopping at
   *  the first close on MySQL is right, and nesting there would swallow real code. */
  nestedBlockComments: boolean;

  /**
   * The engine's own identifier quote beside the standard `"`, or null when it has none. A pair
   * rather than a single character, because SQL Server's is not symmetric: `[name]` opens with
   * `[` and closes with `]`, doubling `]` — not `[` — to escape one inside a name. When
   * `open === close` (every engine below that has one at all) this behaves exactly as the
   * single-character version it replaces did.
   */
  identifierQuote: { open: string; close: string } | null;

  /**
   * A double-quoted run is a **name**, not a string.
   *
   * True on PostgreSQL, where `"select"` is a column called `select`; false on MySQL in its default
   * mode, where `"x"` is the string `x`. Nothing else in the tokenizer decides as much: read the
   * wrong way round, every quoted column name becomes a string literal — or every string becomes a
   * name the schema check then complains it has never heard of.
   */
  doubleQuoteIsIdentifier: boolean;

  /** A backslash escapes the next character inside a string literal. True on MySQL; on PostgreSQL
   *  a backslash in an ordinary string is just a backslash. */
  backslashEscapes: boolean;

  /** `E'...'` is a string in which a backslash *does* escape. PostgreSQL only, and the one thing
   *  that makes `backslashEscapes` above a property of the literal rather than of the engine. */
  escapeStringPrefix: boolean;

  /** `$tag$ ... $tag$` holds text that may contain anything, semicolons included — which is how a
   *  function body is written. PostgreSQL only. */
  dollarQuoting: boolean;

  /**
   * A line holding nothing but `GO` — optionally followed by a repeat count, `GO 3` — ends the
   * current batch. `GO` is a client-side convention (`sqlcmd`, SSMS), not a SQL Server keyword:
   * sending it to the server as a statement is a syntax error, so the splitter must carve it out
   * before anything downstream ever sees it as code. SQL Server only.
   */
  batchSeparator: boolean;
}

export const MYSQL_SYNTAX: SqlSyntax = {
  hashComments: true,
  dashCommentNeedsSpace: true,
  nestedBlockComments: false,
  identifierQuote: { open: "`", close: "`" },
  doubleQuoteIsIdentifier: false,
  backslashEscapes: true,
  escapeStringPrefix: false,
  dollarQuoting: false,
  batchSeparator: false,
};

export const POSTGRES_SYNTAX: SqlSyntax = {
  hashComments: false,
  dashCommentNeedsSpace: false,
  nestedBlockComments: true,
  identifierQuote: null,
  doubleQuoteIsIdentifier: true,
  backslashEscapes: false,
  escapeStringPrefix: true,
  dollarQuoting: true,
  batchSeparator: false,
};

/**
 * SQLite, which sits between the two on almost every line.
 *
 * It understands MySQL's backtick and PostgreSQL's double quote as identifier quotes, and a
 * double-quoted run is a name — a string only if there is no column of that name, which is a
 * fallback the tokenizer has no business reproducing. `identifierQuote` names the backtick so both
 * spellings lex, and `doubleQuoteIsIdentifier` settles the standard one.
 *
 * No backslash escaping: `'a\b'` in SQLite is the four characters it looks like, and a quote
 * inside a string is doubled the standard way.
 */
export const SQLITE_SYNTAX: SqlSyntax = {
  hashComments: false,
  dashCommentNeedsSpace: false,
  nestedBlockComments: false,
  identifierQuote: { open: "`", close: "`" },
  doubleQuoteIsIdentifier: true,
  backslashEscapes: false,
  escapeStringPrefix: false,
  dollarQuoting: false,
  batchSeparator: false,
};

/**
 * ClickHouse, as its manual states its lexical rules. Both identifier quotes are accepted at once —
 * backtick and double quote — the one point where this matches {@link SQLITE_SYNTAX} exactly rather
 * than either of the other two. The backend has no `SqlSyntax` for it: it reads ClickHouse with
 * the `Scanner` in `src-tauri/src/modules/db/drivers/clickhouse/script.rs`.
 */
export const CLICKHOUSE_SYNTAX: SqlSyntax = {
  hashComments: true,
  dashCommentNeedsSpace: false,
  nestedBlockComments: true,
  identifierQuote: { open: "`", close: "`" },
  doubleQuoteIsIdentifier: true,
  backslashEscapes: true,
  escapeStringPrefix: false,
  dollarQuoting: false,
  batchSeparator: false,
};

/**
 * T-SQL's lexical rules, read off the SQL Server docs and `tiberius`'s default session settings.
 * Mirrored by `MSSQL` in `src-tauri/src/modules/db/drivers/sql/syntax.rs`.
 */
export const MSSQL_SYNTAX: SqlSyntax = {
  // `#` opens a temporary table's name here (`#t`, `##t`), not a comment.
  hashComments: false,
  dashCommentNeedsSpace: false,
  // T-SQL's `/* */` does not nest: a second `/*` inside one is just text, and the first `*/`
  // closes the whole thing.
  nestedBlockComments: false,
  identifierQuote: { open: "[", close: "]" },
  // QUOTED_IDENTIFIER is ON by default, and every client driver — tiberius included — sets it: a
  // double-quoted run is a name, not a string.
  doubleQuoteIsIdentifier: true,
  backslashEscapes: false,
  escapeStringPrefix: false,
  dollarQuoting: false,
  batchSeparator: true,
};

/** What may sit inside a PostgreSQL identifier after its first character — `$` included. */
const IDENT_CHAR = /[\p{L}\p{N}_$]/u;

/**
 * Whether the `'` at `i` opens an `E'...'` literal, the one PostgreSQL string a backslash escapes
 * in.
 *
 * The prefix has to be touching: `E'it\'s; here'` is one string holding a semicolon,
 * while `SELECT e 'x'` is a column aliased `e` beside a plain string, and a name merely *ending*
 * in `e` — `type'x'` — is neither. Checked against the server: `SELECT E'it\'s; here'`
 * returns `it's; here`, while `SELECT 'a\b'` returns `a\b` unchanged.
 */
export function opensEscapeString(sql: string, i: number): boolean {
  const before = sql[i - 1];
  if (before !== "E" && before !== "e") return false;
  const twoBack = sql[i - 2];
  return twoBack === undefined || !IDENT_CHAR.test(twoBack);
}

/**
 * The tag of a dollar quote opening at `i`, or null when one does not.
 *
 * `$$` gives `""` and `$body$` gives `"body"`. A tag is an identifier and so cannot open with a
 * digit — which is what keeps the placeholders `$1, $2` out of this. Read as an opening quote, one
 * of those would never close and would swallow the rest of the script.
 */
export function dollarTag(sql: string, i: number): string | null {
  if (sql[i] !== "$") return null;
  /* Glued to the end of a name it belongs to that name. `$` is a legal character inside a
     PostgreSQL identifier, and the server reads the longest name it can before it looks for a
     quote: `SELECT 1 AS a$b$c` names the column `a$b$c`, and `SELECT$$hi$$` is a syntax error
     naming the whole of `SELECT$$hi$$` — both checked against the server. Read as an opening quote
     instead, `a$b$c` swallows everything up to the next `$b$`, or the rest of the script. */
  if (i > 0 && IDENT_CHAR.test(sql[i - 1])) return null;
  let tag = "";
  for (let j = i + 1; j < sql.length; j += 1) {
    const c = sql[j];
    if (c === "$") return tag;
    if (!/[\p{L}\p{N}_]/u.test(c)) return null;
    if (tag === "" && /\p{N}/u.test(c)) return null;
    tag += c;
  }
  return null;
}
