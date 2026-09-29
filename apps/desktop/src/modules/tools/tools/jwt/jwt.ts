import { base64ToText } from "../encode/encode";

export interface JwtParts {
  header: unknown;
  payload: unknown;
  /** As is, not decoded. It is not verified — see the comment on `decodeJwt`. */
  signature: string;
}

export type JwtResult =
  | { ok: true; parts: JwtParts }
  | { ok: false; reason: "shape" | "base64" | "json" };

export interface JwtTimes {
  exp?: number;
  iat?: number;
  nbf?: number;
  /** `null` when the payload has no `exp` — "unknown" is different from "not expired". */
  expired: boolean | null;
}

/**
 * Splits a JWT into its three parts and reads the first two.
 *
 * Returns a result instead of throwing: three reasons for failure need three different sentences
 * on screen, and one shared `catch` could not tell broken base64 from broken JSON.
 *
 * **The signature is not checked.** That needs the secret, and a wrong "valid" is more dangerous
 * than silence.
 */
export function decodeJwt(token: string): JwtResult {
  const parts = token.trim().split(".");
  if (parts.length !== 3 || parts.some((part) => part === "")) return { ok: false, reason: "shape" };

  let headerText: string;
  let payloadText: string;
  try {
    headerText = base64ToText(parts[0]);
    payloadText = base64ToText(parts[1]);
  } catch {
    return { ok: false, reason: "base64" };
  }

  try {
    return {
      ok: true,
      parts: {
        header: JSON.parse(headerText),
        payload: JSON.parse(payloadText),
        signature: parts[2],
      },
    };
  } catch {
    return { ok: false, reason: "json" };
  }
}

const numberClaim = (payload: Record<string, unknown>, key: string): number | undefined =>
  typeof payload[key] === "number" ? (payload[key] as number) : undefined;

/** `now` is in milliseconds; a JWT's `exp`/`iat`/`nbf` are in seconds. */
export function claimTimes(payload: unknown, now: number): JwtTimes {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return { expired: null };
  }
  const claims = payload as Record<string, unknown>;
  const exp = numberClaim(claims, "exp");
  return {
    exp,
    iat: numberClaim(claims, "iat"),
    nbf: numberClaim(claims, "nbf"),
    expired: exp === undefined ? null : exp * 1000 < now,
  };
}
