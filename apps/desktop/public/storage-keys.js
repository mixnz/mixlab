// Moves the window's own localStorage keys from the names the standalone client gave them —
// roadmap task T176g. A classic script of its own, loaded first by both index.html and tray.html:
// theme-preload.js, theme.ts on import and the tray page all read these keys before any module
// could call a function, and the tray may be the first page of a run.
//
// For each pair: the new key keeps its value when it has one, the old value is copied across when
// it does not, and the old key is removed either way — so after one page load there is one prefix.
// The retired glass key and the old terminal font-size key are not here: each is read once more by
// its own code and deleted there.
(function () {
  var MOVED = [
    ["mixdb-theme", "mixlab-theme"],
    ["mixdb-accent", "mixlab-accent"],
    ["mixdb-lang", "mixlab-lang"],
    ["mixdb-modules", "mixlab-modules"],
    ["mixdb-session", "mixlab-session"],
  ];
  try {
    for (var i = 0; i < MOVED.length; i++) {
      var value = localStorage.getItem(MOVED[i][0]);
      if (value === null) continue;
      if (localStorage.getItem(MOVED[i][1]) === null) localStorage.setItem(MOVED[i][1], value);
      localStorage.removeItem(MOVED[i][0]);
    }
  } catch (e) {}
})();
