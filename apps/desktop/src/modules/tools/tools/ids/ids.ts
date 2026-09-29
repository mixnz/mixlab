export type IdKind = "uuidv4" | "uuidv7" | "ulid" | "nanoid";

export const ID_KINDS: IdKind[] = ["uuidv4", "uuidv7", "ulid", "nanoid"];

/** The number of random bytes each kind needs. The Panel asks `crypto.getRandomValues` for exactly
 *  this many. */
export const RANDOM_BYTES: Record<IdKind, number> = {
  uuidv4: 16,
  uuidv7: 10,
  ulid: 16,
  nanoid: 21,
};

const hex = (b: Uint8Array) => Array.from(b, (n) => n.toString(16).padStart(2, "0")).join("");

const dash = (h: string) =>
  `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20, 32)}`;

/*
 * Every function here takes `now` and `rnd` as parameters instead of calling `Date.now()` and
 * `crypto.getRandomValues()` itself. That is why they can be tested — and also why the tests can
 * assert the most important property of v7 and ULID: sorting by string is sorting by time.
 */

export function uuidv4(rnd: Uint8Array): string {
  const b = rnd.slice(0, 16);
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  return dash(hex(b));
}

export function uuidv7(now: number, rnd: Uint8Array): string {
  const b = new Uint8Array(16);
  // 48 bits of time at the front, big-endian — that is what makes v7 sortable by time.
  for (let i = 0; i < 6; i++) b[i] = Math.floor(now / 2 ** (8 * (5 - i))) & 0xff;
  b.set(rnd.slice(0, 10), 6);
  b[6] = (b[6] & 0x0f) | 0x70;
  b[8] = (b[8] & 0x3f) | 0x80;
  return dash(hex(b));
}

/** Crockford's base32 alphabet: no I, L, O, U — letters easily misread as digits. */
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

export function ulid(now: number, rnd: Uint8Array): string {
  let time = "";
  let left = now;
  for (let i = 0; i < 10; i++) {
    time = CROCKFORD[left % 32] + time;
    left = Math.floor(left / 32);
  }
  // 256 is divisible by 32, so `& 31` on a byte is uniformly distributed — no bias to correct.
  let random = "";
  for (let i = 0; i < 16; i++) random += CROCKFORD[rnd[i] & 31];
  return time + random;
}

const NANO_ALPHABET = "useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";

export function nanoid(rnd: Uint8Array): string {
  let out = "";
  for (let i = 0; i < rnd.length; i++) out += NANO_ALPHABET[rnd[i] & 63];
  return out;
}
