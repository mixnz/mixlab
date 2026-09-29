import { describe, expect, it } from "vitest";
import { translate } from "./translate";

const ok = async (sql: string) => {
  const result = await translate(sql, "mysql");
  if (!result.ok) throw new Error(`không dịch được: ${JSON.stringify(result.unsupported)}`);
  return result;
};

const codes = async (sql: string) => {
  const result = await translate(sql, "mysql");
  if (result.ok) throw new Error("đáng lẽ phải bị từ chối");
  return result.unsupported.map((u) => u.code);
};

describe("translate — minimal cases", () => {
  it("SELECT * becomes an empty find", async () => {
    expect((await ok("SELECT * FROM users")).output).toBe("db.users.find({})");
  });

  it("ignores semicolons and extra whitespace", async () => {
    expect((await ok("  SELECT * FROM users ;  ")).output).toBe("db.users.find({})");
  });
});

describe("translate — refusals", () => {
  it("refuses JOIN", async () => {
    expect(await codes("SELECT * FROM a JOIN b ON a.id = b.a_id")).toContain("join");
  });

  it("refuses subqueries", async () => {
    expect(await codes("SELECT * FROM a WHERE id IN (SELECT id FROM b)")).toContain("subquery");
  });

  it("refuses UNION", async () => {
    expect(await codes("SELECT a FROM x UNION SELECT a FROM y")).toContain("union");
  });

  it("refuses CTEs", async () => {
    expect(await codes("WITH t AS (SELECT 1) SELECT * FROM t")).toContain("cte");
  });

  it("refuses INSERT/UPDATE/DELETE", async () => {
    expect(await codes("DELETE FROM users WHERE id = 1")).toContain("dml");
    expect(await codes("INSERT INTO users (a) VALUES (1)")).toContain("dml");
  });

  it("refuses several statements in one paste", async () => {
    expect(await codes("SELECT * FROM a; SELECT * FROM b")).toContain("multi");
  });

  it("refuses scalar functions", async () => {
    expect(await codes("SELECT CONCAT(a, b) FROM t")).toContain("function");
  });

  it("refuses CASE", async () => {
    expect(await codes("SELECT CASE WHEN a = 1 THEN 2 ELSE 3 END FROM t")).toContain("case");
  });

  it("reports a syntax error as the `parse` code rather than throwing", async () => {
    expect(await codes("SELECT FROM")).toContain("parse");
  });

  it("includes the offending SQL fragment, so the Panel can point at it", async () => {
    const result = await translate("SELECT * FROM a JOIN b ON a.id = b.a_id", "mysql");
    if (result.ok) throw new Error("đáng lẽ phải bị từ chối");
    expect(result.unsupported[0].fragment).not.toBe("");
  });

  it("`NOT` is a function node in the AST but must still translate", async () => {
    expect((await ok("SELECT * FROM t WHERE NOT (a = 1)")).output).toContain("$nor");
  });
});

describe("translate — the find() path", () => {
  it("turns the column list into a projection, and turns _id off", async () => {
    expect((await ok("SELECT name, email FROM users")).output).toBe(
      'db.users.find({}, {\n  "name": 1,\n  "email": 1,\n  "_id": 0\n})',
    );
  });

  it("keeps _id when it is selected by name", async () => {
    const out = (await ok("SELECT _id, name FROM users")).output;
    expect(out).toContain('"_id": 1');
    expect(out).not.toContain('"_id": 0');
  });

  it("turns aliases into a projection with expressions", async () => {
    expect((await ok("SELECT name AS n FROM users")).output).toContain('"n": "$name"');
  });

  it("translates comparison operators", async () => {
    expect((await ok("SELECT * FROM u WHERE age > 18")).output).toContain('"$gt": 18');
    expect((await ok("SELECT * FROM u WHERE age = 18")).output).toContain('"age": 18');
    expect((await ok("SELECT * FROM u WHERE age <> 18")).output).toContain('"$ne": 18');
    expect((await ok("SELECT * FROM u WHERE age >= 18")).output).toContain('"$gte": 18');
    expect((await ok("SELECT * FROM u WHERE age <= 18")).output).toContain('"$lte": 18');
    expect((await ok("SELECT * FROM u WHERE age < 18")).output).toContain('"$lt": 18');
  });

  it("translates AND and OR", async () => {
    expect((await ok("SELECT * FROM u WHERE a = 1 AND b = 2")).output).toContain('"$and"');
    expect((await ok("SELECT * FROM u WHERE a = 1 OR b = 2")).output).toContain('"$or"');
  });

  it("translates IN, NOT IN and BETWEEN", async () => {
    expect((await ok("SELECT * FROM u WHERE id IN (1, 2)")).output).toContain('"$in"');
    expect((await ok("SELECT * FROM u WHERE id NOT IN (1, 2)")).output).toContain('"$nin"');
    const between = (await ok("SELECT * FROM u WHERE age BETWEEN 1 AND 5")).output;
    expect(between).toContain('"$gte": 1');
    expect(between).toContain('"$lte": 5');
  });

  it("translates IS NULL and IS NOT NULL", async () => {
    expect((await ok("SELECT * FROM u WHERE a IS NULL")).output).toContain('"a": null');
    expect((await ok("SELECT * FROM u WHERE a IS NOT NULL")).output).toContain('"$ne": null');
  });

  it("translates ORDER BY, LIMIT and OFFSET, in the right method-chain order", async () => {
    expect((await ok("SELECT * FROM u ORDER BY a DESC, b ASC LIMIT 10 OFFSET 20")).output).toBe(
      'db.u.find({}).sort({\n  "a": -1,\n  "b": 1\n}).skip(20).limit(10)',
    );
  });

  it("LIMIT without OFFSET produces no .skip()", async () => {
    expect((await ok("SELECT * FROM u LIMIT 10")).output).toBe("db.u.find({}).limit(10)");
  });

  it("reads PostgreSQL column names, which the AST wraps in an object", async () => {
    const result = await translate("SELECT * FROM u WHERE age > 18", "postgresql");
    if (!result.ok) throw new Error("đáng lẽ dịch được");
    expect(result.output).toContain('"age"');
  });
});

describe("translate — LIKE", () => {
  it("uses $regex, and MySQL is case-insensitive so $options i is added", async () => {
    const out = (await ok("SELECT * FROM u WHERE name LIKE 'abc%'")).output;
    expect(out).toContain('"$regex": "^abc.*"');
    expect(out).toContain('"$options": "i"');
  });

  it("PostgreSQL's LIKE is case-sensitive, so there is no $options", async () => {
    const result = await translate("SELECT * FROM u WHERE name LIKE 'abc%'", "postgresql");
    if (!result.ok) throw new Error("đáng lẽ dịch được");
    expect(result.output).toContain('"$regex": "^abc.*"');
    expect(result.output).not.toContain("$options");
  });

  it("PostgreSQL's ILIKE does have it", async () => {
    const result = await translate("SELECT * FROM u WHERE name ILIKE 'abc%'", "postgresql");
    if (!result.ok) throw new Error("đáng lẽ dịch được");
    expect(result.output).toContain('"$options": "i"');
  });

  it("NOT LIKE becomes $not around the regex expression", async () => {
    expect((await ok("SELECT * FROM u WHERE name NOT LIKE 'a%'")).output).toContain('"$not"');
  });
});

describe("translate — warnings", () => {
  const warnings = async (sql: string) => (await ok(sql)).warnings.map((w) => w.code);

  it("warns on IS NULL: Mongo also matches documents missing the field", async () => {
    expect(await warnings("SELECT * FROM u WHERE a IS NULL")).toContain("isNull");
  });

  it("warns when a column is compared with a string that looks like a number", async () => {
    expect(await warnings("SELECT * FROM u WHERE id = '5'")).toContain("type");
  });

  it("warns when _id is compared with a 24-character hex string", async () => {
    expect(await warnings("SELECT * FROM u WHERE _id = '507f1f77bcf86cd799439011'")).toContain(
      "objectId",
    );
  });

  it("warns about nothing for a query with no semantic mismatch", async () => {
    expect((await ok("SELECT * FROM u WHERE age > 18")).warnings).toEqual([]);
  });

  it("each warning carries the field it is about", async () => {
    const result = await ok("SELECT * FROM u WHERE a IS NULL");
    expect(result.warnings[0].fragment).toBe("a");
  });
});

describe("translate — pipeline", () => {
  it("GROUP BY with COUNT(*) becomes $group then $project", async () => {
    const out = (await ok("SELECT city, COUNT(*) AS n FROM users GROUP BY city")).output;
    expect(out.startsWith("db.users.aggregate([")).toBe(true);
    expect(out).toContain('"_id": "$city"');
    expect(out).toContain('"$sum": 1');
    expect(out).toContain('"city": "$_id"');
  });

  it("COUNT(col) skips NULL, so it is not $sum 1", async () => {
    const out = (await ok("SELECT city, COUNT(email) AS n FROM users GROUP BY city")).output;
    expect(out).toContain('"$cond"');
    expect(out).toContain('"$eq"');
  });

  it("WHERE becomes $match before $group, HAVING becomes $match after", async () => {
    const out = (
      await ok(
        "SELECT city, COUNT(*) AS n FROM users WHERE age > 18 GROUP BY city HAVING COUNT(*) > 5",
      )
    ).output;
    const beforeGroup = out.indexOf('"$match"');
    const group = out.indexOf('"$group"');
    const afterGroup = out.lastIndexOf('"$match"');
    expect(beforeGroup).toBeGreaterThan(-1);
    expect(beforeGroup).toBeLessThan(group);
    expect(afterGroup).toBeGreaterThan(group);
  });

  it("HAVING filters by the alias $group just created, not by an empty name", async () => {
    const out = (
      await ok("SELECT city, COUNT(*) AS n FROM users GROUP BY city HAVING COUNT(*) > 5")
    ).output;
    expect(out).toContain('"n": {\n        "$gt": 5\n      }');
    expect(out).not.toContain('""');
  });

  it("HAVING using an aggregate not in SELECT makes $group add it itself", async () => {
    const out = (await ok("SELECT city FROM users GROUP BY city HAVING COUNT(*) > 5")).output;
    expect(out).not.toContain('""');
    // The helper field has to be grouped to be filterable, then dropped by $project.
    expect(out).toContain('"$sum": 1');
    const project = out.slice(out.indexOf('"$project"'));
    expect(project).not.toContain("_having");
  });

  it("SUM, AVG, MIN, MAX become operators of the same name", async () => {
    const out = (
      await ok("SELECT e, SUM(a) AS s, AVG(b) AS v, MIN(c) AS lo, MAX(d) AS hi FROM t GROUP BY e")
    ).output;
    for (const op of ["$sum", "$avg", "$min", "$max"]) expect(out).toContain(op);
  });

  it("DISTINCT becomes a $group with no aggregate", async () => {
    const out = (await ok("SELECT DISTINCT city FROM users")).output;
    expect(out).toContain('"$group"');
    expect(out).toContain('"_id"');
    expect(out).toContain('"city"');
  });

  it("an aggregate without GROUP BY still takes the pipeline, over the whole table", async () => {
    const out = (await ok("SELECT COUNT(*) AS n FROM users")).output;
    expect(out).toContain('"$group"');
    expect(out).toContain('"_id": null');
  });

  it("ORDER BY, LIMIT become the final stages, in the right order", async () => {
    const out = (await ok("SELECT city, COUNT(*) AS n FROM u GROUP BY city ORDER BY n DESC LIMIT 5"))
      .output;
    expect(out.indexOf('"$sort"')).toBeLessThan(out.indexOf('"$limit"'));
  });

  it("a query with nothing to group still takes the find() path, not the pipeline", async () => {
    expect((await ok("SELECT a FROM t WHERE b = 1")).output).toContain(".find(");
  });

  it("warns on SELECT * with GROUP BY — MySQL allows it, Mongo has no equivalent", async () => {
    const result = await ok("SELECT * FROM users GROUP BY city");
    expect(result.warnings.map((w) => w.code)).toContain("starWithGroupBy");
  });
});
