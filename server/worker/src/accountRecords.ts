// The record routes of one account's object: reading since a cursor, the heads, one write, and
// a batch. Functions of the `Account` they are given rather than its methods, so the class keeps
// the account's own life and this file the records', as `server/native/` keeps them in
// `records.rs`. The table logic they call is `records.ts`.

import type { Account } from "./account";
import type { Config } from "./config";
import { fail, json, notFound } from "./http";
import {
  applyDelete,
  applyPut,
  listSince,
  staleSince,
  type Outcome,
  type Precondition,
} from "./records";
import { asObject } from "./validate";

/** `If-Match: "41"` and `If-None-Match: *`, which are how a write states what it believes. */
function readPrecondition(headers: Headers): Precondition {
  const ifMatch = headers.get("If-Match");
  const parsed = ifMatch === null ? Number.NaN : Number(ifMatch.replace(/"/g, ""));
  return {
    ifMatch: Number.isSafeInteger(parsed) ? parsed : undefined,
    ifNoneMatch: headers.get("If-None-Match") === "*",
  };
}

/**
 * A record answer, successful or not. On a 409 or a 412 the current record travels **beside** the
 * error rather than instead of it: the client is the one that resolves a conflict (D4), so it
 * needs the other side of it in the same answer it was refused by.
 */
function outcome(result: Outcome): Response {
  const body = {
    ...(result.record ?? {}),
    ...(result.error ? { error: result.error } : {}),
  };
  const headers =
    result.record && !result.error ? { ETag: `"${result.record.version}"` } : undefined;
  return json(result.status, body, headers);
}

export async function readRecords(
  object: Account,
  config: Config,
  request: Request,
  url: URL,
): Promise<Response> {
  const session = await object.authenticate(request);
  if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
  // `Number("")` is 0, which would have quietly turned `?since=` into "from the beginning"
  // here while `server/native/` refused it. Digits or nothing.
  const raw = url.searchParams.get("since");
  const since = raw === null ? 0 : /^\d+$/.test(raw) ? Number(raw) : Number.NaN;
  // A flag that turns expiry off takes one spelling, not `true`, `0` or nothing.
  const resync = url.searchParams.get("resync");
  if (resync !== null && resync !== "1") {
    return fail(400, "invalid-request", "`resync` is 1 or absent.");
  }

  const result = listSince(
    object.sql,
    config.capabilities,
    since,
    url.searchParams.get("collection"),
    resync === "1",
  );
  return "records" in result ? json(200, result) : outcome(result);
}

export async function readHeads(
  object: Account,
  config: Config,
  request: Request,
  body: unknown,
): Promise<Response> {
  const session = await object.authenticate(request);
  if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
  // A read, like `GET /v1/records`: a frozen account still answers it (D4b).
  const result = staleSince(object.sql, config.capabilities, body, session.deviceId);
  return "stale" in result ? json(200, result) : outcome(result);
}

export async function writeRecord(
  object: Account,
  config: Config,
  request: Request,
  url: URL,
  body: unknown,
): Promise<Response> {
  const session = await object.authenticate(request);
  if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
  const frozen = object.frozen();
  if (frozen) return frozen;

  const parts = url.pathname.slice("/v1/records/".length).split("/");
  if (parts.length !== 2) return notFound();
  const [collection, id] = parts as [string, string];
  const precondition = readPrecondition(request.headers);

  if (request.method === "PUT") {
    return outcome(
      applyPut(object.sql, config.capabilities, collection, id, body, precondition, session.deviceId),
    );
  }

  const removed = applyDelete(
    object.sql,
    collection,
    id,
    precondition.ifMatch,
    asObject(body)?.["updatedAt"],
    session.deviceId,
  );
  if (removed.status === 200) await object.scheduleReaping(config.capabilities.tombstoneRetentionDays);
  return outcome(removed);
}

export async function batch(
  object: Account,
  config: Config,
  request: Request,
  body: unknown,
): Promise<Response> {
  const session = await object.authenticate(request);
  if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
  const frozen = object.frozen();
  if (frozen) return frozen;

  const operations = asObject(body)?.["operations"];
  const limits = config.capabilities;
  if (
    !Array.isArray(operations) ||
    operations.length === 0 ||
    operations.length > limits.maxBatchOperations
  ) {
    return fail(
      400,
      "invalid-request",
      `A batch needs between 1 and ${limits.maxBatchOperations} operations.`,
    );
  }

  // The envelope refuses only when the account is already full: past that point every entry
  // would fail alike, and answering once is kinder than answering a hundred times.
  const account = object.account();
  if (account && account.stored_bytes > limits.accountQuotaBytes) {
    return fail(507, "quota-exceeded", "This account is full.", {
      limit: limits.accountQuotaBytes,
      used: account.stored_bytes,
    });
  }

  // Independent compare-and-swaps and not a transaction (D4): each entry succeeds or conflicts
  // on its own, and a 409 in entry seven is news for the client rather than a failed request.
  const results = operations.map((operation) => {
    const fields = asObject(operation);
    if (!fields) return { status: 400, error: { code: "invalid-request", message: "Not an operation." } };

    const collection = String(fields["collection"] ?? "");
    const id = String(fields["id"] ?? "");
    const ifMatch = typeof fields["ifMatch"] === "number" ? fields["ifMatch"] : undefined;

    if (fields["op"] === "delete") {
      return applyDelete(object.sql, collection, id, ifMatch, fields["updatedAt"], session.deviceId);
    }
    if (fields["op"] === "put") {
      return applyPut(
        object.sql,
        limits,
        collection,
        id,
        fields["record"],
        { ifMatch, ifNoneMatch: fields["ifNoneMatch"] === true },
        session.deviceId,
      );
    }
    return { status: 400, error: { code: "invalid-request", message: "`op` is put or delete." } };
  });

  if (results.some((entry) => entry.record?.deleted === true)) {
    await object.scheduleReaping(limits.tombstoneRetentionDays);
  }

  return json(200, { results });
}
