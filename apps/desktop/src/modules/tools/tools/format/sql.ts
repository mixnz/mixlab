/**
 * Collapses an SQL statement onto one line.
 *
 * Not a parser, but it **has to understand strings and comments**: collapsing whitespace inside
 * `'…'` changes the data, and dropping half a `-- …` line turns the tail of the statement into a
 * comment.
 */

const WS = " \t\n\r";

export function minifySql(text: string): string {
  const out: string[] = [];
  let i = 0;
  let pending = false;

  const emit = (chunk: string): void => {
    if (pending && out.length > 0) out.push(" ");
    pending = false;
    out.push(chunk);
  };

  /** Reads a whole string or quoted identifier, including a nested `''`. */
  const readQuoted = (quote: string): string => {
    const start = i;
    i += 1;
    while (i < text.length) {
      if (text[i] === "\\" && quote !== "`") {
        i += 2;
        continue;
      }
      if (text[i] === quote) {
        if (text[i + 1] === quote) {
          i += 2;
          continue;
        }
        i += 1;
        return text.slice(start, i);
      }
      i += 1;
    }
    // Not closed: return whatever is left. Minify has no business refusing a statement.
    return text.slice(start, i);
  };

  while (i < text.length) {
    const ch = text[i]!;
    if (ch === "'" || ch === '"' || ch === "`") {
      emit(readQuoted(ch));
    } else if (text.startsWith("--", i) || ch === "#") {
      const end = text.indexOf("\n", i);
      i = end === -1 ? text.length : end;
      pending = true;
    } else if (text.startsWith("/*", i)) {
      const end = text.indexOf("*/", i + 2);
      i = end === -1 ? text.length : end + 2;
      pending = true;
    } else if (WS.includes(ch)) {
      i += 1;
      pending = true;
    } else {
      emit(ch);
      i += 1;
    }
  }

  return out.join("").trim();
}
