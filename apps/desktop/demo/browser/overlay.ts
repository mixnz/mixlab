/**
 * A browser window over MixLab, for the clip that ends by opening the site it built.
 *
 * MixLab opens a URL through the opener plugin; the demo's fixture for it dispatches
 * `demo:open-url`, and this draws a window — title bar, address bar with a lock, a loading bar —
 * showing `laravel-welcome.html`. Only when the clip asked for it (`fixtures.browser`), so a
 * screenshot never sees it. Its colours follow `prefers-color-scheme`, as the page inside does; the
 * clip rig sets that to the clip's theme.
 */

export const BROWSER_ID = "__demo-browser";
const PAGE = "/demo/browser/laravel-welcome.html";
const SLIDE_MS = 450;
const LOAD_MS = 600;

const LOCK =
  '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" ' +
  'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="11" width="16" height="10" rx="2"/>' +
  '<path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>';

function palette(dark: boolean) {
  return dark
    ? { frame: "#1f1f1e", bar: "#2a2a28", field: "#121211", text: "#e8e8e3", muted: "#9a9a93", accent: "#3fb950" }
    : { frame: "#f3f3f1", bar: "#e6e6e3", field: "#ffffff", text: "#1b1b18", muted: "#706f6c", accent: "#1a7f37" };
}

function open(doc: Document, url: string, dark: boolean): void {
  doc.getElementById(BROWSER_ID)?.remove();
  const c = palette(dark);
  const shown = url.replace(/^https:\/\//, "");

  const browser = doc.createElement("div");
  browser.id = BROWSER_ID;
  browser.setAttribute("aria-hidden", "true");
  Object.assign(browser.style, {
    position: "fixed",
    left: "11%",
    right: "11%",
    top: "9%",
    bottom: "5%",
    zIndex: "2147483646",
    display: "flex",
    flexDirection: "column",
    borderRadius: "12px",
    overflow: "hidden",
    background: c.frame,
    boxShadow: "0 30px 80px rgba(0,0,0,.35), 0 0 0 1px rgba(0,0,0,.12)",
    font: "13px/1.2 system-ui, -apple-system, 'Segoe UI', sans-serif",
    color: c.text,
  });
  browser.innerHTML =
    `<div style="display:flex;align-items:center;gap:14px;padding:10px 14px;background:${c.bar}">` +
    `<span style="display:flex;gap:7px">` +
    ["#ff5f57", "#febc2e", "#28c840"]
      .map((dot) => `<i style="width:12px;height:12px;border-radius:50%;background:${dot};display:block"></i>`)
      .join("") +
    `</span>` +
    `<span style="flex:1;display:flex;align-items:center;gap:7px;justify-content:center;max-width:520px;margin:0 auto;` +
    `padding:6px 12px;border-radius:8px;background:${c.field};color:${c.text}">` +
    `<span style="color:${c.accent};display:flex">${LOCK}</span>` +
    `<span style="color:${c.muted}">https://</span><span style="margin-left:-7px">${shown}</span></span>` +
    `<span style="width:47px"></span></div>` +
    `<div style="height:2px;background:transparent"><div data-loading style="height:2px;width:0;background:${c.accent}"></div></div>` +
    `<iframe src="${PAGE}" title="${shown}" style="flex:1;width:100%;border:0;background:${dark ? "#0a0a0a" : "#FDFDFC"}"></iframe>`;
  doc.body.appendChild(browser);

  browser.animate(
    [
      { opacity: 0, transform: "translateY(40px) scale(.98)" },
      { opacity: 1, transform: "none" },
    ],
    { duration: SLIDE_MS, easing: "cubic-bezier(.2,.8,.2,1)" },
  );
  browser
    .querySelector<HTMLElement>("[data-loading]")
    ?.animate([{ width: "0%", opacity: 1 }, { width: "100%", opacity: 1 }, { width: "100%", opacity: 0 }], {
      duration: LOAD_MS,
      easing: "ease-out",
      fill: "forwards",
    });
}

export function installBrowserOverlay(win: Window): void {
  if (win.__demoFixtures?.browser !== true) return;
  win.addEventListener("demo:open-url", (event) => {
    const url = (event as CustomEvent<string>).detail;
    const dark = win.matchMedia("(prefers-color-scheme: dark)").matches;
    open(win.document, url, dark);
  });
}
