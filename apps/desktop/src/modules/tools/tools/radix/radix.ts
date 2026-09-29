/**
 * Converts between binary, octal, decimal and hexadecimal — with `bigint` throughout, never
 * `Number`.
 *
 * bigint or snowflake-style IDs in a DB often exceed `Number.MAX_SAFE_INTEGER`; `parseInt`/`Number`
 * round wrongly there without reporting an error. `bigint` has no ceiling.
 */

export type Base = "bin" | "oct" | "dec" | "hex";

export const BASES: Base[] = ["bin", "oct", "dec", "hex"];

const RADIX: Record<Base, number> = { bin: 2, oct: 8, dec: 10, hex: 16 };

const CHARSET: Record<Base, RegExp> = {
  bin: /^[01]+$/,
  oct: /^[0-7]+$/,
  dec: /^[0-9]+$/,
  hex: /^[0-9a-fA-F]+$/,
};

const PREFIX: Record<Exclude<Base, "dec">, RegExp> = {
  bin: /^0[bB]/,
  oct: /^0[oO]/,
  hex: /^0[xX]/,
};

const HEX_DIGITS = "0123456789abcdef";

/** `BigInt(string)` does not accept a prefix and a minus sign together, so this accumulates by
 *  radix itself. */
function magnitudeToBigInt(digits: string, base: Base): bigint {
  const radix = BigInt(RADIX[base]);
  let value = 0n;
  for (const ch of digits.toLowerCase()) {
    value = value * radix + BigInt(HEX_DIGITS.indexOf(ch));
  }
  return value;
}

/** Looks at the prefix (`0x`/`0b`/`0o`) first; decimal is the default base when no prefix
 *  matches. */
export function detectBase(input: string): Base | null {
  const trimmed = input.trim();
  const body = trimmed.startsWith("-") ? trimmed.slice(1) : trimmed;
  if (body === "") return null;
  if (PREFIX.hex.test(body) && CHARSET.hex.test(body.slice(2))) return "hex";
  if (PREFIX.bin.test(body) && CHARSET.bin.test(body.slice(2))) return "bin";
  if (PREFIX.oct.test(body) && CHARSET.oct.test(body.slice(2))) return "oct";
  if (CHARSET.dec.test(body)) return "dec";
  return null;
}

/** Negative numbers are only read in decimal — the other bases have no two's complement convention
 *  here. */
export function parseValue(input: string, base: Base): bigint | null {
  const trimmed = input.trim();
  if (trimmed === "" || trimmed === "-") return null;
  const negative = trimmed.startsWith("-");
  const rest = negative ? trimmed.slice(1) : trimmed;
  if (negative && base !== "dec") return null;
  const digits = base === "dec" ? rest : rest.replace(PREFIX[base as Exclude<Base, "dec">] ?? /^$/, "");
  if (digits === "" || !CHARSET[base].test(digits)) return null;
  const magnitude = magnitudeToBigInt(digits, base);
  return negative ? -magnitude : magnitude;
}

export interface RadixOutputs {
  bin: string;
  oct: string;
  dec: string;
  hex: string;
}

/** Groups of 4 characters counted from the right; the remainder (if any) is the first group on the
 *  left. */
function groupBinary(digits: string): string {
  const chunks: string[] = [];
  let end = digits.length;
  while (end > 0) {
    const start = Math.max(0, end - 4);
    chunks.unshift(digits.slice(start, end));
    end = start;
  }
  return chunks.join(" ");
}

export function formatOutputs(value: bigint): RadixOutputs {
  const negative = value < 0n;
  const abs = negative ? -value : value;
  const sign = negative ? "-" : "";
  return {
    bin: sign + groupBinary(abs.toString(2)),
    oct: sign + abs.toString(8),
    dec: value.toString(10),
    hex: sign + "0x" + abs.toString(16),
  };
}
