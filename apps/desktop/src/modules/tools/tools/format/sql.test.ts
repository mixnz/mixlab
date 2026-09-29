import { describe, expect, it } from "vitest";
import { minifySql } from "./sql";

describe("minifySql", () => {
  it("collapses whitespace and newlines into one space", () => {
    expect(minifySql("SELECT   a,\n       b\nFROM   t")).toBe("SELECT a, b FROM t");
  });

  // Collapsing whitespace inside a string changes the data, it does not tidy the statement.
  it("does not touch whitespace inside strings", () => {
    expect(minifySql("SELECT  'a   b'  FROM t")).toBe("SELECT 'a   b' FROM t");
  });

  it("understands doubled quotes inside strings", () => {
    expect(minifySql("SELECT 'it''s   ok'   FROM t")).toBe("SELECT 'it''s   ok' FROM t");
  });

  it("keeps backtick- and double-quoted identifiers intact", () => {
    expect(minifySql('SELECT `a  b`,  "c  d"  FROM t')).toBe('SELECT `a  b`, "c  d" FROM t');
  });

  // Swallowing half a comment line turns the rest of the statement into a comment.
  it("drops a whole single-line comment", () => {
    expect(minifySql("SELECT a -- lấy cột a\nFROM t")).toBe("SELECT a FROM t");
  });

  it("drops block comments", () => {
    expect(minifySql("SELECT /* ghi chú */ a FROM t")).toBe("SELECT a FROM t");
  });

  it("does not mistake dashes inside a string for a comment", () => {
    expect(minifySql("SELECT '-- không phải comment' FROM t")).toBe(
      "SELECT '-- không phải comment' FROM t",
    );
  });
});
