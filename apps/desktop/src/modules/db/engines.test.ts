import { describe, expect, it } from "vitest";
import { isSqlKind } from "./engines";
import { DEFAULT_PORTS, type DbKind } from "./types";

/** Every kind this build has, read off the one table that has to list all of them. */
const ALL_KINDS = Object.keys(DEFAULT_PORTS) as DbKind[];

/**
 * Kinds `ConnectionWorkspace` deliberately has no workspace for. Emptying this list is what finishing an engine
 * looks like; adding to it is what starting one looks like.
 */
const NO_WORKSPACE: DbKind[] = [];

/**
 * Which branch of `ConnectionWorkspace`'s dispatch claims a kind — mirroring the if-chain in
 * `tab/ConnectionWorkspace.tsx`, in the order it asks. The two must agree: a branch added or reordered there is a change
 * here, the same contract `filters.rs` and `src/filters.ts` keep between them.
 */
function workspaceFor(kind: DbKind): "sql" | "mongo" | "redis" | "none" {
  if (isSqlKind(kind)) return "sql";
  if (kind === "mongo") return "mongo";
  if (kind === "redis") return "redis";
  return "none";
}

describe("which workspace a kind opens", () => {
  /**
   * The regression guard for a bug that shipped: `DbTab` ended in an unguarded `return
   * <RedisWorkspace/>`, so Redis was the workspace for every kind the branches above it did not
   * claim. That was invisible while they covered every kind. The first kind they did not — SQL
   * Server — opened Redis's key browser, which asked the backend for Redis things and was told
   * "This is not a Redis connection".
   *
   * So: no kind may reach a workspace by falling through to it. A kind with nothing to open says
   * so, and this test is what makes forgetting that a failure rather than a mystery.
   */
  it("gives Redis only to Redis", () => {
    const redisKinds = ALL_KINDS.filter((kind) => workspaceFor(kind) === "redis");
    expect(redisKinds).toEqual(["redis"]);
  });

  it("gives Mongo only to Mongo", () => {
    const mongoKinds = ALL_KINDS.filter((kind) => workspaceFor(kind) === "mongo");
    expect(mongoKinds).toEqual(["mongo"]);
  });

  /** A new kind is unclaimed until someone claims it, and has to be admitted here on purpose. */
  it("accounts for every kind, so a new one cannot land somewhere by accident", () => {
    const unclaimed = ALL_KINDS.filter((kind) => workspaceFor(kind) === "none");
    expect(unclaimed).toEqual(NO_WORKSPACE);
  });

  /** The five SQL engines, SQL Server now among them — its table reads landed. */
  it("opens the SQL workspace for the engines that have one", () => {
    expect(ALL_KINDS.filter((kind) => workspaceFor(kind) === "sql")).toEqual([
      "mysql",
      "postgres",
      "sqlite",
      "clickhouse",
      "mssql",
    ]);
  });
});
