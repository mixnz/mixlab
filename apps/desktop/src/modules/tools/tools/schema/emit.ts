import { convert } from "../case/caseConvert";
import type { SqlDialect } from "../convert/insert";
import type { Field, JsonType } from "./infer";

/**
 * The three code generators working from `inferSchema`'s result.
 *
 * Naming reuses the Case converter tool's `convert()` — `snake_case` for SQL columns, `PascalCase`
 * for Go fields, unchanged for TypeScript. This is the first time two tools in the module call each
 * other, and it goes in the right direction: pure logic calling pure logic.
 */

export interface CreateTableOptions {
  table: string;
  dialect: SqlDialect;
}

/** A field's single type, with `null` taken out. `null` means mixed types or only nulls. */
function soleType(field: Field): JsonType | null {
  const types = field.types.filter((type) => type !== "null");
  return types.length === 1 ? types[0]! : null;
}

function nullable(field: Field): boolean {
  return field.optional || field.types.includes("null");
}

function sqlType(field: Field, dialect: SqlDialect): string {
  const type = soleType(field);
  if (type === null) return "TEXT";
  if (type === "object" || type === "array") return dialect === "mysql" ? "JSON" : "JSONB";
  if (type === "boolean") return dialect === "mysql" ? "TINYINT(1)" : "BOOLEAN";
  // A sample is only a sample: an INT column overflowing at the two-billionth record is something
  // to fix in production.
  if (type === "integer") return "BIGINT";
  if (type === "number") return dialect === "mysql" ? "DOUBLE" : "DOUBLE PRECISION";
  if (type === "string") {
    if (field.isoLike) return dialect === "mysql" ? "DATETIME" : "TIMESTAMPTZ";
    return "VARCHAR(255)";
  }
  return "TEXT";
}

export function toCreateTable(fields: Field[], options: CreateTableOptions): string {
  const ident = (name: string): string =>
    options.dialect === "mysql" ? `\`${name}\`` : `"${name}"`;
  const lines = fields.map((field) => {
    const column = ident(convert(field.name, "snake"));
    const suffix = nullable(field) ? "" : " NOT NULL";
    return `  ${column} ${sqlType(field, options.dialect)}${suffix}`;
  });
  // The table name is kept exactly as the user typed it — they have already chosen it.
  return `CREATE TABLE ${ident(options.table)} (\n${lines.join(",\n")}\n);`;
}

const TS_TYPE: Record<JsonType, string> = {
  string: "string",
  number: "number",
  integer: "number",
  boolean: "boolean",
  null: "null",
  object: "Record<string, unknown>",
  array: "unknown[]",
  unknown: "unknown",
};

const IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

export function toTypeScript(fields: Field[], rootName: string): string {
  const blocks: string[] = [];

  function emit(list: Field[], name: string): void {
    const lines = list.map((field) => {
      const key = IDENTIFIER.test(field.name) ? field.name : JSON.stringify(field.name);
      return `  ${key}${field.optional ? "?" : ""}: ${tsType(field, name)};`;
    });
    blocks.push(`export interface ${name} {\n${lines.join("\n")}\n}`);
  }

  function tsType(field: Field, parent: string): string {
    const suffix = field.types.includes("null") ? " | null" : "";
    const type = soleType(field);
    if (type === null) return `unknown${suffix}`;
    if ((type === "object" || type === "array") && field.children) {
      const child = `${parent}${convert(field.name, "pascal")}`;
      emit(field.children, child);
      return type === "array" ? `${child}[]${suffix}` : `${child}${suffix}`;
    }
    return `${TS_TYPE[type]}${suffix}`;
  }

  emit(fields, rootName);
  // `emit` pushes a parent block after the child blocks it produces, so reverse to put the root
  // first.
  return blocks.reverse().join("\n\n");
}

const GO_TYPE: Record<JsonType, string> = {
  string: "string",
  number: "float64",
  integer: "int64",
  boolean: "bool",
  null: "any",
  object: "map[string]any",
  array: "[]any",
  unknown: "any",
};

export function toGoStruct(fields: Field[], rootName: string): string {
  const blocks: string[] = [];

  function emit(list: Field[], name: string): void {
    const lines = list.map(
      (field) =>
        `\t${convert(field.name, "pascal")} ${goType(field, name)} \`json:"${field.name}"\``,
    );
    blocks.push(`type ${name} struct {\n${lines.join("\n")}\n}`);
  }

  function goType(field: Field, parent: string): string {
    const pointer = nullable(field) ? "*" : "";
    const type = soleType(field);
    if (type === null) return "any";
    if ((type === "object" || type === "array") && field.children) {
      const child = `${parent}${convert(field.name, "pascal")}`;
      emit(field.children, child);
      // A slice is already a nil-able type, so no pointer is added.
      return type === "array" ? `[]${child}` : `${pointer}${child}`;
    }
    return type === "array" ? "[]any" : `${pointer}${GO_TYPE[type]}`;
  }

  emit(fields, rootName);
  return blocks.reverse().join("\n\n");
}
