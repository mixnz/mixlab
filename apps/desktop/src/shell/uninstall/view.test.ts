import { describe, expect, it } from "vitest";
import type { Plan, PlanRow } from "./api";
import { blockedRows, canRemove, declined, failedRows, relocatedRows } from "./view";

const row = (id: string, removal: string, extra: Record<string, string> = {}): PlanRow => ({
  id,
  what: id,
  location: `/${id}`,
  outcome: { removal, ...extra },
});

describe("the Remove MixLab dialog's state", () => {
  it("lists what blocks, and will not remove while anything does", () => {
    const plan: Plan = { items: [row("home", "planned"), row("in_use", "blocked", { by: "node (pid 1)" })] };
    expect(blockedRows(plan)).toEqual([{ what: "in_use", location: "/in_use", by: "node (pid 1)" }]);
    expect(canRemove("plan", plan)).toBe(false);
    expect(canRemove("plan", { items: [row("home", "planned")] })).toBe(true);
    expect(canRemove("checking", null)).toBe(false);
    expect(canRemove("removing", { items: [row("home", "planned")] })).toBe(false);
  });

  it("names the folders moved out of the home", () => {
    const plan: Plan = { items: [row("relocated_directory", "planned"), row("home", "planned")] };
    expect(relocatedRows(plan)).toEqual(["/relocated_directory"]);
  });

  it("a declined prompt leaves the dialog open with nothing removed", () => {
    const report: Plan = { items: [row("hosts_block", "enqueued", { what: "" }), row("home", "kept", { because: "" })] };
    expect(declined(report)).toBe(true);
    expect(failedRows(report)).toEqual([]);
    expect(declined({ items: [row("hosts_block", "removed", { what: "" })] })).toBe(false);
  });

  it("a declined prompt reads as failed rows still waiting, and nothing removed", () => {
    // What the daemon reports after a declined grant (T182b, D7): the privileged rows are failed,
    // still waiting for permission, and nothing was removed or is going.
    const report: Plan = {
      items: [
        row("resolver_wiring", "failed", { because: "this is still here, and the operation that removes it is still waiting for permission" }),
        row("package", "failed", { because: "this is still here, and the operation that removes it is still waiting for permission" }),
        row("home", "kept", { because: "you asked" }),
      ],
    };
    expect(declined(report)).toBe(true);
    expect(declined({ items: [row("package", "failed", { because: "still here" }), row("home", "on_exit")] })).toBe(false);
  });

  it("names the rows still there after the act", () => {
    const report: Plan = { items: [row("package", "failed", { because: "still here" }), row("home", "on_exit")] };
    expect(failedRows(report).map((r) => r.id)).toEqual(["package"]);
    expect(declined(report)).toBe(false);
  });
});
