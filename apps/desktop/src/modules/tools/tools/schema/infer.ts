/**
 * Infers the shape of a JSON sample.
 *
 * This is the part of the Schema generator tool worth testing; the three code generators in
 * `emit.ts` only print what this function has concluded. The sample is an object, or an array of
 * objects — for an array the keys are merged, and a key absent from any element is `optional`.
 */

export type JsonType =
  | "string"
  | "number"
  | "integer"
  | "boolean"
  | "null"
  | "object"
  | "array"
  | "unknown";

export interface Field {
  /** The key exactly as in the JSON. Renaming is the code generator's job. */
  name: string;
  types: JsonType[];
  /** The key is absent from at least one element of the sample array. */
  optional: boolean;
  /** Every string value seen looks like ISO 8601 — a time column, not a `VARCHAR`. */
  isoLike: boolean;
  /** For an object: the child fields. For an array of objects: the element's shape. */
  children?: Field[];
}

/**
 * Deliberately stricter than `Date.parse`.
 *
 * `timestamp/time.ts` has no ISO detection function to reuse — it calls `Date.parse`, and
 * `Date.parse("2026")` is valid. A column full of four-digit strings is not a time column.
 */
const ISO_8601 = /^\d{4}-\d{2}-\d{2}([T ]\d{2}:\d{2}(:\d{2})?(\.\d+)?(Z|[+-]\d{2}:?\d{2})?)?$/;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function typeOf(value: unknown): JsonType {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  if (typeof value === "string") return "string";
  if (typeof value === "boolean") return "boolean";
  if (typeof value === "number") return Number.isInteger(value) ? "integer" : "number";
  if (typeof value === "object") return "object";
  return "unknown";
}

function widen(types: JsonType[]): JsonType[] {
  if (types.includes("number") && types.includes("integer")) {
    return types.filter((type) => type !== "integer");
  }
  return types;
}

function childrenOf(values: unknown[]): Field[] | undefined {
  const objects = values.filter(isRecord);
  if (objects.length > 0) return fieldsOf(objects);
  const items = values.filter(Array.isArray).flat().filter(isRecord);
  if (items.length > 0) return fieldsOf(items);
  return undefined;
}

function fieldsOf(samples: Record<string, unknown>[]): Field[] {
  const order: string[] = [];
  const bag = new Map<string, unknown[]>();

  for (const sample of samples) {
    for (const [key, value] of Object.entries(sample)) {
      let values = bag.get(key);
      if (!values) {
        values = [];
        bag.set(key, values);
        order.push(key);
      }
      values.push(value);
    }
  }

  return order.map((name) => {
    const values = bag.get(name)!;
    const types: JsonType[] = [];
    for (const value of values) {
      const type = typeOf(value);
      if (!types.includes(type)) types.push(type);
    }
    const strings = values.filter((value): value is string => typeof value === "string");
    const field: Field = {
      name,
      types: widen(types),
      optional: values.length < samples.length,
      isoLike: strings.length > 0 && strings.every((value) => ISO_8601.test(value)),
    };
    const children = childrenOf(values);
    if (children) field.children = children;
    return field;
  });
}

export function inferSchema(value: unknown): Field[] | null {
  if (isRecord(value)) return fieldsOf([value]);
  if (Array.isArray(value)) {
    const rows = value.filter(isRecord);
    return rows.length > 0 ? fieldsOf(rows) : null;
  }
  return null;
}
