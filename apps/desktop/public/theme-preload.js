// Applies the remembered theme before the first paint, so a dark-theme app doesn't flash white
// while the bundle loads. A file of its own rather than an inline script in `index.html`: the app
// runs under a CSP with `script-src 'self'`, which an inline script has no way of satisfying.
(function () {
  try {
    // Always a theme, never absent: *system* is resolved here too, the same way theme.ts does.
    var stored = localStorage.getItem("mixlab-theme");
    var theme =
      stored === "light" || stored === "dark"
        ? stored
        : window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light";
    document.documentElement.setAttribute("data-theme", theme);
  } catch (e) {}
})();
