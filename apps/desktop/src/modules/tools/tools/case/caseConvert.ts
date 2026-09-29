export type CaseStyle = "camel" | "snake" | "kebab" | "pascal" | "constant" | "dot" | "title";

export const CASE_STYLES: CaseStyle[] = [
  "camel",
  "snake",
  "kebab",
  "pascal",
  "constant",
  "dot",
  "title",
];

/**
 * A string split into lowercase words.
 *
 * Every character class here is a Unicode property rather than `a-zA-Z`: `\p{Ll}` is lowercase,
 * `\p{Lu}` is uppercase, `\p{L}` is any letter and `\p{N}` is a digit. An `a-zA-Z` table treats
 * every accented letter as a separator, so `"có gì hot"` gives `c_g_hot` and `"Xin chào bạn"` gives
 * five pieces — Vietnamese letters vanishing from the very name being converted.
 *
 * `\p{L}` also accepts Han, Cyrillic, Greek… They have no notion of upper/lower case, so they
 * simply pass through intact, which is the only sensible thing to do with them.
 *
 * The order of the three replacements matters and is locked down by tests:
 *
 * 1. Insert a space before digits — `user2FA` becomes `user 2FA`, not `user2 FA`.
 * 2. Split lower-then-upper — `fooBar` becomes `foo Bar`. Deliberately does **not** accept a digit
 *    on the left side, otherwise the `2FA` just put together would be torn apart at once.
 * 3. Split an uppercase run from the word following it — `HTTPResponse` becomes `HTTP Response`,
 *    which a naive lower/upper split turns into `h_t_t_p_response`.
 */
export function splitWords(input: string): string[] {
  return input
    .replace(/(\p{L})(\p{N})/gu, "$1 $2")
    .replace(/(\p{Ll})(\p{Lu})/gu, "$1 $2")
    .replace(/(\p{Lu}+)(\p{Lu}\p{Ll})/gu, "$1 $2")
    .split(/[^\p{L}\p{N}]+/u)
    .filter((word) => word !== "")
    .map((word) => word.toLowerCase());
}

const upperFirst = (word: string) => word.charAt(0).toUpperCase() + word.slice(1);

/** One line converted to one style. A line with no words is returned as is — wiping a line the
 *  user just pasted is losing data, even if it is only a blank line. */
export function convert(line: string, style: CaseStyle): string {
  const words = splitWords(line);
  if (words.length === 0) return line;

  switch (style) {
    case "camel":
      return words[0] + words.slice(1).map(upperFirst).join("");
    case "snake":
      return words.join("_");
    case "kebab":
      return words.join("-");
    case "pascal":
      return words.map(upperFirst).join("");
    case "constant":
      return words.join("_").toUpperCase();
    case "dot":
      return words.join(".");
    case "title":
      return words.map(upperFirst).join(" ");
  }
}
