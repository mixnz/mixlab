// Applies the remembered theme before the first paint, so a dark-theme app doesn't flash white
// while the bundle loads. A file of its own rather than an inline script in `index.html`: the app
// runs under a CSP with `script-src 'self'`, which an inline script has no way of satisfying.
(function () {
  try {
    // Always a theme, never absent: *system* is resolved here too, the same way theme.ts does. A
    // colour theme is dark underneath, with its palette named beside it; an unknown palette is left
    // to theme.ts, which reads it as the first one a moment later.
    var stored = localStorage.getItem("mixlab-theme");
    var theme =
      stored === "light" || stored === "dark"
        ? stored
        : stored === "color"
          ? "dark"
          : window.matchMedia("(prefers-color-scheme: dark)").matches
            ? "dark"
            : "light";
    document.documentElement.setAttribute("data-theme", theme);
    if (stored === "color") {
      document.documentElement.setAttribute("data-palette", localStorage.getItem("mixlab-palette") || "navy");
    }
  } catch (e) {}
})();
