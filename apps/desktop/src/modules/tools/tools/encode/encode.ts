import { md5 } from "./md5";

export type HashAlgo = "MD5" | "SHA-1" | "SHA-256" | "SHA-384" | "SHA-512";

export const HASH_ALGOS: HashAlgo[] = ["MD5", "SHA-1", "SHA-256", "SHA-384", "SHA-512"];

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * `btoa` takes a string whose every character must be in Latin-1, so calling it directly breaks on
 * anything beyond ASCII — Vietnamese included. The detour through `TextEncoder` is required, not
 * excess caution.
 */
export function textToBase64(text: string, urlSafe: boolean): string {
  let binary = "";
  for (const byte of encoder.encode(text)) binary += String.fromCharCode(byte);
  const base64 = btoa(binary);
  return urlSafe ? base64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "") : base64;
}

/** Accepts both plain and url-safe base64, and puts back the padding url-safe dropped. */
export function base64ToText(input: string): string {
  const normalised = input.trim().replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalised + "=".repeat((4 - (normalised.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return decoder.decode(bytes);
}

export function textToHex(text: string, spaced: boolean): string {
  const parts = Array.from(encoder.encode(text), (b) => b.toString(16).padStart(2, "0"));
  return parts.join(spaced ? " " : "");
}

export function hexToText(input: string): string {
  const clean = input.replace(/\s+/g, "");
  if (clean.length % 2 !== 0) throw new Error("odd hex length");
  if (!/^[0-9a-fA-F]*$/.test(clean)) throw new Error("not hex");
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  return decoder.decode(bytes);
}

export async function hashText(text: string, algo: HashAlgo): Promise<string> {
  const bytes = encoder.encode(text);
  if (algo === "MD5") return md5(bytes);
  const digest = await crypto.subtle.digest(algo, bytes);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}
