/**
 * The three things the Query tab checks before it sends a script anywhere.
 *
 * None of them is about whether the SQL is *correct* — that is `lint.ts` and the server. These are
 * about whether it does more than the person typing it meant: a `DELETE` that names no rows takes
 * every one of them, a `SELECT` with no `LIMIT` can name more rows than there is memory for, and a
 * connection someone marked read-only should not be the one a `DROP` goes down.
 *
 * All three read the statement's tokens rather than matching text with a regular expression: a
 * `WHERE` inside a string or a comment is not a `WHERE`, and that is exactly the case that would
 * make a regular expression wave a dangerous statement through.
 */

import { tokenize, type Token } from "./lint";
import type { SqlDialect } from "./dialect";
import type { SqlStatement } from "./statements";

/** One name-or-keyword of a statement that is code rather than comment, string or bracketed
 *  subquery. */
interface TopWord {
  /**
   * Upper-cased, for comparing against the keyword sets below — and **empty for a backtick-quoted
   * identifier**, which is a name and must never match one of them. The quoting is how MySQL is
   * told that `` `where` `` is a column rather than a clause, and reading it as the clause is how a
   * gate would talk itself out of asking about the statement.
   */
  word: string;
  token: Token;
}

/** The tokens that are actually code, with the brackets counted so a clause can be recognised as
 *  belonging to the statement itself rather than to a subquery inside it. */
function topLevelWords(statement: SqlStatement, dialect: SqlDialect): TopWord[] {
  const words: TopWord[] = [];
  const code = tokenize(statement.text, dialect.syntax, statement.from).filter((t) => t.kind !== "comment");
  let depth = 0;
  for (const token of code) {
    if (token.raw === "(") depth += 1;
    else if (token.raw === ")") depth -= 1;
    // Quoted identifiers are kept rather than skipped: a script that backticks every name — which
    // is what every tool that generates SQL does — would otherwise leave the dialog unable to say
    // which table a `DROP` was about.
    else if (depth === 0 && (token.kind === "word" || token.kind === "quoted")) {
      words.push({ word: token.kind === "word" ? token.value.toUpperCase() : "", token });
    }
  }
  return words;
}

/**
 * ClickHouse's own spelling of `UPDATE` — `ALTER TABLE <name> UPDATE col = val, ... [WHERE ...]`.
 * Recognises only the simplest shape: exactly one `AlterCommand`, nothing else in the same
 * statement. ClickHouse allows several comma-separated commands in one `ALTER TABLE` (`ALTER TABLE
 * t DROP COLUMN x, UPDATE y = 1 WHERE ...`) — a top-level comma anywhere in the statement means
 * more than one command is packed in, and this returns `null` rather than guess which one governs
 * (D3 of `docs/specs/2026-09-04-clickhouse-query-dml-design.md`).
 *
 * `null` on every dialect but ClickHouse: no other engine spells `UPDATE` this way, and MySQL's own
 * `ALTER TABLE` never carries an `UPDATE` clause at all.
 */
export function clickhouseAlterUpdate(
  statement: SqlStatement,
  dialect: SqlDialect
): { table: string; clauses: TopWord[] } | null {
  if (dialect.kind !== "clickhouse" || statement.verb !== "ALTER") return null;
  const code = tokenize(statement.text, dialect.syntax, statement.from).filter((t) => t.kind !== "comment");
  let depth = 0;
  const words: TopWord[] = [];
  for (const token of code) {
    if (token.raw === "(") {
      depth += 1;
      continue;
    }
    if (token.raw === ")") {
      depth -= 1;
      continue;
    }
    if (depth === 0 && token.raw === ",") return null;
    if (depth === 0 && (token.kind === "word" || token.kind === "quoted")) {
      words.push({ word: token.kind === "word" ? token.value.toUpperCase() : "", token });
    }
  }
  // words[0] is ALTER itself.
  if (words[1]?.word !== "TABLE") return null;
  const table = words[2];
  if (!table || words[3]?.word !== "UPDATE") return null;
  return { table: table.token.value, clauses: words.slice(4) };
}

/**
 * Whether `statement` is one of the row-level DML verbs a `rowsWritable` dialect may send through
 * the Query tab even while `writable` is false — ClickHouse's four verbs (D5 of
 * `docs/specs/2026-09-04-clickhouse-query-dml-design.md`). `INSERT`/`DELETE`/
 * `TRUNCATE` are spelled the same as on every other dialect; `ALTER TABLE <name> UPDATE ...` is
 * ClickHouse's own spelling, recognised by {@link clickhouseAlterUpdate}.
 */
export function isRowsDml(statement: SqlStatement, dialect: SqlDialect): boolean {
  if (statement.verb === "INSERT" || statement.verb === "DELETE" || statement.verb === "TRUNCATE") {
    return true;
  }
  return clickhouseAlterUpdate(statement, dialect) !== null;
}

/**
 * The verbs that make a `WITH` more than a read.
 *
 * MySQL 8 lets a common table expression lead into an `UPDATE`, a `DELETE`, an `INSERT` or a
 * `REPLACE`, so a statement whose first word is `WITH` may still be a write — the one blind spot
 * shared by all three checks in this file. Every one of these words is reserved, so an unquoted one
 * at the top level of a statement is the statement's own verb and never a column someone named.
 */
const WRITING_WORDS = new Set(["UPDATE", "DELETE", "INSERT", "REPLACE"]);

/** A statement that takes more than a sentence about it would suggest. */
export interface UnguardedWrite {
  /** What it does to what it names: `rows` for a statement that rewrites every row of a table,
   *  `drop` for one that removes the thing itself. The two are asked about in different words —
   *  rows come back from a backup, a dropped table's grants and triggers do not. */
  kind: "rows" | "drop";
  /** `UPDATE`, `DELETE`, `TRUNCATE`, or a `DROP`/`ALTER` with the object it names — `DROP TABLE`
   *  rather than `DROP`, since which of them it is is the difference between losing a table and
   *  losing the server. */
  verb: string;
  /** The table it is aimed at, as written. Empty when the statement's shape defeated the reader —
   *  which is not a reason to wave it through, only a reason to name it less precisely. */
  table: string;
}

/** The kinds of thing a `DROP` or an `ALTER` names. */
const OBJECTS = new Set([
  "TABLE", "TABLES", "DATABASE", "SCHEMA", "VIEW", "INDEX", "TRIGGER", "EVENT",
  "FUNCTION", "PROCEDURE", "USER", "ROLE", "TABLESPACE", "SERVER", "LOGFILE",
]);

/** Words that may sit between the verb, the object and the name, and say nothing about either. */
const FILLER = new Set(["TEMPORARY", "IF", "EXISTS", "ONLINE", "OFFLINE", "IGNORE"]);

/** What a `DROP` or an `ALTER` is aimed at: the object kind it names, and the name itself. `at` is
 *  where the verb sits, which is not always the front — see {@link actualVerb}. */
function dropTarget(verb: string, words: readonly TopWord[], at: number): UnguardedWrite {
  let i = at + 1;
  while (i < words.length && FILLER.has(words[i].word)) i += 1;
  const object = words[i] && OBJECTS.has(words[i].word) ? words[i].word : "";
  if (object !== "") i += 1;
  while (i < words.length && FILLER.has(words[i].word)) i += 1;
  return {
    kind: "drop",
    verb: object === "" ? verb : `${verb} ${object}`,
    table: words[i]?.token.value ?? "",
  };
}

/** The verbs this file has an opinion about, whether they open the statement or follow a CTE. */
const GUARDED = new Set(["UPDATE", "DELETE", "TRUNCATE", "DROP", "ALTER"]);

/**
 * What the statement actually does, and where in its words that starts.
 *
 * Almost always the opening keyword, at the front. The exception is `WITH`: a common table
 * expression says nothing about what it leads into, and `WITH ids AS (...) DELETE FROM users` is a
 * `DELETE` however it begins. A CTE's own body is bracketed, so a writing word at the *top* level
 * is the thing the statement leads into rather than anything inside the expression.
 *
 * `verb` is the empty string for a statement none of the checks care about.
 */
function actualVerb(opening: string, words: readonly TopWord[]): { verb: string; at: number } {
  if (opening !== "WITH") return { verb: opening, at: 0 };
  const at = words.findIndex(({ word }) => GUARDED.has(word));
  return at < 0 ? { verb: "", at: 0 } : { verb: words[at].word, at };
}

/**
 * The words `EXPLAIN` puts between itself and the statement it is given.
 *
 * Both dialects allow the bare list (`EXPLAIN ANALYZE VERBOSE ...`, `EXPLAIN ANALYZE FORMAT=TREE
 * ...`); PostgreSQL also spells it in brackets, which {@link topLevelWords} drops along with every
 * other bracketed group, so a word missing from here can only cost a bracket-less `EXPLAIN
 * ANALYZE` a refusal it did not need — never a write let through.
 */
const EXPLAIN_OPTIONS = new Set([
  "ANALYZE", "ANALYSE", "VERBOSE", "COSTS", "SETTINGS", "GENERIC_PLAN", "BUFFERS", "SERIALIZE",
  "WAL", "TIMING", "SUMMARY", "MEMORY", "FORMAT", "TEXT", "XML", "JSON", "YAML", "TREE",
  "TRADITIONAL", "EXTENDED", "PARTITIONS", "ON", "OFF", "TRUE", "FALSE",
]);

/** `EXPLAIN`'s one option that turns a plan into a run. PostgreSQL takes both spellings. */
const EXPLAIN_ANALYZE = new Set(["ANALYZE", "ANALYSE"]);

/**
 * Whether an `EXPLAIN` runs the statement it is given rather than only planning it.
 *
 * `EXPLAIN ANALYZE <stmt>` executes `<stmt>` — always on PostgreSQL, and on MySQL since 8.0.18 —
 * so `EXPLAIN ANALYZE DELETE FROM users` empties the table for real. The whole statement's tokens
 * are read rather than only the top-level ones, because PostgreSQL's bracketed spelling
 * (`EXPLAIN (ANALYZE, VERBOSE) ...`) hides the word one level down.
 *
 * `ANALYZE` is reserved in both dialects, so an unquoted one anywhere in an `EXPLAIN` is the
 * option and never a column someone named.
 */
function explainRuns(statement: SqlStatement, dialect: SqlDialect): boolean {
  return tokenize(statement.text, dialect.syntax, statement.from).some(
    (token) => token.kind === "word" && EXPLAIN_ANALYZE.has(token.value.toUpperCase())
  );
}

/**
 * The statement the gates below have to judge, which is not always the one that was written.
 *
 * `EXPLAIN ANALYZE DELETE FROM users` is a `DELETE` wearing a reading word in front of it, so the
 * `EXPLAIN` and its options are read past and what follows them is handed on in their place. Every
 * other statement is itself, and the words are the ones {@link topLevelWords} found.
 *
 * `verb` is empty when nothing follows the options, which no gate treats as a read.
 */
function judged(
  statement: SqlStatement,
  dialect: SqlDialect
): { verb: string; words: TopWord[] } {
  const words = topLevelWords(statement, dialect);
  if (statement.verb !== "EXPLAIN" || !explainRuns(statement, dialect)) {
    return { verb: statement.verb, words };
  }
  let at = 1;
  while (at < words.length && EXPLAIN_OPTIONS.has(words[at].word)) at += 1;
  return { verb: words[at]?.word ?? "", words: words.slice(at) };
}

/**
 * The statements in the script that would empty a table, or remove one outright.
 *
 * `UPDATE` and `DELETE` count when they carry neither a `WHERE` nor a `LIMIT` of their own — a
 * `DELETE ... LIMIT 10` is bounded, whatever else is true of it. `TRUNCATE` always counts: saying
 * which rows is not something it can do.
 *
 * A `WHERE` inside a subquery does not save the outer statement, which is why only the top level
 * is looked at: `DELETE FROM t WHERE ...` is guarded, `DELETE FROM t` with a subquery somewhere in
 * its `FROM` is not.
 *
 * `DROP` always counts, and an `ALTER` counts when it drops something of its own — a column, a
 * partition, a key. Nothing about either says which rows, so the row test above cannot be asked of
 * them; the reason to ask before running one is simply that it is the more final of the two things
 * this file is here about. It would be a strange gate that stopped `DELETE FROM users` and waved
 * `DROP TABLE users` straight through.
 *
 * A statement opening with `WITH` is judged by what it leads into, not by that word — see
 * {@link actualVerb}. Everything after the verb is read from where the verb sits, so the `WHERE`
 * that bounds a common table expression is not mistaken for one bounding the `DELETE` it feeds.
 */
export function unguardedWrites(
  statements: readonly SqlStatement[],
  dialect: SqlDialect
): UnguardedWrite[] {
  const found: UnguardedWrite[] = [];
  for (const statement of statements) {
    // The cheap test first: the opening word settles it for everything but a `WITH` and an
    // `EXPLAIN`, and only those are worth tokenising a statement to look inside. A script is mostly
    // `SELECT`s, and this runs over the whole of it every time Run All is pressed.
    if (!GUARDED.has(statement.verb) && statement.verb !== "WITH" && statement.verb !== "EXPLAIN") {
      continue;
    }
    const { verb: opening, words } = judged(statement, dialect);
    const { verb, at } = actualVerb(opening, words);
    if (!GUARDED.has(verb)) continue;
    // The statement proper: what the verb governs, and nothing in front of it.
    const clauses = words.slice(at + 1);

    if (verb === "DROP" || verb === "ALTER") {
      // ClickHouse's own spelling of `UPDATE` — judged the same way a plain `UPDATE` is, just
      // below, not as a `DROP`: it changes rows, not the table itself. Checked before the
      // DROP-shape branch so the two never compete over the same statement.
      const alterUpdate = verb === "ALTER" ? clickhouseAlterUpdate(statement, dialect) : null;
      if (alterUpdate !== null) {
        if (!alterUpdate.clauses.some(({ word }) => word === "WHERE" || word === "LIMIT")) {
          found.push({ kind: "rows", verb: "UPDATE", table: alterUpdate.table });
        }
        continue;
      }
      // An `ALTER` is only here for what it drops: adding a column or an index changes no data,
      // and asking about every one of those is how a question stops being read.
      if (verb === "ALTER" && !clauses.some(({ word }) => word === "DROP")) continue;
      found.push(dropTarget(verb, words, at));
      continue;
    }

    if (verb !== "TRUNCATE" && clauses.some(({ word }) => word === "WHERE" || word === "LIMIT")) {
      continue;
    }

    // The table is the word after `UPDATE` / after the `FROM` or `TABLE` that follows the verb.
    // Anything more careful than this belongs in a parser; getting the name wrong costs a vaguer
    // sentence in a dialog, and getting the *danger* wrong is what is being avoided here.
    const from = clauses.findIndex(({ word }) => word === "FROM" || word === "TABLE");
    const name = from >= 0 ? clauses[from + 1] : clauses[0];
    found.push({ kind: "rows", verb, table: name?.token.value ?? "" });
  }
  return found;
}

/**
 * How many rows a `SELECT` written without a `LIMIT` is sent with.
 *
 * Fixed rather than a preference. What the ceiling is there for is to stop an unbounded `SELECT`
 * against an uncounted table from naming more rows than there is memory for — and that is a
 * property of the wire, not a matter of taste. It is set to what the backend will decode of one
 * result set, so the ceiling never cuts a result the backend would have delivered whole: past this
 * the set comes back truncated either way, and the results say so.
 *
 * A `LIMIT` written by hand is always left alone, which is the answer for anyone who wants a
 * different number.
 */
export const AUTO_LIMIT = 10_000;

/**
 * The same `SELECT`, but with `TOP (n)` inserted right after its `SELECT` (and its `DISTINCT`/`ALL`,
 * when it has one) — SQL Server's own spelling of the ceiling {@link withLimit} appends everywhere
 * else. There is no `LIMIT` on this engine, and `OFFSET ... FETCH` cannot be appended blind either:
 * it demands an `ORDER BY` of its own, which most ad hoc reads do not carry (see the note on
 * `ORDER BY (SELECT NULL)` in `drivers/mssql.rs::table_data`) — `TOP` is the one ceiling that never
 * needs one.
 *
 * `at` is the offset right after that keyword — `words[i].token.to`, which {@link topLevelWords}
 * reports against the *whole script* (`statement.from` is folded into every token's position, since
 * `tokenize` is called with it as a base), so it is turned back into an offset into `statement.text`
 * by subtracting `statement.from` before slicing.
 */
function withTop(statement: SqlStatement, limit: number, words: readonly TopWord[]): string {
  const selectAt = words.findIndex(({ word }) => word === "SELECT");
  const next = words[selectAt + 1];
  const after = next && (next.word === "DISTINCT" || next.word === "ALL") ? next : words[selectAt];
  const at = after.token.to - statement.from;
  return `${statement.text.slice(0, at)} TOP (${limit})${statement.text.slice(at)}`;
}

/**
 * The same `SELECT` with a ceiling on how many rows it can name, or null when it needs none.
 *
 * Only a statement that reads from somewhere and sets no ceiling of its own is touched: `SELECT 1`
 * gets nothing, and a `SELECT ... LIMIT 10` (or SQL Server's `SELECT TOP 10 ...`) is already saying
 * what it wants. Everywhere but SQL Server the ceiling is `LIMIT n` appended on a line of its own —
 * the statement may well end in a `-- comment`, and a `LIMIT` appended to that line would be part of
 * the comment rather than part of the statement. SQL Server has no `LIMIT` at all, so
 * {@link withTop} inserts `TOP (n)` at the front instead — see its own doc comment for why appending
 * is not an option there.
 *
 * A `WITH` counts as a read, since a common table expression is most often written to be selected
 * from — but only until it turns out to lead into a write, where a ceiling would not page a result
 * but cap how many rows the statement changes. The CTE's own body sits inside parentheses, which
 * {@link topLevelWords} does not descend into, so the `SELECT` {@link withTop} finds is always the
 * one the CTE is read from — never one buried inside its definition.
 */
export function withLimit(
  statement: SqlStatement,
  limit: number,
  dialect: SqlDialect
): string | null {
  if (statement.verb !== "SELECT" && statement.verb !== "WITH") return null;
  const words = topLevelWords(statement, dialect);
  if (statement.verb === "WITH" && words.some(({ word }) => WRITING_WORDS.has(word))) return null;
  if (!words.some(({ word }) => word === "FROM")) return null;
  if (words.some(({ word }) => word === "LIMIT")) return null;
  // `FETCH FIRST n ROWS ONLY` is the standard's spelling of the same ceiling — SQL Server has it
  // too, alongside `OFFSET`, and unlike `TOP` it does demand an `ORDER BY` of its own. A statement
  // that has one cannot also carry a `LIMIT`/`TOP`: appending one is a syntax error rather than a
  // narrower page.
  if (words.some(({ word }) => word === "FETCH")) return null;
  // SQL Server's own ceiling, already set — never doubled up.
  if (words.some(({ word }) => word === "TOP")) return null;
  // `SELECT ... INTO OUTFILE` and `INTO @var` are not result sets to be paged through — nor is SQL
  // Server's `SELECT ... INTO newtable`, which a `TOP` would silently turn into "copy only the first
  // n rows" rather than leaving it alone the way this function does for every other kind of `INTO`.
  if (words.some(({ word }) => word === "INTO")) return null;
  // A locking clause — `FOR UPDATE`, `FOR SHARE`, `LOCK IN SHARE MODE` — has to come *after* the
  // `LIMIT` in MySQL's grammar, so one appended here lands on the wrong side of it and the whole
  // statement stops parsing (1064, checked on 5.7.44 and on 8.4.8). A locking read is deliberate
  // and rarely unbounded, so it is left exactly as it was written rather than rewritten around.
  if (words.some(({ word }) => word === "FOR" || word === "LOCK")) return null;
  if (dialect.kind === "mssql") return withTop(statement, limit, words);
  return `${statement.text}\nLIMIT ${limit}`;
}

/**
 * The whole script with a `LIMIT` put on every `SELECT` that wanted one.
 *
 * Rewritten from the end backwards so that each splice leaves the ranges of the statements before
 * it untouched — the statements were measured against the text as it was, and editing forwards
 * would move every one of them out from under its own coordinates.
 *
 * The editor's own text is never changed by this: what the user wrote is what stays on screen, and
 * this is only what gets sent. `added` is how many statements were touched, so the results can say
 * so rather than quietly showing a shorter answer than was asked for.
 */
export function withAutoLimits(
  sql: string,
  statements: readonly SqlStatement[],
  limit: number,
  dialect: SqlDialect
): { sql: string; added: number } {
  if (limit <= 0) return { sql, added: 0 };
  let out = sql;
  let added = 0;
  for (let i = statements.length - 1; i >= 0; i -= 1) {
    const limited = withLimit(statements[i], limit, dialect);
    if (limited === null) continue;
    out = out.slice(0, statements[i].from) + limited + out.slice(statements[i].to);
    added += 1;
  }
  return { sql: out, added };
}

/**
 * The statements that only read.
 *
 * Written as a list of what is allowed rather than of what is forbidden: MySQL grows statements
 * faster than this list would be updated, and the failure of an allow-list is a read refused,
 * while the failure of a deny-list is a write let through. *
 * `EXPLAIN` is here for the plan it prints, not for the statement it is given: `EXPLAIN ANALYZE`
 * runs that statement, so it is the inner verb that is looked up here — see {@link judged}.
 */
const READ_VERBS = new Set([
  "SELECT", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "USE", "HELP", "CHECKSUM", "WITH",
]);

/** A statement a read-only connection must not send, and the word that gives it away. */
export interface BlockedWrite {
  statement: SqlStatement;
  /** What to name in the refusal: the opening keyword, the writing word found inside a `WITH`, or
   *  `INTO OUTFILE` for a `SELECT` whose result goes to a file rather than to the screen. Naming
   *  the statement's own verb there would say "the script holds a SELECT", which explains nothing. */
  verb: string;
}

/**
 * The statements in the script that a read-only connection must not send.
 *
 * Everything is judged by its opening word, which is what the splitter already has — with three
 * exceptions, all of them statements that open with a reading word and go on to write anyway. A
 * `WITH` may lead into an `UPDATE` or a `DELETE`, and is refused if any writing word appears in it
 * at all; that is stricter than it needs to be, and being too strict here costs a query the user
 * can run by turning the flag off. An `EXPLAIN ANALYZE` runs the statement it is given, so it is
 * refused for whatever that statement is. A `SELECT ... INTO OUTFILE` — or a `WITH` that leads
 * into one — reads no more than any other `SELECT` but leaves a file on the server's disk, which is
 * exactly the kind of mark on a machine this flag is set to prevent.
 */
export function writingStatements(
  statements: readonly SqlStatement[],
  dialect: SqlDialect
): BlockedWrite[] {
  const blocked: BlockedWrite[] = [];
  for (const statement of statements) {
    if (!READ_VERBS.has(statement.verb)) {
      blocked.push({ statement, verb: statement.verb });
      continue;
    }
    // What an `EXPLAIN ANALYZE` is given, for everything else the statement itself — see
    // {@link judged}. The refusal names the inner verb, which is the word that does the damage.
    const { verb, words } = judged(statement, dialect);
    if (!READ_VERBS.has(verb)) {
      blocked.push({ statement, verb: verb === "" ? statement.verb : verb });
      continue;
    }

    if (verb === "WITH") {
      const writing = words.find(({ word }) => WRITING_WORDS.has(word));
      if (writing) {
        blocked.push({ statement, verb: writing.word });
        continue;
      }
      // A `WITH` that leads into a plain `SELECT` still falls through to the file test below: it
      // is a `SELECT` in every way that matters here, and `INTO OUTFILE` writes a file whichever
      // word the statement happens to open with.
    }

    // `INTO @variable` is not this: it puts the answer in the session and leaves nothing behind.
    const into = words.findIndex(({ word }) => word === "INTO");
    const target = into >= 0 ? words[into + 1]?.word : undefined;
    if (target === "OUTFILE" || target === "DUMPFILE") {
      blocked.push({ statement, verb: `INTO ${target}` });
      continue;
    }
    // PostgreSQL has no `OUTFILE`, and no session variables to select into either: at the top level
    // of a statement its `INTO` is `SELECT ... INTO t`, which is `CREATE TABLE AS` in another
    // spelling. That leaves a table behind on the server, which is exactly what this flag is set to
    // prevent.
    if (into >= 0 && dialect.kind === "postgres") {
      blocked.push({ statement, verb: "SELECT INTO" });
    }
  }
  return blocked;
}
