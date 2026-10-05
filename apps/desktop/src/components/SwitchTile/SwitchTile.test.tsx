import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import SwitchTile from "./SwitchTile";

describe("SwitchTile", () => {
  it("writes its demo hook on the tile, and nothing without one", () => {
    const withHook = renderToStaticMarkup(
      <SwitchTile label="HTTPS" checked={false} onChange={() => {}} demo="site-https" />,
    );
    expect(withHook).toMatch(/<label[^>]*data-demo="site-https"/);
    const without = renderToStaticMarkup(<SwitchTile label="HTTPS" checked={false} onChange={() => {}} />);
    expect(without).not.toContain("data-demo");
  });
});
