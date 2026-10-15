import { describe, expect, it } from "vitest";
import { clickhouseAlterUpdate, isRowsDml, unguardedWrites, withAutoLimits, withLimit, writingStatements } from "./guard";
import { splitStatements } from "./statements";
import { mysqlDialect } from "../mysql/dialect";
import { postgresDialect } from "../postgres/dialect";
import { clickhouseDialect } from "../clickhouse/dialect";
import { mssqlDialect } from "../mssql/dialect";
import { CLICKHOUSE_SYNTAX, MSSQL_SYNTAX, MYSQL_SYNTAX, POSTGRES_SYNTAX } from "./syntax";

/**
 * The gates, held against the statements they exist to stop.
 *
 * Every case here is the same shape: a script in, and what the gate decided about it out. What
 * matters is not the wording — that is the dialog's business — but *whether* a statement was
 * stopped, because a miss here is a table nobody was asked about.
 *
 * The scripts are put through the real splitter rather than hand-built, so a test cannot pass on a
 * statement the editor would never actually produce.
 */

const guarded = (sql: string) => unguardedWrites(splitStatements(sql, MYSQL_SYNTAX), mysqlDialect);
const blocked = (sql: string) => writingStatements(splitStatements(sql, MYSQL_SYNTAX), mysqlDialect).map((b) => b.verb);
const blockedPg = (sql: string) =>
  writingStatements(splitStatements(sql, POSTGRES_SYNTAX), postgresDialect).map((b) => b.verb);
const guardedPg = (sql: string) => unguardedWrites(splitStatements(sql, POSTGRES_SYNTAX), postgresDialect);
const chAlterUpdate = (sql: string) =>
  clickhouseAlterUpdate(splitStatements(sql, CLICKHOUSE_SYNTAX)[0], clickhouseDialect);
const chIsRowsDml = (sql: string) => isRowsDml(splitStatements(sql, CLICKHOUSE_SYNTAX)[0], clickhouseDialect);
const chGuarded = (sql: string) => unguardedWrites(splitStatements(sql, CLICKHOUSE_SYNTAX), clickhouseDialect);

describe("unguardedWrites on ClickHouse's ALTER TABLE ... UPDATE", () => {
  it("stops one that names no rows", () => {
    expect(chGuarded("ALTER TABLE users UPDATE status = 'x'")).toEqual([
      { kind: "rows", verb: "UPDATE", table: "users" },
    ]);
  });

  it("lets a bounded one through", () => {
    expect(chGuarded("ALTER TABLE users UPDATE status = 'x' WHERE id = 1")).toEqual([]);
  });

  it("does not confuse it with a DROP-shaped ALTER", () => {
    expect(chGuarded("ALTER TABLE users DROP COLUMN status")).toEqual([
      { kind: "drop", verb: "ALTER TABLE", table: "users" },
    ]);
  });

  it("leaves a multi-command ALTER unrecognised as the UPDATE shape — falls through to the existing DROP-shape check, which still catches this one since it names a DROP", () => {
    expect(chGuarded("ALTER TABLE users DROP COLUMN x, UPDATE y = 1")).toEqual([
      { kind: "drop", verb: "ALTER TABLE", table: "users" },
    ]);
  });
});

describe("clickhouseAlterUpdate", () => {
  it("reads the table out of ALTER TABLE ... UPDATE ... WHERE ...", () => {
    expect(chAlterUpdate("ALTER TABLE t UPDATE x = 1 WHERE id = 2")).toEqual({
      table: "t",
      clauses: expect.any(Array),
    });
  });

  it("is null for a plain ALTER TABLE ... DROP COLUMN", () => {
    expect(chAlterUpdate("ALTER TABLE t DROP COLUMN x")).toBeNull();
  });

  it("is null when more than one AlterCommand is packed in", () => {
    expect(chAlterUpdate("ALTER TABLE t DROP COLUMN x, UPDATE y = 1 WHERE z = 2")).toBeNull();
  });

  it("is null on MySQL — this is ClickHouse's own spelling of UPDATE", () => {
    expect(
      clickhouseAlterUpdate(splitStatements("ALTER TABLE t UPDATE x = 1", MYSQL_SYNTAX)[0], mysqlDialect)
    ).toBeNull();
  });
});

describe("isRowsDml", () => {
  it("is true for INSERT, DELETE and TRUNCATE regardless of shape", () => {
    expect(chIsRowsDml("INSERT INTO t VALUES (1)")).toBe(true);
    expect(chIsRowsDml("DELETE FROM t WHERE id = 1")).toBe(true);
    expect(chIsRowsDml("TRUNCATE TABLE t")).toBe(true);
  });

  it("is true for ALTER TABLE ... UPDATE ... WHERE ...", () => {
    expect(chIsRowsDml("ALTER TABLE t UPDATE x = 1 WHERE id = 2")).toBe(true);
  });

  it("is false for a plain ALTER TABLE ... DROP COLUMN, or any other DDL", () => {
    expect(chIsRowsDml("ALTER TABLE t DROP COLUMN x")).toBe(false);
    expect(chIsRowsDml("CREATE TABLE t (id UInt32) ENGINE = MergeTree ORDER BY id")).toBe(false);
    expect(chIsRowsDml("DROP TABLE t")).toBe(false);
  });
});

describe("unguardedWrites", () => {
  it("stops a write that names no rows, and names the table it would take", () => {
    expect(guarded("DELETE FROM users")).toEqual([
      { kind: "rows", verb: "DELETE", table: "users" },
    ]);
    expect(guarded("UPDATE users SET active = 0")).toEqual([
      { kind: "rows", verb: "UPDATE", table: "users" },
    ]);
    // Saying which rows is not something a TRUNCATE can do, so there is nothing to look for.
    expect(guarded("TRUNCATE TABLE sessions")).toEqual([
      { kind: "rows", verb: "TRUNCATE", table: "sessions" },
    ]);
  });

  it("lets a bounded write through", () => {
    expect(guarded("DELETE FROM users WHERE id = 1")).toEqual([]);
    expect(guarded("DELETE FROM users LIMIT 10")).toEqual([]);
    expect(guarded("UPDATE users SET active = 0 WHERE id = 1")).toEqual([]);
  });

  it("does not take a subquery's WHERE for the statement's own", () => {
    expect(guarded("DELETE FROM users WHERE id IN (SELECT id FROM banned)")).toEqual([]);
    expect(guarded("DELETE FROM (SELECT * FROM users WHERE id = 1) x")).toHaveLength(1);
  });

  it("reads a WHERE in a string or a comment as text, not as a clause", () => {
    expect(guarded("DELETE FROM notes WHERE body = 'no where'")).toEqual([]);
    expect(guarded("DELETE FROM notes -- WHERE id = 1")).toHaveLength(1);
    expect(guarded("DELETE FROM notes /* WHERE id = 1 */")).toHaveLength(1);
  });

  /** The gap this file was written for: MySQL 8 lets a CTE lead into a write, and the statement's
   *  own first word says nothing about it. */
  it("judges a WITH by what it leads into", () => {
    expect(guarded("WITH ids AS (SELECT id FROM banned) DELETE FROM users")).toEqual([
      { kind: "rows", verb: "DELETE", table: "users" },
    ]);
    expect(guarded("WITH ids AS (SELECT id FROM banned) UPDATE users SET active = 0")).toEqual([
      { kind: "rows", verb: "UPDATE", table: "users" },
    ]);
    // The CTE's own WHERE bounds the CTE. What bounds the DELETE is the one after it.
    expect(guarded("WITH ids AS (SELECT id FROM banned WHERE x = 1) DELETE FROM users")).toHaveLength(1);
    expect(
      guarded("WITH ids AS (SELECT id FROM banned) DELETE FROM users WHERE id IN (SELECT id FROM ids)")
    ).toEqual([]);
    // A CTE that only reads is not a write however long it is.
    expect(guarded("WITH ids AS (SELECT id FROM banned) SELECT * FROM users")).toEqual([]);
  });

  it("asks about a DROP in the words a DROP deserves", () => {
    expect(guarded("DROP TABLE users")).toEqual([
      { kind: "drop", verb: "DROP TABLE", table: "users" },
    ]);
    expect(guarded("DROP DATABASE shop")).toEqual([
      { kind: "drop", verb: "DROP DATABASE", table: "shop" },
    ]);
    // The words between the verb and the name say nothing about either.
    expect(guarded("DROP TEMPORARY TABLE IF EXISTS scratch")).toEqual([
      { kind: "drop", verb: "DROP TABLE", table: "scratch" },
    ]);
    // Backticks are not part of the name — and a script that quotes every name is the commonest
    // kind there is, so the dialog has to be able to say which table this was about.
    expect(guarded("DROP TABLE `order`")).toEqual([
      { kind: "drop", verb: "DROP TABLE", table: "order" },
    ]);
    expect(guarded("DELETE FROM `users`")).toEqual([
      { kind: "rows", verb: "DELETE", table: "users" },
    ]);
  });

  it("does not read a quoted name as the clause it is spelled like", () => {
    // `` `where` `` is a column. Reading it as the clause would talk the gate out of asking.
    expect(guarded("UPDATE audit SET `where` = 'x'")).toEqual([
      { kind: "rows", verb: "UPDATE", table: "audit" },
    ]);
    expect(guarded("DELETE FROM `delete`")).toEqual([
      { kind: "rows", verb: "DELETE", table: "delete" },
    ]);
  });

  it("asks about an ALTER only for what it drops", () => {
    expect(guarded("ALTER TABLE users DROP COLUMN email")).toEqual([
      { kind: "drop", verb: "ALTER TABLE", table: "users" },
    ]);
    expect(guarded("ALTER TABLE users ADD COLUMN email VARCHAR(255)")).toEqual([]);
    // A column whose *name* holds the word is not a DROP: the token is one word, not two.
    expect(guarded("ALTER TABLE users ADD COLUMN dropped_at DATETIME")).toEqual([]);
  });

  it("reports every unguarded statement in a script, in order", () => {
    const script = `
      SELECT 1;
      DELETE FROM a;
      UPDATE b SET x = 1 WHERE id = 2;
      DROP TABLE c;
    `;
    expect(guarded(script)).toEqual([
      { kind: "rows", verb: "DELETE", table: "a" },
      { kind: "drop", verb: "DROP TABLE", table: "c" },
    ]);
  });

  /** `EXPLAIN ANALYZE` runs what it is given: the plan comes back with the rows already gone. */
  it("judges an EXPLAIN ANALYZE by the statement it runs", () => {
    expect(guarded("EXPLAIN ANALYZE DELETE FROM users")).toEqual([
      { kind: "rows", verb: "DELETE", table: "users" },
    ]);
    expect(guardedPg("EXPLAIN ANALYZE VERBOSE UPDATE users SET active = false")).toEqual([
      { kind: "rows", verb: "UPDATE", table: "users" },
    ]);
    // PostgreSQL's bracketed spelling of the same option, which the top-level reader drops.
    expect(guardedPg("EXPLAIN (ANALYZE, VERBOSE) DELETE FROM users")).toEqual([
      { kind: "rows", verb: "DELETE", table: "users" },
    ]);
    // The CTE behind it is still read for what it leads into.
    expect(guarded("EXPLAIN ANALYZE WITH ids AS (SELECT id FROM banned) DELETE FROM users")).toEqual([
      { kind: "rows", verb: "DELETE", table: "users" },
    ]);
    // A bounded one is bounded however it is wrapped, and a plan on its own runs nothing.
    expect(guarded("EXPLAIN ANALYZE DELETE FROM users WHERE id = 1")).toEqual([]);
    expect(guarded("EXPLAIN DELETE FROM users")).toEqual([]);
  });

  it("still stops a statement whose shape defeated the reader, only less precisely", () => {
    expect(guarded("DELETE FROM")).toEqual([{ kind: "rows", verb: "DELETE", table: "" }]);
    expect(guarded("DROP")).toEqual([{ kind: "drop", verb: "DROP", table: "" }]);
  });
});

describe("writingStatements", () => {
  it("passes the statements that only read", () => {
    expect(blocked("SELECT * FROM users")).toEqual([]);
    expect(blocked("SHOW TABLES; DESCRIBE users; EXPLAIN SELECT 1; USE shop")).toEqual([]);
    expect(blocked("WITH ids AS (SELECT id FROM banned) SELECT * FROM ids")).toEqual([]);
  });

  it("refuses a write by its opening word", () => {
    expect(blocked("INSERT INTO users (id) VALUES (1)")).toEqual(["INSERT"]);
    expect(blocked("DROP TABLE users")).toEqual(["DROP"]);
    expect(blocked("SET autocommit = 0")).toEqual(["SET"]);
  });

  it("refuses a WITH that leads into a write, naming the word that gives it away", () => {
    expect(blocked("WITH ids AS (SELECT id FROM banned) DELETE FROM users")).toEqual(["DELETE"]);
    expect(blocked("WITH ids AS (SELECT id FROM banned) UPDATE users SET active = 0")).toEqual([
      "UPDATE",
    ]);
  });

  it("refuses a SELECT that leaves a file on the server", () => {
    expect(blocked("SELECT * FROM users INTO OUTFILE '/tmp/u.csv'")).toEqual(["INTO OUTFILE"]);
    expect(blocked("SELECT * FROM users INTO DUMPFILE '/tmp/u.bin'")).toEqual(["INTO DUMPFILE"]);
    // `INTO @variable` leaves nothing behind.
    expect(blocked("SELECT id FROM users INTO @last")).toEqual([]);
    // The CTE in front changes nothing: it is still a SELECT writing a file.
    expect(blocked("WITH ids AS (SELECT id FROM banned) SELECT * FROM ids INTO OUTFILE '/tmp/u.csv'"))
      .toEqual(["INTO OUTFILE"]);
  });

  it("refuses PostgreSQL's SELECT INTO, which creates the table it names", () => {
    expect(blockedPg("SELECT * INTO backup FROM users")).toEqual(["SELECT INTO"]);
    expect(blockedPg("SELECT * INTO TEMP backup FROM users")).toEqual(["SELECT INTO"]);
    expect(blockedPg("WITH ids AS (SELECT id FROM banned) SELECT * INTO backup FROM ids")).toEqual([
      "SELECT INTO",
    ]);
    // An INTO inside a subquery is not the statement's own.
    expect(blockedPg("SELECT * FROM (SELECT 1) t")).toEqual([]);
  });

  it("refuses an EXPLAIN ANALYZE by the statement it runs, and passes a plain EXPLAIN", () => {
    expect(blocked("EXPLAIN ANALYZE DELETE FROM users")).toEqual(["DELETE"]);
    expect(blocked("EXPLAIN ANALYZE FORMAT=TREE INSERT INTO users (id) VALUES (1)")).toEqual([
      "INSERT",
    ]);
    expect(blockedPg("EXPLAIN ANALYZE DELETE FROM users")).toEqual(["DELETE"]);
    expect(blockedPg("EXPLAIN (ANALYZE) UPDATE users SET active = false")).toEqual(["UPDATE"]);
    expect(blockedPg("EXPLAIN (ANALYSE, BUFFERS) TRUNCATE sessions")).toEqual(["TRUNCATE"]);
    // A CTE leading into a write, one wrapper further out.
    expect(blocked("EXPLAIN ANALYZE WITH ids AS (SELECT id FROM banned) DELETE FROM users")).toEqual(
      ["DELETE"]
    );
    // Planning is not running: `EXPLAIN` on its own reads nothing but the plan.
    expect(blocked("EXPLAIN DELETE FROM users")).toEqual([]);
    expect(blocked("EXPLAIN ANALYZE SELECT * FROM users")).toEqual([]);
    expect(blockedPg("EXPLAIN (ANALYZE) SELECT * FROM users")).toEqual([]);
  });

  it("names every refused statement, not only the first", () => {
    expect(blocked("SELECT 1; DELETE FROM a; INSERT INTO b VALUES (1)")).toEqual([
      "DELETE",
      "INSERT",
    ]);
  });
});

describe("withLimit", () => {
  const one = (sql: string, limit = 500) => withLimit(splitStatements(sql, MYSQL_SYNTAX)[0], limit, mysqlDialect);

  it("puts a ceiling on a read that sets none of its own", () => {
    expect(one("SELECT * FROM users")).toBe("SELECT * FROM users\nLIMIT 500");
    // On its own line: the statement may well end in a comment.
    expect(one("SELECT * FROM users -- everyone")).toBe("SELECT * FROM users -- everyone\nLIMIT 500");
  });

  it("leaves alone what needs no ceiling", () => {
    expect(one("SELECT * FROM users LIMIT 10")).toBeNull();
    expect(one("SELECT 1")).toBeNull();
    expect(one("SELECT NOW()")).toBeNull();
    expect(one("UPDATE users SET active = 0")).toBeNull();
    expect(one("SELECT * FROM users INTO OUTFILE '/tmp/u.csv'")).toBeNull();
  });

  it("leaves a read that sets its ceiling the standard's way alone", () => {
    // `FETCH FIRST` is the ceiling; PostgreSQL rejects a `LIMIT` appended beside it.
    const pg = (sql: string) => withLimit(splitStatements(sql, POSTGRES_SYNTAX)[0], 500, postgresDialect);
    expect(pg("SELECT * FROM users FETCH FIRST 10 ROWS ONLY")).toBeNull();
    expect(pg("SELECT * FROM users OFFSET 20 ROWS FETCH NEXT 10 ROWS ONLY")).toBeNull();
    // An OFFSET on its own sets no ceiling, so it still gets one.
    expect(pg("SELECT * FROM users OFFSET 20")).toBe("SELECT * FROM users OFFSET 20\nLIMIT 500");
  });

  it("leaves a locking read exactly as it was written", () => {
    // A LIMIT appended here lands after the locking clause, which MySQL will not parse.
    expect(one("SELECT * FROM users FOR UPDATE")).toBeNull();
    expect(one("SELECT * FROM users LOCK IN SHARE MODE")).toBeNull();
  });

  it("limits a CTE that is read from, and never one that writes", () => {
    expect(one("WITH ids AS (SELECT id FROM banned) SELECT * FROM ids")).toBe(
      "WITH ids AS (SELECT id FROM banned) SELECT * FROM ids\nLIMIT 500"
    );
    expect(one("WITH ids AS (SELECT id FROM banned) DELETE FROM users")).toBeNull();
    expect(one("WITH ids AS (SELECT id FROM banned) UPDATE users u JOIN ids SET u.x = 1")).toBeNull();
  });

  describe("on SQL Server, which has no LIMIT", () => {
    const mssql = (sql: string, limit = 500) =>
      withLimit(splitStatements(sql, MSSQL_SYNTAX)[0], limit, mssqlDialect);

    it("inserts TOP right after SELECT rather than appending anything", () => {
      expect(mssql("SELECT * FROM users")).toBe("SELECT TOP (500) * FROM users");
      // Never on its own line at the end — there is nothing to append at all here.
      expect(mssql("SELECT * FROM users -- everyone")).toBe(
        "SELECT TOP (500) * FROM users -- everyone"
      );
    });

    it("lands after DISTINCT, not before it", () => {
      expect(mssql("SELECT DISTINCT name FROM users")).toBe(
        "SELECT DISTINCT TOP (500) name FROM users"
      );
      expect(mssql("SELECT ALL name FROM users")).toBe("SELECT ALL TOP (500) name FROM users");
    });

    it("leaves alone what already sets its own ceiling", () => {
      expect(mssql("SELECT TOP 10 * FROM users")).toBeNull();
      expect(mssql("SELECT TOP (10) * FROM users")).toBeNull();
      expect(mssql("SELECT * FROM users ORDER BY id OFFSET 0 ROWS FETCH NEXT 10 ROWS ONLY")).toBeNull();
      expect(mssql("SELECT 1")).toBeNull();
    });

    it("leaves a SELECT ... INTO alone rather than silently truncating what it copies", () => {
      expect(mssql("SELECT * INTO backup FROM users")).toBeNull();
    });

    it("still finds the outer SELECT past a CTE's own body", () => {
      expect(mssql("WITH ids AS (SELECT id FROM banned) SELECT * FROM ids")).toBe(
        "WITH ids AS (SELECT id FROM banned) SELECT TOP (500) * FROM ids"
      );
    });
  });
});

describe("withAutoLimits", () => {
  it("rewrites every statement that wanted one and leaves the rest where they were", () => {
    const sql = "SELECT * FROM a;\nUPDATE b SET x = 1;\nSELECT * FROM c;";
    expect(withAutoLimits(sql, splitStatements(sql, MYSQL_SYNTAX), 100, mysqlDialect)).toEqual({
      sql: "SELECT * FROM a\nLIMIT 100;\nUPDATE b SET x = 1;\nSELECT * FROM c\nLIMIT 100;",
      added: 2,
    });
  });

  it("does nothing at all when the ceiling is off", () => {
    const sql = "SELECT * FROM a";
    expect(withAutoLimits(sql, splitStatements(sql, MYSQL_SYNTAX), 0, mysqlDialect)).toEqual({
      sql,
      added: 0,
    });
  });

  it("keeps the script's own text around each statement it rewrites", () => {
    const sql = "-- a note\nSELECT * FROM a;\n\n-- and another\nSELECT * FROM b;\n";
    const { sql: out, added } = withAutoLimits(sql, splitStatements(sql, MYSQL_SYNTAX), 5, mysqlDialect);
    expect(added).toBe(2);
    expect(out).toContain("-- a note");
    expect(out).toContain("-- and another");
    expect(out.endsWith(";\n")).toBe(true);
  });
});
