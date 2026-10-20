/**
 * Carving the Query tab's script into the statements it is made of.
 *
 * This is a port of `split_statements` in `src-tauri/src/modules/db/drivers/sql/statements.rs`,
 * which is what actually runs a script. The backend splits so it can send one statement at a time;
 * the editor splits so it can say which statement the caret is in, run that one alone, and draw it.
 * They must agree — a statement the editor highlights and sends has to be the same one the server
 * ends up running — **so a change to either splitter belongs in the same commit as the other**, and
 * in both sets of tests: [statements.test.ts](./statements.test.ts) here, `mod tests` there.
 *
 * One splitter serves every engine: what differs between them is lexical, and travels in a
 * {@link SqlSyntax}. The only thing this port adds over the Rust one is where each statement sits
 * in the text. ClickHouse's backend reads its script with a scanner of its own.
 */

import { dollarTag, opensEscapeString, type SqlSyntax } from "./syntax";

/** One statement, and the range of the script it came from. */
export interface SqlStatement {
  /** The statement as written, trimmed. Carries its comments — a `/*!50000 ... *\/` is part of it. */
  text: string;
  /** The keyword it opens with, upper-cased. */
  verb: string;
  /** Where the trimmed text starts in the script. */
  from: number;
  /** Where it ends. The terminating semicolon is not included. */
  to: number;
}

/** Whether the character can be part of the opening keyword. Mirrors Rust's `is_alphanumeric`
 *  closely enough for a keyword: what follows it is only ever compared against ASCII verbs. */
function isWordChar(c: string): boolean {
  return /[\p{L}\p{N}_]/u.test(c);
}

/** A line holding nothing but `GO`, optionally with a repeat count — case-insensitive, and the
 *  count may carry trailing spaces of its own. Anchored at the very start of what it is tested
 *  against, since callers always slice from the candidate position. */
const GO_SEPARATOR = /^go\b[ \t]*(?:[0-9]+[ \t]*)?\r?(?:\n|$)/i;

/**
 * If a `GO` batch separator begins at `sql[i]`, the index right after its line — so the caller can
 * skip straight past it, delimiter and all. Null when this is not one.
 *
 * A separator needs the whole of its line to itself but for whitespace, checked on both sides:
 * `\bgo\b` alone rejects `GOOD` and `GO3` (no true word boundary either side of the run), but not
 * `SELECT 1 GO` — something other than whitespace already sits between the last newline and here,
 * which is checked separately since a regex anchored at `i` cannot see backwards.
 */
function matchBatchSeparator(sql: string, i: number): number | null {
  const lineStart = sql.lastIndexOf("\n", i - 1) + 1;
  if (/\S/.test(sql.slice(lineStart, i))) return null;
  const m = GO_SEPARATOR.exec(sql.slice(i));
  return m ? i + m[0].length : null;
}

/**
 * Splits a script into its statements.
 *
 * Only a semicolon outside a string, a quoted identifier, a comment and — where the engine has
 * them — a dollar-quoted body separates two. MySQL's client-side `DELIMITER` directive is not
 * supported here any more than it is in the backend: a routine body whose `BEGIN ... END` holds
 * semicolons of its own reads as several statements, and has to be run as the one statement it is.
 * PostgreSQL needs no such directive, its function bodies being dollar-quoted.
 */
export function splitStatements(sql: string, syntax: SqlSyntax): SqlStatement[] {
  const statements: SqlStatement[] = [];
  let chunkStart = 0;
  let verb = "";
  // Set once the opening word has ended, so the words after it can't overwrite it.
  let verbDone = false;
  let i = 0;

  function closeChunk(end: number) {
    const start = chunkStart;
    const opening = verb;
    verb = "";
    verbDone = false;
    // No opening keyword means there was no statement here — only whitespace, or a comment
    // sitting between two separators.
    if (opening === "") return;
    const raw = sql.slice(start, end);
    const leading = raw.length - raw.trimStart().length;
    const trimmed = raw.trim();
    statements.push({
      text: trimmed,
      verb: opening,
      from: start + leading,
      to: start + leading + trimmed.length,
    });
  }

  function push(end: number) {
    closeChunk(end);
    // The next chunk opens after the delimiter this one ended on — a single character, `;`.
    chunkStart = end + 1;
  }

  while (i < sql.length) {
    const c = sql[i];

    // On MySQL `--` opens a comment only when whitespace (or the end of the text) follows it, so
    // that `5--3` stays arithmetic. PostgreSQL has no such rule.
    if (
      c === "-" &&
      sql[i + 1] === "-" &&
      (!syntax.dashCommentNeedsSpace || i + 2 >= sql.length || " \t\n\r".includes(sql[i + 2]))
    ) {
      while (i < sql.length && sql[i] !== "\n") i += 1;
      continue;
    }
    if (c === "#" && syntax.hashComments) {
      while (i < sql.length && sql[i] !== "\n") i += 1;
      continue;
    }
    if (c === "/" && sql[i + 1] === "*") {
      let depth = 0;
      while (i < sql.length) {
        if (sql[i] === "/" && sql[i + 1] === "*") {
          depth += 1;
          i += 2;
          continue;
        }
        if (sql[i] === "*" && sql[i + 1] === "/") {
          depth -= 1;
          i += 2;
          // Where comments do not nest the first close ends it, whatever the count says.
          if (depth === 0 || !syntax.nestedBlockComments) break;
          continue;
        }
        i += 1;
      }
      continue;
    }

    // A dollar-quoted body ends only at its own tag, so `$$ ... ; ... $$` is one statement however
    // many semicolons it holds.
    if (syntax.dollarQuoting) {
      const tag = dollarTag(sql, i);
      if (tag !== null) {
        const close = `$${tag}$`;
        i += close.length;
        const end = sql.indexOf(close, i);
        i = end === -1 ? sql.length : end + close.length;
        if (verb !== "") verbDone = true;
        continue;
      }
    }

    const openQuote = syntax.identifierQuote?.open;
    if (c === "'" || c === '"' || c === openQuote) {
      const isIdent = c === openQuote;
      // The character that closes this run — the same one it opened with, unless it opened an
      // identifier quote whose close differs, like SQL Server's `[` ... `]`.
      const closeChar = isIdent ? syntax.identifierQuote!.close : c;
      /* Whether a backslash escapes inside *this* literal. On MySQL it always does; on PostgreSQL
         only in an `E'...'`, whose backslashes are the reason the split has to know. Inside a
         quoted identifier it never does on either — there, doubling is the only escape. */
      const escapes =
        !isIdent &&
        (syntax.backslashEscapes ||
          (syntax.escapeStringPrefix && c === "'" && opensEscapeString(sql, i)));
      i += 1;
      while (i < sql.length) {
        const ch = sql[i];
        i += 1;
        if (ch === "\\" && escapes) {
          i += 1;
          continue;
        }
        if (ch === closeChar) {
          // Two of the close quote in a row are an escaped one, not the end of the literal.
          if (sql[i] === closeChar) {
            i += 1;
            continue;
          }
          break;
        }
      }
      continue;
    }

    if (c === ";") {
      push(i);
      i += 1;
      continue;
    }

    // SQL Server's `GO` — a client-side batch separator, never sent to the server as text.
    // Checked only when the dialect has one at all, and only on the character it could possibly
    // start with, so this never touches the four engines without one.
    if (syntax.batchSeparator && (c === "g" || c === "G")) {
      const end = matchBatchSeparator(sql, i);
      if (end !== null) {
        closeChunk(i);
        chunkStart = end;
        i = end;
        continue;
      }
    }

    // Plain code: the first word of it is the statement's keyword.
    if (!verbDone) {
      if (isWordChar(c)) verb += c.toUpperCase();
      else if (verb !== "") verbDone = true;
    }
    i += 1;
  }

  push(sql.length);
  return statements;
}

/**
 * The statement the caret is working on, or null when the script holds none.
 *
 * Inside a statement it is that statement. Past the end of one — the caret left sitting after the
 * semicolon it just typed — it is still that statement, until a blank line separates the two: past
 * one of those, the caret has moved on to whatever comes next.
 */
export function statementAt(
  sql: string,
  statements: SqlStatement[],
  pos: number
): SqlStatement | null {
  if (statements.length === 0) return null;

  let previous: SqlStatement | null = null;
  let next: SqlStatement | null = null;
  for (const statement of statements) {
    if (pos >= statement.from && pos <= statement.to) return statement;
    if (statement.to < pos) previous = statement;
    else if (next === null) next = statement;
  }

  if (previous === null) return next;
  if (next === null) return previous;
  // A blank line is what says the caret has left the statement above it. Anything less — a
  // newline, the space after a semicolon — and it is still that statement's line of work.
  return /\n\s*\n/.test(sql.slice(previous.to, pos)) ? next : previous;
}

/** The verbs that change what completion knows: after one of these the schema outline is stale. */
const DDL_VERBS = new Set(["CREATE", "ALTER", "DROP", "RENAME", "TRUNCATE"]);

/** Whether running this script could have changed the shape of the database. */
export function changesSchema(statements: SqlStatement[]): boolean {
  return statements.some((statement) => DDL_VERBS.has(statement.verb));
}
