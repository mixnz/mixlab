/**
 * Formats and minifies XML with a hand-written parser.
 *
 * **No `DOMParser`.** It is a browser API, absent from Node, while vitest runs in Node and the repo
 * deliberately has no jsdom — using it would make this the only tool in the module that cannot be
 * tested. In return we also get what `DOMParser` does not give: errors with positions, and the same
 * wording everywhere.
 */

export type XmlNode =
  | { kind: "element"; name: string; attrs: string; selfClosing: boolean; children: XmlNode[] }
  /** The last four kinds pass through verbatim — there is nothing to reprint. */
  | { kind: "text" | "comment" | "cdata" | "pi" | "doctype"; raw: string };

export interface XmlSyntaxError {
  index: number;
  line: number;
  column: number;
  message: string;
}

export type XmlResult = { ok: true; output: string } | { ok: false; error: XmlSyntaxError };

class ScanError extends Error {
  constructor(
    readonly index: number,
    message: string,
  ) {
    super(message);
  }
}

const NAME_END = " \t\n\r/>";

function parse(text: string): XmlNode[] {
  let i = 0;
  const root: XmlNode[] = [];
  const stack: { name: string; children: XmlNode[]; index: number }[] = [];
  const into = (): XmlNode[] => stack[stack.length - 1]?.children ?? root;

  /** Swallows a whole fixed open/close block — comment, CDATA, PI, doctype. */
  const raw = (kind: "comment" | "cdata" | "pi" | "doctype", close: string): void => {
    const start = i;
    const end = text.indexOf(close, i);
    if (end === -1) throw new ScanError(start, `Thiếu "${close}"`);
    i = end + close.length;
    into().push({ kind, raw: text.slice(start, i) });
  };

  while (i < text.length) {
    if (text[i] !== "<") {
      const start = i;
      while (i < text.length && text[i] !== "<") i += 1;
      into().push({ kind: "text", raw: text.slice(start, i) });
      continue;
    }

    if (text.startsWith("<!--", i)) {
      raw("comment", "-->");
    } else if (text.startsWith("<![CDATA[", i)) {
      raw("cdata", "]]>");
    } else if (text.startsWith("<?", i)) {
      raw("pi", "?>");
    } else if (text.startsWith("<!", i)) {
      raw("doctype", ">");
    } else if (text.startsWith("</", i)) {
      const start = i;
      i += 2;
      const nameStart = i;
      while (i < text.length && text[i] !== ">") i += 1;
      if (i >= text.length) throw new ScanError(start, "Thẻ đóng chưa có dấu >");
      const name = text.slice(nameStart, i).trim();
      i += 1;
      const open = stack.pop();
      if (!open) throw new ScanError(start, `Thẻ đóng </${name}> không có thẻ mở`);
      if (open.name !== name) {
        throw new ScanError(start, `Thẻ đóng </${name}> không khớp thẻ mở <${open.name}>`);
      }
    } else {
      const start = i;
      i += 1;
      const nameStart = i;
      while (i < text.length && !NAME_END.includes(text[i]!)) i += 1;
      const name = text.slice(nameStart, i);
      if (name === "") throw new ScanError(start, "Thẻ không có tên");
      // A `>` inside an attribute value is not where the tag ends.
      const attrStart = i;
      let quote = "";
      while (i < text.length) {
        const ch = text[i]!;
        if (quote !== "") {
          if (ch === quote) quote = "";
        } else if (ch === '"' || ch === "'") {
          quote = ch;
        } else if (ch === ">") {
          break;
        }
        i += 1;
      }
      if (i >= text.length) throw new ScanError(start, `Thẻ <${name}> chưa có dấu >`);
      const head = text.slice(attrStart, i).trim();
      const selfClosing = head.endsWith("/");
      const attrs = (selfClosing ? head.slice(0, -1) : head).trim();
      i += 1;
      const node: XmlNode = { kind: "element", name, attrs, selfClosing, children: [] };
      into().push(node);
      if (!selfClosing) stack.push({ name, children: node.children, index: start });
    }
  }

  const unclosed = stack[stack.length - 1];
  if (unclosed) throw new ScanError(unclosed.index, `Thẻ <${unclosed.name}> chưa đóng`);
  return root;
}

type Element = Extract<XmlNode, { kind: "element" }>;

function isBlank(node: XmlNode): boolean {
  return node.kind === "text" && node.raw.trim() === "";
}

/** Has both real text and child elements. Whitespace here is data, not presentation. */
function isMixed(children: XmlNode[]): boolean {
  return (
    children.some((node) => node.kind === "text" && node.raw.trim() !== "") &&
    children.some((node) => node.kind === "element")
  );
}

function openTag(node: Element): string {
  const head = node.attrs === "" ? node.name : `${node.name} ${node.attrs}`;
  return node.selfClosing ? `<${head} />` : `<${head}>`;
}

/** Prints verbatim, without adding or removing a single space. Meant for mixed content. */
function inline(nodes: XmlNode[]): string {
  return nodes
    .map((node) => {
      if (node.kind !== "element") return node.raw;
      if (node.selfClosing) return openTag(node);
      return `${openTag(node)}${inline(node.children)}</${node.name}>`;
    })
    .join("");
}

function print(nodes: XmlNode[], indent: string): string {
  const out: string[] = [];

  const walk = (list: XmlNode[], depth: number): void => {
    for (const node of list) {
      if (isBlank(node)) continue;
      const pad = indent.repeat(depth);
      if (node.kind !== "element") {
        out.push(pad + (node.kind === "text" ? node.raw.trim() : node.raw));
        continue;
      }
      if (node.selfClosing) {
        out.push(pad + openTag(node));
        continue;
      }
      const kept = node.children.filter((child) => !isBlank(child));
      if (kept.length === 0) {
        out.push(`${pad}${openTag(node)}</${node.name}>`);
        continue;
      }
      if (isMixed(node.children)) {
        out.push(`${pad}${openTag(node)}${inline(node.children)}</${node.name}>`);
        continue;
      }
      const only = kept[0]!;
      if (kept.length === 1 && only.kind !== "element") {
        const body = only.kind === "text" ? only.raw.trim() : only.raw;
        out.push(`${pad}${openTag(node)}${body}</${node.name}>`);
        continue;
      }
      out.push(pad + openTag(node));
      walk(node.children, depth + 1);
      out.push(`${pad}</${node.name}>`);
    }
  };

  walk(nodes, 0);
  return out.join("\n");
}

function compact(nodes: XmlNode[]): string {
  return nodes
    .filter((node) => !isBlank(node))
    .map((node) => {
      if (node.kind === "text") return node.raw.trim();
      if (node.kind !== "element") return node.raw;
      if (node.selfClosing) return openTag(node);
      const body = isMixed(node.children) ? inline(node.children) : compact(node.children);
      return `${openTag(node)}${body}</${node.name}>`;
    })
    .join("");
}

function locate(text: string, index: number, message: string): XmlSyntaxError {
  let line = 1;
  let column = 1;
  for (let k = 0; k < index && k < text.length; k += 1) {
    if (text[k] === "\n") {
      line += 1;
      column = 1;
    } else {
      column += 1;
    }
  }
  return { index, line, column, message };
}

function run(text: string, emit: (nodes: XmlNode[]) => string): XmlResult {
  try {
    const nodes = parse(text);
    if (nodes.every(isBlank)) throw new ScanError(0, "Không có gì để đọc");
    return { ok: true, output: emit(nodes) };
  } catch (error) {
    if (!(error instanceof ScanError)) throw error;
    return { ok: false, error: locate(text, error.index, error.message) };
  }
}

export function formatXml(text: string, indent: string): XmlResult {
  return run(text, (nodes) => print(nodes, indent));
}

export function minifyXml(text: string): XmlResult {
  return run(text, compact);
}
