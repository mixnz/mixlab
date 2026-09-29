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

describe("translate — trường hợp tối giản", () => {
  it("SELECT * thành find rỗng", async () => {
    expect((await ok("SELECT * FROM users")).output).toBe("db.users.find({})");
  });

  it("bỏ qua dấu chấm phẩy và khoảng trắng thừa", async () => {
    expect((await ok("  SELECT * FROM users ;  ")).output).toBe("db.users.find({})");
  });
});

describe("translate — từ chối", () => {
  it("từ chối JOIN", async () => {
    expect(await codes("SELECT * FROM a JOIN b ON a.id = b.a_id")).toContain("join");
  });

  it("từ chối subquery", async () => {
    expect(await codes("SELECT * FROM a WHERE id IN (SELECT id FROM b)")).toContain("subquery");
  });

  it("từ chối UNION", async () => {
    expect(await codes("SELECT a FROM x UNION SELECT a FROM y")).toContain("union");
  });

  it("từ chối CTE", async () => {
    expect(await codes("WITH t AS (SELECT 1) SELECT * FROM t")).toContain("cte");
  });

  it("từ chối INSERT/UPDATE/DELETE", async () => {
    expect(await codes("DELETE FROM users WHERE id = 1")).toContain("dml");
    expect(await codes("INSERT INTO users (a) VALUES (1)")).toContain("dml");
  });

  it("từ chối nhiều câu lệnh trong một lần dán", async () => {
    expect(await codes("SELECT * FROM a; SELECT * FROM b")).toContain("multi");
  });

  it("từ chối hàm vô hướng", async () => {
    expect(await codes("SELECT CONCAT(a, b) FROM t")).toContain("function");
  });

  it("từ chối CASE", async () => {
    expect(await codes("SELECT CASE WHEN a = 1 THEN 2 ELSE 3 END FROM t")).toContain("case");
  });

  it("báo lỗi cú pháp thành mã `parse`, không ném ra ngoài", async () => {
    expect(await codes("SELECT FROM")).toContain("parse");
  });

  it("kèm đoạn SQL gây ra, để Panel chỉ được chỗ", async () => {
    const result = await translate("SELECT * FROM a JOIN b ON a.id = b.a_id", "mysql");
    if (result.ok) throw new Error("đáng lẽ phải bị từ chối");
    expect(result.unsupported[0].fragment).not.toBe("");
  });

  it("`NOT` là một node function trong AST nhưng vẫn phải dịch được", async () => {
    expect((await ok("SELECT * FROM t WHERE NOT (a = 1)")).output).toContain("$nor");
  });
});

describe("translate — đường find()", () => {
  it("chuyển danh sách cột thành projection, và tắt _id", async () => {
    expect((await ok("SELECT name, email FROM users")).output).toBe(
      'db.users.find({}, {\n  "name": 1,\n  "email": 1,\n  "_id": 0\n})',
    );
  });

  it("giữ _id khi nó được chọn tên", async () => {
    const out = (await ok("SELECT _id, name FROM users")).output;
    expect(out).toContain('"_id": 1');
    expect(out).not.toContain('"_id": 0');
  });

  it("đổi alias thành projection có biểu thức", async () => {
    expect((await ok("SELECT name AS n FROM users")).output).toContain('"n": "$name"');
  });

  it("dịch các toán tử so sánh", async () => {
    expect((await ok("SELECT * FROM u WHERE age > 18")).output).toContain('"$gt": 18');
    expect((await ok("SELECT * FROM u WHERE age = 18")).output).toContain('"age": 18');
    expect((await ok("SELECT * FROM u WHERE age <> 18")).output).toContain('"$ne": 18');
    expect((await ok("SELECT * FROM u WHERE age >= 18")).output).toContain('"$gte": 18');
    expect((await ok("SELECT * FROM u WHERE age <= 18")).output).toContain('"$lte": 18');
    expect((await ok("SELECT * FROM u WHERE age < 18")).output).toContain('"$lt": 18');
  });

  it("dịch AND và OR", async () => {
    expect((await ok("SELECT * FROM u WHERE a = 1 AND b = 2")).output).toContain('"$and"');
    expect((await ok("SELECT * FROM u WHERE a = 1 OR b = 2")).output).toContain('"$or"');
  });

  it("dịch IN, NOT IN và BETWEEN", async () => {
    expect((await ok("SELECT * FROM u WHERE id IN (1, 2)")).output).toContain('"$in"');
    expect((await ok("SELECT * FROM u WHERE id NOT IN (1, 2)")).output).toContain('"$nin"');
    const between = (await ok("SELECT * FROM u WHERE age BETWEEN 1 AND 5")).output;
    expect(between).toContain('"$gte": 1');
    expect(between).toContain('"$lte": 5');
  });

  it("dịch IS NULL và IS NOT NULL", async () => {
    expect((await ok("SELECT * FROM u WHERE a IS NULL")).output).toContain('"a": null');
    expect((await ok("SELECT * FROM u WHERE a IS NOT NULL")).output).toContain('"$ne": null');
  });

  it("dịch ORDER BY, LIMIT và OFFSET, theo đúng thứ tự chuỗi phương thức", async () => {
    expect((await ok("SELECT * FROM u ORDER BY a DESC, b ASC LIMIT 10 OFFSET 20")).output).toBe(
      'db.u.find({}).sort({\n  "a": -1,\n  "b": 1\n}).skip(20).limit(10)',
    );
  });

  it("LIMIT không kèm OFFSET thì không sinh ra .skip()", async () => {
    expect((await ok("SELECT * FROM u LIMIT 10")).output).toBe("db.u.find({}).limit(10)");
  });

  it("đọc được tên cột của PostgreSQL, thứ AST gói trong một object thay vì một chuỗi", async () => {
    const result = await translate("SELECT * FROM u WHERE age > 18", "postgresql");
    if (!result.ok) throw new Error("đáng lẽ dịch được");
    expect(result.output).toContain('"age"');
  });
});

describe("translate — LIKE", () => {
  it("dùng $regex, và MySQL không phân biệt hoa thường nên thêm $options i", async () => {
    const out = (await ok("SELECT * FROM u WHERE name LIKE 'abc%'")).output;
    expect(out).toContain('"$regex": "^abc.*"');
    expect(out).toContain('"$options": "i"');
  });

  it("LIKE của PostgreSQL có phân biệt hoa thường, nên không có $options", async () => {
    const result = await translate("SELECT * FROM u WHERE name LIKE 'abc%'", "postgresql");
    if (!result.ok) throw new Error("đáng lẽ dịch được");
    expect(result.output).toContain('"$regex": "^abc.*"');
    expect(result.output).not.toContain("$options");
  });

  it("ILIKE của PostgreSQL thì có", async () => {
    const result = await translate("SELECT * FROM u WHERE name ILIKE 'abc%'", "postgresql");
    if (!result.ok) throw new Error("đáng lẽ dịch được");
    expect(result.output).toContain('"$options": "i"');
  });

  it("NOT LIKE thành $not quanh biểu thức regex", async () => {
    expect((await ok("SELECT * FROM u WHERE name NOT LIKE 'a%'")).output).toContain('"$not"');
  });
});

describe("translate — cảnh báo", () => {
  const warnings = async (sql: string) => (await ok(sql)).warnings.map((w) => w.code);

  it("cảnh báo IS NULL vì Mongo khớp cả tài liệu thiếu hẳn trường đó", async () => {
    expect(await warnings("SELECT * FROM u WHERE a IS NULL")).toContain("isNull");
  });

  it("cảnh báo khi so một cột với chuỗi trông như số", async () => {
    expect(await warnings("SELECT * FROM u WHERE id = '5'")).toContain("type");
  });

  it("cảnh báo khi _id bị so với một chuỗi 24 ký tự hex", async () => {
    expect(await warnings("SELECT * FROM u WHERE _id = '507f1f77bcf86cd799439011'")).toContain(
      "objectId",
    );
  });

  it("không cảnh báo gì cho một truy vấn không có chỗ nào lệch nghĩa", async () => {
    expect((await ok("SELECT * FROM u WHERE age > 18")).warnings).toEqual([]);
  });

  it("mỗi cảnh báo mang theo trường nó nói về", async () => {
    const result = await ok("SELECT * FROM u WHERE a IS NULL");
    expect(result.warnings[0].fragment).toBe("a");
  });
});

describe("translate — pipeline", () => {
  it("GROUP BY với COUNT(*) thành $group rồi $project", async () => {
    const out = (await ok("SELECT city, COUNT(*) AS n FROM users GROUP BY city")).output;
    expect(out.startsWith("db.users.aggregate([")).toBe(true);
    expect(out).toContain('"_id": "$city"');
    expect(out).toContain('"$sum": 1');
    expect(out).toContain('"city": "$_id"');
  });

  it("COUNT(col) bỏ qua NULL, nên không phải $sum 1", async () => {
    const out = (await ok("SELECT city, COUNT(email) AS n FROM users GROUP BY city")).output;
    expect(out).toContain('"$cond"');
    expect(out).toContain('"$eq"');
  });

  it("WHERE thành $match trước $group, HAVING thành $match sau", async () => {
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

  it("HAVING lọc theo alias mà $group vừa tạo, không theo một tên rỗng", async () => {
    const out = (
      await ok("SELECT city, COUNT(*) AS n FROM users GROUP BY city HAVING COUNT(*) > 5")
    ).output;
    expect(out).toContain('"n": {\n        "$gt": 5\n      }');
    expect(out).not.toContain('""');
  });

  it("HAVING dùng một hàm gộp không có trong SELECT thì $group phải tự thêm nó vào", async () => {
    const out = (await ok("SELECT city FROM users GROUP BY city HAVING COUNT(*) > 5")).output;
    expect(out).not.toContain('""');
    // The helper field has to be grouped to be filterable, then dropped by $project.
    expect(out).toContain('"$sum": 1');
    const project = out.slice(out.indexOf('"$project"'));
    expect(project).not.toContain("_having");
  });

  it("SUM, AVG, MIN, MAX thành toán tử cùng tên", async () => {
    const out = (
      await ok("SELECT e, SUM(a) AS s, AVG(b) AS v, MIN(c) AS lo, MAX(d) AS hi FROM t GROUP BY e")
    ).output;
    for (const op of ["$sum", "$avg", "$min", "$max"]) expect(out).toContain(op);
  });

  it("DISTINCT thành $group không có hàm gộp nào", async () => {
    const out = (await ok("SELECT DISTINCT city FROM users")).output;
    expect(out).toContain('"$group"');
    expect(out).toContain('"_id"');
    expect(out).toContain('"city"');
  });

  it("một hàm gộp không có GROUP BY vẫn đi pipeline, gộp cả bảng", async () => {
    const out = (await ok("SELECT COUNT(*) AS n FROM users")).output;
    expect(out).toContain('"$group"');
    expect(out).toContain('"_id": null');
  });

  it("ORDER BY, LIMIT thành stage cuối, đúng thứ tự", async () => {
    const out = (await ok("SELECT city, COUNT(*) AS n FROM u GROUP BY city ORDER BY n DESC LIMIT 5"))
      .output;
    expect(out.indexOf('"$sort"')).toBeLessThan(out.indexOf('"$limit"'));
  });

  it("một truy vấn không có gì cần gộp vẫn đi đường find(), không đi pipeline", async () => {
    expect((await ok("SELECT a FROM t WHERE b = 1")).output).toContain(".find(");
  });

  it("cảnh báo SELECT * cùng GROUP BY — MySQL cho qua, Mongo không có tương ứng", async () => {
    const result = await ok("SELECT * FROM users GROUP BY city");
    expect(result.warnings.map((w) => w.code)).toContain("starWithGroupBy");
  });
});
