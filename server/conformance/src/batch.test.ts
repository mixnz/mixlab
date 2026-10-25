import { beforeAll, describe, expect, it } from "vitest";
import {
  call,
  type Capabilities,
  type ErrorBody,
  newRecord,
  opaqueId,
  put,
  seed,
  type Session,
  signedUp,
  since,
  type StoredRecord,
} from "./client.js";

interface BatchResult {
  results: { status: number; record?: StoredRecord; error?: ErrorBody["error"] }[];
}

let session: Session;
let limits: Capabilities;

const batch = (token: string, operations: unknown[]) =>
  call<BatchResult & Partial<ErrorBody>>("/v1/records/batch", { body: { operations }, token });

beforeAll(async () => {
  session = (await signedUp()).session;
  limits = (await call<Capabilities>("/v1/capabilities")).body;
});

describe("POST /v1/records/batch", () => {
  it("answers one result per operation, in the order they were sent", async () => {
    const collection = opaqueId();
    const ids = [opaqueId(), opaqueId(), opaqueId()];
    const result = await batch(
      session.accessToken,
      ids.map((id) => ({ op: "put", collection, id, ifNoneMatch: true, record: newRecord() })),
    );

    expect(result.status).toBe(200);
    expect(result.body.results).toHaveLength(3);
    expect(result.body.results.map((entry) => entry.status)).toEqual([201, 201, 201]);
    expect(result.body.results.map((entry) => entry.record?.id)).toEqual(ids);
  });

  it("answers 200 on the envelope even when an entry conflicts", async () => {
    // It is a batch of independent compare-and-swaps and not a transaction (D4), so a 409 in one
    // entry is news for the client rather than a failure of the request.
    const { collection, id } = await seed(session.accessToken);
    const result = await batch(session.accessToken, [
      { op: "put", collection, id, ifMatch: 99, record: newRecord() },
      { op: "put", collection, id: opaqueId(), ifNoneMatch: true, record: newRecord() },
    ]);

    expect(result.status).toBe(200);
    expect(result.body.results[0]?.status).toBe(409);
    expect(result.body.results[0]?.error?.code).toBe("version-conflict");
    expect(result.body.results[1]?.status).toBe(201);
  });

  it("carries a delete as readily as a write", async () => {
    const { collection, id, stored } = await seed(session.accessToken);
    const result = await batch(session.accessToken, [
      { op: "delete", collection, id, ifMatch: stored.version, updatedAt: 1_900_000_000 },
    ]);

    expect(result.body.results[0]?.status).toBe(200);
    expect(result.body.results[0]?.record?.deleted).toBe(true);
    expect(result.body.results[0]?.record?.updatedAt).toBe(1_900_000_000);
  });

  it("refuses a delete entry that does not say when it was made", async () => {
    // The same rule as the single route: a deletion is weighed by D4, so it carries its time.
    const { collection, id, stored } = await seed(session.accessToken);
    const result = await batch(session.accessToken, [{ op: "delete", collection, id, ifMatch: stored.version }]);
    expect(result.body.results[0]?.status).toBe(400);
    expect(result.body.results[0]?.error?.code).toBe("invalid-request");
  });

  it("assigns sequence numbers in the order the operations were given", async () => {
    const { session: own } = await signedUp();
    const collection = opaqueId();
    const ids = [opaqueId(), opaqueId(), opaqueId()];
    await batch(
      own.accessToken,
      ids.map((id) => ({ op: "put", collection, id, ifNoneMatch: true, record: newRecord() })),
    );

    const page = await since(own.accessToken, 0, collection);
    expect(page.body.records.map((record) => record.id)).toEqual(ids);
  });

  it("writes the same thing a single request would have", async () => {
    // The whole reason a first sync may use this route is that two hundred round trips are a wait
    // (D4). It must not be a different kind of write.
    const { session: own } = await signedUp();
    const collection = opaqueId();
    const id = opaqueId();
    const record = newRecord();
    await batch(own.accessToken, [{ op: "put", collection, id, ifNoneMatch: true, record }]);

    const alone = await put(own.accessToken, collection, opaqueId(), record, { ifNoneMatch: true });
    const page = await since(own.accessToken, 0, collection);
    const batched = page.body.records.find((entry) => entry.id === id);

    expect(batched?.version).toBe(alone.body.version);
    expect(batched?.ciphertext).toBe(alone.body.ciphertext);
    expect(batched?.deleted).toBe(alone.body.deleted);
  });

  it("refuses an empty list", async () => {
    const result = await batch(session.accessToken, []);
    expect(result.status).toBe(400);
    expect(result.body.error?.code).toBe("invalid-request");
  });

  it("refuses more operations than it said it would take", async () => {
    const collection = opaqueId();
    const operations = Array.from({ length: limits.maxBatchOperations + 1 }, () => ({
      op: "put",
      collection,
      id: opaqueId(),
      ifNoneMatch: true,
      record: newRecord(16),
    }));

    const result = await batch(session.accessToken, operations);
    expect(result.status).toBe(400);
    expect(result.body.error?.code).toBe("invalid-request");
  });

  it("refuses a batch presented without a token", async () => {
    const result = await call<ErrorBody>("/v1/records/batch", {
      body: { operations: [{ op: "put", collection: opaqueId(), id: opaqueId(), ifNoneMatch: true, record: newRecord() }] },
    });
    expect(result.status).toBe(401);
  });
});
