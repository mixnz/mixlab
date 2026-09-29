import { describe, expect, it } from "vitest";
import { formatXml, minifyXml } from "./xml";

const out = (result: ReturnType<typeof formatXml>): string => (result.ok ? result.output : "");

describe("formatXml", () => {
  it("indents by tree", () => {
    expect(formatXml("<a><b><c>1</c></b></a>", "  ")).toEqual({
      ok: true,
      output: "<a>\n  <b>\n    <c>1</c>\n  </b>\n</a>",
    });
  });

  it("keeps attributes and self-closing tags", () => {
    expect(out(formatXml('<r><img src="a>b.png" /></r>', "  "))).toBe(
      '<r>\n  <img src="a>b.png" />\n</r>',
    );
  });

  // Whitespace in mixed content *is* data: adding a newline in the middle changes the document.
  it("does not reindent mixed content", () => {
    expect(out(formatXml("<doc><p>xin <b>chào</b> bạn</p></doc>", "  "))).toBe(
      "<doc>\n  <p>xin <b>chào</b> bạn</p>\n</doc>",
    );
  });

  it("keeps comments, CDATA and processing instructions as they are", () => {
    const source = '<?xml version="1.0"?><r><!-- ghi chú --><d><![CDATA[a < b]]></d></r>';
    expect(out(formatXml(source, "  "))).toBe(
      '<?xml version="1.0"?>\n<r>\n  <!-- ghi chú -->\n  <d><![CDATA[a < b]]></d>\n</r>',
    );
  });

  it("collapses text-only elements onto one line", () => {
    expect(out(formatXml("<r>\n  <a>\n    xin chào\n  </a>\n</r>", "  "))).toBe(
      "<r>\n  <a>xin chào</a>\n</r>",
    );
  });
});

describe("syntax errors", () => {
  it("catches a mismatched closing tag and points at the right place", () => {
    const result = formatXml("<a><b></c></a>", "  ");
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error.message).toContain("b");
    expect(result.error.index).toBe(6);
  });

  it("catches an unclosed tag", () => {
    expect(formatXml("<a><b></a>", "  ").ok).toBe(false);
  });

  it("catches a closing tag with no opening tag", () => {
    expect(formatXml("</a>", "  ").ok).toBe(false);
  });
});

describe("minifyXml", () => {
  it("removes whitespace between tags", () => {
    expect(minifyXml("<a>\n  <b>1</b>\n  <c>2</c>\n</a>")).toEqual({
      ok: true,
      output: "<a><b>1</b><c>2</c></a>",
    });
  });

  it("does not touch mixed content", () => {
    expect(out(minifyXml("<doc>\n  <p>xin <b>chào</b> bạn</p>\n</doc>"))).toBe(
      "<doc><p>xin <b>chào</b> bạn</p></doc>",
    );
  });
});
