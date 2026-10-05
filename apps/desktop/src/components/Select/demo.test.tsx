import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import Select from "./Select";

afterEach(() => vi.unstubAllGlobals());

describe("Select demo hook", () => {
  it("goes on the trigger button, which is what a person clicks", () => {
    // The provider reads the stored language; node has no localStorage.
    vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
    const html = renderToStaticMarkup(
      <I18nProvider>
        <Select value="blog" onChange={() => {}} options={[{ value: "blog", label: "blog" }]} demo="site-project" />
      </I18nProvider>,
    );
    const trigger = /<button[^>]*aria-haspopup="listbox"[^>]*>/.exec(html)?.[0] ?? "";
    expect(trigger).toContain('data-demo="site-project"');
  });
});
