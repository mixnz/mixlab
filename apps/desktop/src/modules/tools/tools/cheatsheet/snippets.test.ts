import { describe, expect, it } from "vitest";
import {
  addSnippet,
  fill,
  paramsOf,
  readSnippets,
  removeSnippet,
  updateSnippet,
  type Snippet,
} from "./snippets";

describe("paramsOf", () => {
  it("lấy tham số theo thứ tự xuất hiện", () => {
    expect(paramsOf("cmd -h {{host}} -u {{user}}")).toEqual(["host", "user"]);
  });

  it("không lặp lại tham số dùng nhiều lần", () => {
    expect(paramsOf("{{a}} {{b}} {{a}}")).toEqual(["a", "b"]);
  });

  it("trả mảng rỗng cho template không có tham số", () => {
    expect(paramsOf("docker system prune -af")).toEqual([]);
  });

  it("bỏ qua ngoặc nhọn không thành cặp", () => {
    expect(paramsOf("echo {{a} và {b}}")).toEqual([]);
  });

  it("nhận tên có gạch dưới và số", () => {
    expect(paramsOf("{{local_port}}:{{remote_2}}")).toEqual(["local_port", "remote_2"]);
  });
});

describe("fill", () => {
  it("thay tham số bằng giá trị", () => {
    expect(fill("psql -h {{host}} -U {{user}}", { host: "db", user: "an" })).toBe(
      "psql -h db -U an",
    );
  });

  it("thay mọi lần xuất hiện của cùng một tham số", () => {
    expect(fill("{{a}}-{{a}}", { a: "x" })).toBe("x-x");
  });

  // An unfilled slot has to be visible in the output, not vanish into whitespace.
  it("giữ nguyên tham số chưa có giá trị", () => {
    expect(fill("cmd {{host}} {{port}}", { host: "db" })).toBe("cmd db {{port}}");
  });

  it("giữ nguyên tham số có giá trị rỗng", () => {
    expect(fill("cmd {{host}}", { host: "" })).toBe("cmd {{host}}");
  });

  // The tool does not add quotes: the template's author decides where quotes are needed.
  it("không tự bọc ngoặc cho giá trị có dấu cách", () => {
    expect(fill("mysql -p'{{password}}'", { password: "mật khẩu" })).toBe("mysql -p'mật khẩu'");
  });
});

const draft = { title: "Dump", group: "mysql", template: "mysqldump {{db}}" };

describe("thao tác trên danh sách", () => {
  it("thêm vào cuối và đặt id", () => {
    const list = addSnippet([], draft);
    expect(list).toHaveLength(1);
    expect(list[0]?.title).toBe("Dump");
    expect(list[0]?.id).not.toBe("");
  });

  it("không đặt trùng id cho hai snippet", () => {
    const list = addSnippet(addSnippet([], draft), draft);
    expect(list[0]?.id).not.toBe(list[1]?.id);
  });

  it("sửa đúng một mục và giữ nguyên id", () => {
    const list = addSnippet([], draft);
    const id = list[0]!.id;
    const after = updateSnippet(list, id, { ...draft, title: "Khác" });
    expect(after[0]?.id).toBe(id);
    expect(after[0]?.title).toBe("Khác");
  });

  it("bỏ qua khi sửa một id không có", () => {
    const list = addSnippet([], draft);
    expect(updateSnippet(list, "khong-co", draft)).toEqual(list);
  });

  it("xoá đúng một mục", () => {
    const list = addSnippet(addSnippet([], draft), { ...draft, title: "Hai" });
    const after = removeSnippet(list, list[0]!.id);
    expect(after).toHaveLength(1);
    expect(after[0]?.title).toBe("Hai");
  });

  it("không đụng vào mảng gốc", () => {
    const list: Snippet[] = addSnippet([], draft);
    const copy = [...list];
    removeSnippet(list, list[0]!.id);
    expect(list).toEqual(copy);
  });
});

describe("readSnippets", () => {
  const good = { id: "a", title: "T", group: "g", template: "cmd" };

  it("giữ mục đúng shape", () => {
    expect(readSnippets([good])).toEqual([good]);
  });

  // A hand-edited file loses the broken entries; it must not break the whole tool.
  it("bỏ mục thiếu trường hoặc sai kiểu", () => {
    expect(readSnippets([good, { id: "b" }, { ...good, id: "" }, { ...good, template: 7 }])).toEqual(
      [good],
    );
  });

  it("trả mảng rỗng cho thứ không phải mảng", () => {
    expect(readSnippets(null)).toEqual([]);
    expect(readSnippets({ id: "a" })).toEqual([]);
    expect(readSnippets("hỏng")).toEqual([]);
  });
});
