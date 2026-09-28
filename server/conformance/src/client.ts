// The whole of what the suite needs to talk to a server, and deliberately nothing more.
//
// **There is no cryptography here, and that is the point.** The server never inspects a ciphertext
// (D1), so every opaque field it receives — `collection`, `id`, `nonce`, `ciphertext`, `a`,
// `saltAccount`, `wrappedMk*` — is random bytes of the right length and nothing else. A suite that
// had to reproduce the key hierarchy in order to exercise the server would be exercising something
// the server is not allowed to do, and would pass against a server that had learned to do it.

import { randomBytes } from "node:crypto";

export interface Capabilities {
  protocolVersions: string[];
  maxRecordBytes: number;
  maxBatchOperations: number;
  maxBatchBytes: number;
  maxPageRecords: number;
  accountQuotaBytes: number;
  tombstoneRetentionDays: number;
  /** When the operator intends to switch this server off, or null. Advisory (D4a). */
  closingOn: number | null;
  features: string[];
}

export interface ErrorBody {
  error: { code: string; message: string; [member: string]: unknown };
}

export interface StoredRecord {
  collection: string;
  id: string;
  version: number;
  seq: number;
  updatedAt: number;
  deleted: boolean;
  /** The device whose session wrote it, stamped by the server (D3). D4's tie-break reads it. */
  device: string;
  nonce?: string;
  ciphertext?: string;
}

export interface Page {
  records: StoredRecord[];
  nextSince: number;
  more: boolean;
}

export interface Result<T> {
  status: number;
  headers: Headers;
  body: T;
}

/** The address of the server under test. A missing one is a mistake, not a reason to skip. */
export function baseUrl(): string {
  const url = process.env.CONFORMANCE_BASE_URL;
  if (!url) {
    throw new Error(
      "CONFORMANCE_BASE_URL is unset. This suite tests a running server; point it at one:\n" +
        "  CONFORMANCE_BASE_URL=http://127.0.0.1:8787 npm test",
    );
  }
  return url.replace(/\/+$/, "");
}

/**
 * A deployment somebody runs for their own company may be closed with a shared token (D4a). The
 * suite carries it on every request when it has one, and the hosted instances never ask for one.
 */
export function accessHeaders(): Record<string, string> {
  const token = process.env.CONFORMANCE_ACCESS_TOKEN;
  return token ? { "X-MixLab-Access": token } : {};
}

export interface CallOptions {
  method?: string;
  body?: unknown;
  token?: string;
  headers?: Record<string, string>;
}

export async function call<T = unknown>(path: string, options: CallOptions = {}): Promise<Result<T>> {
  const headers: Record<string, string> = { ...accessHeaders(), ...options.headers };
  if (options.body !== undefined) headers["Content-Type"] = "application/json";
  if (options.token) headers["Authorization"] = `Bearer ${options.token}`;

  const response = await fetch(`${baseUrl()}${path}`, {
    method: options.method ?? (options.body === undefined ? "GET" : "POST"),
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });

  const type = response.headers.get("content-type") ?? "";
  const body = type.includes("json") ? await response.json() : await response.text();
  return { status: response.status, headers: response.headers, body: body as T };
}

/** A 64-character lowercase hex id, the shape D3 gives a collection and a record. */
export function opaqueId(): string {
  return randomBytes(32).toString("hex");
}

/** Standard base64 with padding, which is the one spelling D4a allows. */
export function base64Bytes(count: number): string {
  return randomBytes(count).toString("base64");
}

/**
 * `.invalid` is reserved by RFC 2606 and resolves nowhere, so a suite that is accidentally pointed
 * at a server with a real mail provider configured cannot deliver to a stranger.
 */
export function newEmail(): string {
  return `conformance-${randomBytes(9).toString("hex")}@example.invalid`;
}

export interface Account {
  email: string;
  a: string;
  saltAccount: string;
  wrappedMkPassword: string;
  wrappedMkRecovery: string;
}

export function newAccount(overrides: Partial<Account> = {}): Account {
  return {
    email: newEmail(),
    a: base64Bytes(32),
    saltAccount: base64Bytes(16),
    // A wrapped 32-byte key under XChaCha20-Poly1305 is 24 + 32 + 16 bytes. The server stores it
    // and never opens it, so only the length is meaningful here.
    wrappedMkPassword: base64Bytes(72),
    wrappedMkRecovery: base64Bytes(72),
    ...overrides,
  };
}

export const ARGON = { m: 65536, t: 3, p: 4 } as const;

export function registerBody(account: Account): Record<string, unknown> {
  return {
    email: account.email,
    a: account.a,
    saltAccount: account.saltAccount,
    argon: ARGON,
    wrappedMkPassword: account.wrappedMkPassword,
    wrappedMkRecovery: account.wrappedMkRecovery,
  };
}

export interface OutboxMessage {
  kind: "verification" | "reset";
  token: string;
  sentAt: number;
}

export async function outbox(email: string): Promise<OutboxMessage[]> {
  const result = await call<{ messages: OutboxMessage[] }>(
    `/__test__/outbox?email=${encodeURIComponent(email)}`,
  );
  if (result.status === 404) {
    throw new Error(
      "The server under test does not serve /__test__/outbox. The suite cannot read an email, so " +
        "verification and reset are unreachable without it — start the server with its test " +
        "outbox enabled (D4a).",
    );
  }
  if (result.status !== 200) throw new Error(`/__test__/outbox answered ${result.status}`);
  return result.body.messages;
}

/** The newest token of a kind, which is the last one: the outbox is oldest first. */
export async function latestToken(email: string, kind: OutboxMessage["kind"]): Promise<string> {
  const messages = await outbox(email);
  const match = messages.filter((message) => message.kind === kind).at(-1);
  if (!match) throw new Error(`no ${kind} message for ${email}; outbox holds ${messages.length}`);
  return match.token;
}

export interface Session {
  accessToken: string;
  refreshToken: string;
  deviceId: string;
  expiresIn: number;
  /** The account's own id: random at registration, never reused (T178c, C4). */
  accountId: string;
}

export async function register(account: Account): Promise<Result<unknown>> {
  return call("/v1/auth/register", { body: registerBody(account) });
}

export async function verify(account: Account): Promise<Result<unknown>> {
  const code = await latestToken(account.email, "verification");
  return call("/v1/auth/verify", { body: { email: account.email, token: code } });
}

export async function login(account: Account, deviceName = "conformance"): Promise<Session> {
  const result = await call<Session>("/v1/auth/login", {
    body: { email: account.email, a: account.a, deviceName },
  });
  if (result.status !== 200) {
    throw new Error(`login answered ${result.status}: ${JSON.stringify(result.body)}`);
  }
  return result.body;
}

/** Register, verify, sign in. What most tests want before they can say anything interesting. */
export async function signedUp(deviceName = "conformance"): Promise<{ account: Account; session: Session }> {
  const account = newAccount();
  const registered = await register(account);
  if (registered.status !== 201) {
    throw new Error(`register answered ${registered.status}: ${JSON.stringify(registered.body)}`);
  }
  await verify(account);
  return { account, session: await login(account, deviceName) };
}

export interface RecordBody {
  updatedAt: number;
  nonce: string;
  ciphertext: string;
}

export function newRecord(ciphertextBytes = 64): RecordBody {
  return {
    updatedAt: Math.floor(Date.now() / 1000),
    nonce: base64Bytes(24),
    ciphertext: base64Bytes(ciphertextBytes),
  };
}

export function put(
  token: string,
  collection: string,
  id: string,
  body: RecordBody,
  precondition: { ifMatch?: number; ifNoneMatch?: boolean },
): Promise<Result<StoredRecord & Partial<ErrorBody>>> {
  const headers: Record<string, string> = {};
  if (precondition.ifNoneMatch) headers["If-None-Match"] = "*";
  if (precondition.ifMatch !== undefined) headers["If-Match"] = `"${precondition.ifMatch}"`;
  return call(`/v1/records/${collection}/${id}`, { method: "PUT", body, token, headers });
}

/** Seconds since the epoch, as `updatedAt` is on the wire. */
export function seconds(): number {
  return Math.floor(Date.now() / 1000);
}

/** `updatedAt` is when the deletion was made; `null` sends no body at all. */
export function remove(
  token: string,
  collection: string,
  id: string,
  ifMatch?: number,
  updatedAt: number | null = seconds(),
): Promise<Result<StoredRecord & Partial<ErrorBody>>> {
  const headers: Record<string, string> = {};
  if (ifMatch !== undefined) headers["If-Match"] = `"${ifMatch}"`;
  const body = updatedAt === null ? undefined : { updatedAt };
  return call(`/v1/records/${collection}/${id}`, { method: "DELETE", token, headers, body });
}

export function since(
  token: string,
  from: number,
  collection?: string,
  options: { resync?: boolean } = {},
): Promise<Result<Page & Partial<ErrorBody>>> {
  const query = new URLSearchParams({ since: String(from) });
  if (collection) query.set("collection", collection);
  if (options.resync) query.set("resync", "1");
  return call(`/v1/records?${query}`, { token });
}

/** `POST /v1/records/heads` (T189). */
export interface Heads {
  stale: string[];
  nextSince: number;
}

/** `cursors` is `unknown` so a test can send what a client never should. */
export function heads(token: string, cursors: unknown): Promise<Result<Heads & Partial<ErrorBody>>> {
  return call("/v1/records/heads", { body: { cursors }, token });
}

/** Create one record and hand back everything a test needs to keep working with it. */
export async function seed(
  token: string,
  ciphertextBytes = 64,
): Promise<{ collection: string; id: string; stored: StoredRecord }> {
  const collection = opaqueId();
  const id = opaqueId();
  const result = await put(token, collection, id, newRecord(ciphertextBytes), { ifNoneMatch: true });
  if (result.status !== 201) {
    throw new Error(`seed answered ${result.status}: ${JSON.stringify(result.body)}`);
  }
  return { collection, id, stored: result.body };
}
