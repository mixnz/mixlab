// What the server needs a cipher for, which is not much: it holds a verifier, hands out tokens,
// and addresses an object. **Nothing here touches a record.** The keys that make a record readable
// are derived on the machine that wrote it and never arrive.

const encoder = new TextEncoder();

function hex(bytes: ArrayBuffer | Uint8Array): string {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  return [...view].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** 32 random bytes as hex: a session token, a device id. Never something a person types. */
export function randomToken(): string {
  return hex(crypto.getRandomValues(new Uint8Array(32)));
}

/** 16 random bytes as hex: an account's id on the wire (`accountId`), never reused. */
export function randomPublicId(): string {
  return hex(crypto.getRandomValues(new Uint8Array(16)));
}

/**
 * Crockford base32: the alphabet without `I`, `L`, `O` and `U`, so nothing read off a screen is
 * ambiguous. The same one the recovery key uses (D2), so a person learns one way of typing a code
 * from this product rather than two.
 */
const BASE32 = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_CHARS = 8;

/** A code a person types, shown as `XXXX-XXXX`. Forty bits, which is why guessing is rate limited. */
export function randomCode(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(CODE_CHARS));
  const code = [...bytes].map((byte) => BASE32[byte % 32]).join("");
  return `${code.slice(0, 4)}-${code.slice(4)}`;
}

/**
 * Forgiving about case and separators, strict about the alphabet — the rule `parse_recovery_key`
 * already applies on the client. A character outside the alphabet means the person has the wrong
 * thing in front of them and should be told so.
 */
export function normaliseCode(presented: string): string | null {
  const stripped = presented.toUpperCase().replace(/[\s-]/g, "");
  if (stripped.length !== CODE_CHARS) return null;
  return [...stripped].every((character) => BASE32.includes(character)) ? stripped : null;
}

/** How many bytes a salt is, fixed so that an invented one cannot be told apart by length. */
export const SALT_BYTES = 16;

/** HKDF-SHA256 output (D2). */
export const VERIFIER_BYTES = 32;

/** XChaCha20-Poly1305 over a 32-byte key: 24 nonce, 32 sealed, 16 tag (D2). */
export const WRAPPED_KEY_BYTES = 72;

/**
 * The salt handed back for an address that has no account (D4a).
 *
 * Stable, so asking twice gives the same answer; unguessable, because the pepper never leaves this
 * deployment; and the same shape as a real one. Without all three, `/v1/auth/params` is the
 * cheapest account-enumeration oracle in the protocol.
 */
export async function inventedSalt(pepper: string, accountKey: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(pepper),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = new Uint8Array(
    await crypto.subtle.sign("HMAC", key, encoder.encode(`salt/v1\0${accountKey}`)),
  );
  return btoa(String.fromCharCode(...mac.slice(0, SALT_BYTES)));
}

export async function sha256Hex(value: string): Promise<string> {
  return hex(await crypto.subtle.digest("SHA-256", encoder.encode(value)));
}

/**
 * The stored password verifier: `HMAC(pepper, A)`. `A` already is a verifier — the client derived
 * it and the password is not recoverable from it — so this is not about hashing a password. It is
 * about the pepper living outside the database, so that a database taken on its own is not a list
 * of values that can be replayed against this server.
 */
export async function peppered(pepper: string, a: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(pepper),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return hex(await crypto.subtle.sign("HMAC", key, encoder.encode(a)));
}

/**
 * Comparison that does not leak where two values first differ. Both arguments here are already
 * hashes of the same fixed length, so the early return costs nothing that matters — but a verifier
 * comparison is exactly the place where "it was fine in practice" stops being an argument.
 */
export function sameSecret(left: string, right: string): boolean {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= left.charCodeAt(index) ^ right.charCodeAt(index);
  }
  return difference === 0;
}

/**
 * The only name an account has (D4a):
 *
 *     account_key = SHA-256("mixlab-sync/account/v1" || 0x00 || lowercase(trim(email)))
 *
 * **Frozen, and deployment-independent on purpose.** It is the name of the Durable Object here and
 * the unique key of the row in `../native/`, so the same address is the same account on either.
 * It carries no pepper: a peppered value could not mean the same thing on two servers, which is
 * the whole point of it.
 *
 * The label is frozen the way D2's five HKDF labels are. Changing it corrupts nothing; it makes
 * every existing account unfindable.
 */
export async function accountKey(email: string): Promise<string> {
  const encoded = encoder.encode(email.trim().toLowerCase());
  const labelled = new Uint8Array(ACCOUNT_LABEL.length + 1 + encoded.length);
  labelled.set(ACCOUNT_LABEL, 0);
  labelled[ACCOUNT_LABEL.length] = 0;
  labelled.set(encoded, ACCOUNT_LABEL.length + 1);
  return hex(await crypto.subtle.digest("SHA-256", labelled));
}

const ACCOUNT_LABEL = new TextEncoder().encode("mixlab-sync/account/v1");
