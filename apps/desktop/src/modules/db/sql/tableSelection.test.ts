import { describe, expect, it } from "vitest";
import { NO_TABLE, tableSelection, type TableSelection } from "./tableSelection";

const followed: TableSelection = { table: "orders", pinned: "orders" };

describe("tableSelection", () => {
  it("pins the table a foreign key was followed to", () => {
    expect(tableSelection(NO_TABLE, { type: "follow", table: "orders" })).toEqual(followed);
  });

  it("keeps the pin while the pinned table is picked, and takes it down for any other", () => {
    expect(tableSelection(followed, { type: "select", table: "orders" })).toEqual(followed);
    expect(tableSelection(followed, { type: "select", table: "customers" })).toEqual({ table: "customers", pinned: null });
  });

  it("opens a table named outside the sidebar without a pin", () => {
    expect(tableSelection(followed, { type: "open", table: "items" })).toEqual({ table: "items", pinned: null });
  });

  it("lets go of a pin the list no longer holds", () => {
    expect(tableSelection(followed, { type: "listed", tables: ["customers"] })).toEqual({ table: "orders", pinned: null });
    const same = tableSelection(followed, { type: "listed", tables: ["orders"] });
    expect(same).toBe(followed);
  });

  it("follows a rename with both the selection and the pin", () => {
    expect(tableSelection(followed, { type: "renamed", from: "orders", to: "sales" })).toEqual({ table: "sales", pinned: "sales" });
    const other: TableSelection = { table: "customers", pinned: null };
    expect(tableSelection(other, { type: "renamed", from: "orders", to: "sales" })).toBe(other);
  });

  it("leaves nothing open on a dropped table", () => {
    expect(tableSelection(followed, { type: "dropped", table: "orders" })).toEqual(NO_TABLE);
    const created: TableSelection = { table: "new_one", pinned: "orders" };
    expect(tableSelection(created, { type: "dropped", table: "new_one" })).toEqual({ table: null, pinned: "orders" });
  });

  it("selects a table just created and keeps a pin the person has not left yet", () => {
    expect(tableSelection(followed, { type: "created", table: "new_one" })).toEqual({ table: "new_one", pinned: "orders" });
  });

  it("clears both when the database changes", () => {
    expect(tableSelection(followed, { type: "cleared" })).toEqual(NO_TABLE);
  });
});
