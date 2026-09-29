import { describe, expect, it } from "vitest";
import { formatXml, minifyXml } from "./xml";

const out = (result: ReturnType<typeof formatXml>): string => (result.ok ? result.output : "");

describe("formatXml", () => {
  it("thụt lề theo cây", () => {
    expect(formatXml("<a><b><c>1</c></b></a>", "  ")).toEqual({
      ok: true,
      output: "<a>\n  <b>\n    <c>1</c>\n  </b>\n</a>",
    });
  });

  it("giữ thuộc tính và thẻ tự đóng", () => {
    expect(out(formatXml('<r><img src="a>b.png" /></r>', "  "))).toBe(
      '<r>\n  <img src="a>b.png" />\n</r>',
    );
  });

  // Whitespace in mixed content *is* data: adding a newline in the middle changes the document.
  it("không thụt lề lại nội dung hỗn hợp", () => {
    expect(out(formatXml("<doc><p>xin <b>chào</b> bạn</p></doc>", "  "))).toBe(
      "<doc>\n  <p>xin <b>chào</b> bạn</p>\n</doc>",
    );
  });

  it("giữ comment, CDATA và processing instruction nguyên trạng", () => {
    const source = '<?xml version="1.0"?><r><!-- ghi chú --><d><![CDATA[a < b]]></d></r>';
    expect(out(formatXml(source, "  "))).toBe(
      '<?xml version="1.0"?>\n<r>\n  <!-- ghi chú -->\n  <d><![CDATA[a < b]]></d>\n</r>',
    );
  });

  it("gom phần tử chỉ có text về một dòng", () => {
    expect(out(formatXml("<r>\n  <a>\n    xin chào\n  </a>\n</r>", "  "))).toBe(
      "<r>\n  <a>xin chào</a>\n</r>",
    );
  });
});

describe("lỗi cú pháp", () => {
  it("bắt thẻ đóng lệch tên và chỉ đúng chỗ", () => {
    const result = formatXml("<a><b></c></a>", "  ");
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error.message).toContain("b");
    expect(result.error.index).toBe(6);
  });

  it("bắt thẻ chưa đóng", () => {
    expect(formatXml("<a><b></a>", "  ").ok).toBe(false);
  });

  it("bắt thẻ đóng không có thẻ mở", () => {
    expect(formatXml("</a>", "  ").ok).toBe(false);
  });
});

describe("minifyXml", () => {
  it("bỏ khoảng trắng giữa các thẻ", () => {
    expect(minifyXml("<a>\n  <b>1</b>\n  <c>2</c>\n</a>")).toEqual({
      ok: true,
      output: "<a><b>1</b><c>2</c></a>",
    });
  });

  it("không đụng vào nội dung hỗn hợp", () => {
    expect(out(minifyXml("<doc>\n  <p>xin <b>chào</b> bạn</p>\n</doc>"))).toBe(
      "<doc><p>xin <b>chào</b> bạn</p></doc>",
    );
  });
});
