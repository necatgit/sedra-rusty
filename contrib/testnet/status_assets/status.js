// SEDRA (#13): optional extras for the testnet status page. The page works without this script; it only makes the
// countdowns tick, warns when the data has stopped updating, and offers a light/dark switch.
(function () {
  "use strict";

  function span(s) {
    s = Math.max(0, Math.floor(s));
    var d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
    if (d) return d + " d " + h + " h " + m + " min";
    if (h) return h + " h " + m + " min " + r + " s";
    if (m) return m + " min " + r + " s";
    return r + " s";
  }

  var dataTime = Number(document.body.getAttribute("data-data-time")) || 0;
  var stale = document.getElementById("stale");
  var countdowns = Array.prototype.slice.call(document.querySelectorAll("[data-eta]")).filter(function (el) {
    return Number(el.getAttribute("data-eta")) > 0;
  });

  function tick() {
    var now = Date.now() / 1000;
    countdowns.forEach(function (el) {
      var left = Number(el.getAttribute("data-eta")) - now;
      el.textContent = left > 0 ? "in about " + span(left) : "any moment now";
    });
    // the page is rebuilt every minute; if the visitor's copy or the data behind it is old, say so
    if (stale && dataTime && now - dataTime > 300) {
      stale.hidden = false;
      stale.textContent = "The monitor data is " + span(now - dataTime) + " old: this page may be out of date.";
    }
  }
  tick();
  setInterval(tick, 1000);

  var btn = document.getElementById("theme-btn");
  if (btn) {
    btn.hidden = false;
    btn.addEventListener("click", function () {
      var root = document.documentElement;
      var current = root.getAttribute("data-theme") ||
        (window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
      var next = current === "light" ? "dark" : "light";
      root.setAttribute("data-theme", next);
      try { localStorage.setItem("theme", next); } catch (e) { /* private mode: not remembered */ }
    });
  }
})();
