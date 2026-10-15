import { describe, expect, it } from "vitest";
import {
  arityLookup,
  createFilterRow,
  initialFilterRows,
  isFilterComplete,
  splitFilterList,
  toQueryFilters,
  type FilterOperatorSpec,
} from "./filters";

/**
 * The cases `split_list_parts` in `src-tauri/src/modules/db/drivers/filters.rs` is checked
 * against, written out here in the same order.
 *
 * The two are a pair by claim — `splitFilterList` says so in its own doc comment — and the claim
 * matters: this side decides whether a row is complete enough to send, and the Rust side decides
 * what the row then matches. If they disagree about how many items a value holds, a `BETWEEN` row
 * is sent as filled in and comes back matching a range the user never asked for.
 */
const AGREED_WITH_RUST: [raw: string, items: string[]][] = [
  ["1, 2 ,3", ["1", "2", "3"]],
  // A trailing comma names no item, so `1,2,` is two items and not three.
  ["1,2,", ["1", "2"]],
  ["   ", []],
  // Quotes are how a value carrying a comma — or spaces that matter — gets through in one piece,
  // and the only way to ask for the empty string.
  ["'a,b', c", ["a,b", "c"]],
  ["' padded '", [" padded "]],
  ["''", [""]],
  ['"double", \'single\'', ["double", "single"]],
  // The spaces a list is typed with are around the items, not in them.
  ["'a' , 'b'", ["a", "b"]],
  ["  'a'  ", ["a"]],
];

describe("splitFilterList", () => {
  it("splits a value the same way the Rust side does", () => {
    for (const [raw, items] of AGREED_WITH_RUST) {
      expect(splitFilterList(raw), `for ${JSON.stringify(raw)}`).toEqual(items);
    }
  });

  it("takes an unterminated quote as running to the end", () => {
    // Not a case the Rust tests name, but both are written the same way and this is what a list
    // looks like while it is still being typed.
    expect(splitFilterList("'a,b")).toEqual(["a,b"]);
  });

  it("keeps a comma inside quotes out of the count", () => {
    // The whole point of the shared rule: this is one item, so a `BETWEEN` row holding it is not
    // complete and is not sent.
    expect(splitFilterList("'a,b'")).toHaveLength(1);
    expect(isFilterComplete("pair", "'a,b'")).toBe(false);
    expect(isFilterComplete("pair", "a,b")).toBe(true);
  });
});

describe("isFilterComplete", () => {
  it("lets an operator that needs no value through", () => {
    expect(isFilterComplete("none", "")).toBe(true);
  });

  it("wants something typed for a single value", () => {
    // The bar opens with an empty `id =` row, and that must not mean "the rows whose id is the
    // empty string".
    expect(isFilterComplete("one", "")).toBe(false);
    expect(isFilterComplete("one", "0")).toBe(true);
    // A space is a value: `' '` would be the way to say "not a value", and this is not that.
    expect(isFilterComplete("one", " ")).toBe(true);
  });

  it("wants at least one item for a list and two bounds for a pair", () => {
    expect(isFilterComplete("list", "")).toBe(false);
    expect(isFilterComplete("list", ",,")).toBe(false);
    expect(isFilterComplete("list", "1")).toBe(true);
    // A quoted empty string is an item, which is the only way to ask for one.
    expect(isFilterComplete("list", "''")).toBe(true);

    expect(isFilterComplete("pair", "1")).toBe(false);
    expect(isFilterComplete("pair", "1,2")).toBe(true);
    expect(isFilterComplete("pair", "1,2,3")).toBe(true);
  });
});

describe("arityLookup", () => {
  const OPERATORS: FilterOperatorSpec[] = [
    { id: "eq", arity: "one" },
    { id: "in", arity: "list" },
    { id: "isNull", arity: "none" },
  ];

  it("answers from the list it was built with", () => {
    const arityOf = arityLookup(OPERATORS);
    expect(arityOf("eq")).toBe("one");
    expect(arityOf("in")).toBe("list");
    expect(arityOf("isNull")).toBe("none");
  });

  it("treats an operator it has never heard of as taking one value", () => {
    // A row is then judged by whether anything was typed into it, which is the answer that keeps
    // a half-known operator out of the query rather than sending it empty.
    expect(arityLookup(OPERATORS)("nonsense")).toBe("one");
  });
});

describe("the row a bar opens with", () => {
  it("starts on the id column, whichever way it is spelled", () => {
    expect(createFilterRow(["name", "id", "age"], "eq").column).toBe("id");
    // Mongo's `_id` counts, and so does a column named in capitals.
    expect(createFilterRow(["name", "_id"], "eq").column).toBe("_id");
    expect(createFilterRow(["name", "ID"], "eq").column).toBe("ID");
  });

  it("falls back to the first column so the row never points at nothing", () => {
    expect(createFilterRow(["name", "age"], "eq").column).toBe("name");
    expect(createFilterRow([], "eq").column).toBe("");
  });

  it("gives every row an identity of its own", () => {
    // Two rows may otherwise be equal, and a row's position changes as the ones above it go.
    const first = createFilterRow(["id"], "eq");
    const second = createFilterRow(["id"], "eq");
    expect(first.id).not.toBe(second.id);
  });

  it("opens with one row only when there is an id to look up by", () => {
    expect(initialFilterRows(["id", "name"], "eq")).toHaveLength(1);
    // No id column is no column to guess at, and an arbitrary one would only be in the way.
    expect(initialFilterRows(["name", "age"], "eq")).toEqual([]);
  });
});

describe("toQueryFilters", () => {
  const arityOf = arityLookup([
    { id: "eq", arity: "one" },
    { id: "between", arity: "pair" },
    { id: "isNull", arity: "none" },
  ]);

  it("sends the rows that are switched on and filled in, and only those", () => {
    const rows = [
      { id: 1, enabled: true, column: "id", operator: "eq", value: "7" },
      // Written down but unapplied.
      { id: 2, enabled: false, column: "name", operator: "eq", value: "ann" },
      // Still waiting for a value.
      { id: 3, enabled: true, column: "age", operator: "eq", value: "" },
      // Half a range says nothing about a range.
      { id: 4, enabled: true, column: "age", operator: "between", value: "18" },
      // Needs no value at all.
      { id: 5, enabled: true, column: "nickname", operator: "isNull", value: "" },
      // No column chosen.
      { id: 6, enabled: true, column: "", operator: "eq", value: "x" },
    ];

    expect(toQueryFilters(rows, arityOf)).toEqual([
      { column: "id", operator: "eq", value: "7" },
      { column: "nickname", operator: "isNull", value: "" },
    ]);
  });

  it("drops the parts of a row only the bar cares about", () => {
    const [only] = toQueryFilters(
      [{ id: 9, enabled: true, column: "id", operator: "eq", value: "7" }],
      arityOf,
    );
    expect(Object.keys(only).sort()).toEqual(["column", "operator", "value"]);
  });
});
