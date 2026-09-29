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
  it("takes parameters in order of appearance", () => {
    expect(paramsOf("cmd -h {{host}} -u {{user}}")).toEqual(["host", "user"]);
  });

  it("does not repeat a parameter used several times", () => {
    expect(paramsOf("{{a}} {{b}} {{a}}")).toEqual(["a", "b"]);
  });

  it("returns an empty array for a template with no parameters", () => {
    expect(paramsOf("docker system prune -af")).toEqual([]);
  });

  it("ignores unpaired braces", () => {
    expect(paramsOf("echo {{a} và {b}}")).toEqual([]);
  });

  it("accepts names with underscores and digits", () => {
    expect(paramsOf("{{local_port}}:{{remote_2}}")).toEqual(["local_port", "remote_2"]);
  });
});

describe("fill", () => {
  it("replaces parameters with values", () => {
    expect(fill("psql -h {{host}} -U {{user}}", { host: "db", user: "an" })).toBe(
      "psql -h db -U an",
    );
  });

  it("replaces every occurrence of the same parameter", () => {
    expect(fill("{{a}}-{{a}}", { a: "x" })).toBe("x-x");
  });

  // An unfilled slot has to be visible in the output, not vanish into whitespace.
  it("keeps a parameter with no value as is", () => {
    expect(fill("cmd {{host}} {{port}}", { host: "db" })).toBe("cmd db {{port}}");
  });

  it("keeps a parameter with an empty value as is", () => {
    expect(fill("cmd {{host}}", { host: "" })).toBe("cmd {{host}}");
  });

  // The tool does not add quotes: the template's author decides where quotes are needed.
  it("does not quote a value containing spaces", () => {
    expect(fill("mysql -p'{{password}}'", { password: "mật khẩu" })).toBe("mysql -p'mật khẩu'");
  });
});

const draft = { title: "Dump", group: "mysql", template: "mysqldump {{db}}" };

describe("list operations", () => {
  it("appends at the end and assigns an id", () => {
    const list = addSnippet([], draft);
    expect(list).toHaveLength(1);
    expect(list[0]?.title).toBe("Dump");
    expect(list[0]?.id).not.toBe("");
  });

  it("does not give two snippets the same id", () => {
    const list = addSnippet(addSnippet([], draft), draft);
    expect(list[0]?.id).not.toBe(list[1]?.id);
  });

  it("edits exactly one entry and keeps its id", () => {
    const list = addSnippet([], draft);
    const id = list[0]!.id;
    const after = updateSnippet(list, id, { ...draft, title: "Khác" });
    expect(after[0]?.id).toBe(id);
    expect(after[0]?.title).toBe("Khác");
  });

  it("ignores an edit to an id that does not exist", () => {
    const list = addSnippet([], draft);
    expect(updateSnippet(list, "khong-co", draft)).toEqual(list);
  });

  it("deletes exactly one entry", () => {
    const list = addSnippet(addSnippet([], draft), { ...draft, title: "Hai" });
    const after = removeSnippet(list, list[0]!.id);
    expect(after).toHaveLength(1);
    expect(after[0]?.title).toBe("Hai");
  });

  it("does not touch the original array", () => {
    const list: Snippet[] = addSnippet([], draft);
    const copy = [...list];
    removeSnippet(list, list[0]!.id);
    expect(list).toEqual(copy);
  });
});

describe("readSnippets", () => {
  const good = { id: "a", title: "T", group: "g", template: "cmd" };

  it("keeps entries with the right shape", () => {
    expect(readSnippets([good])).toEqual([good]);
  });

  // A hand-edited file loses the broken entries; it must not break the whole tool.
  it("drops entries missing a field or with the wrong type", () => {
    expect(readSnippets([good, { id: "b" }, { ...good, id: "" }, { ...good, template: 7 }])).toEqual(
      [good],
    );
  });

  it("returns an empty array for something that is not an array", () => {
    expect(readSnippets(null)).toEqual([]);
    expect(readSnippets({ id: "a" })).toEqual([]);
    expect(readSnippets("hỏng")).toEqual([]);
  });
});
