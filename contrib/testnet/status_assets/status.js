// SEDRA (#13): optional extras for the testnet status page. The page works without this script; it only makes the
// countdowns tick, shows the chain start in the visitor's local time, warns when the data has stopped updating, and
// offers a light/dark switch. Estimated countdowns stop when the data is old, so a stalled monitor shows no false
// countdown; the start countdown is a fixed time and keeps ticking.
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

  Array.prototype.forEach.call(document.querySelectorAll(".local-time[data-unix]"), function (el) {
    var d = new Date(Number(el.getAttribute("data-unix")) * 1000);
    try {
      el.textContent = "your time " + d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
    } catch (e) {
      el.textContent = "your time " + d.toLocaleString();
    }
    el.hidden = false;
  });

  function tick() {
    var now = Date.now() / 1000;
    var old = !dataTime || now - dataTime > 600;
    countdowns.forEach(function (el) {
      var left = Number(el.getAttribute("data-eta")) - now;
      if (el.getAttribute("data-kind") === "fixed") {
        el.textContent = left > 0 ? "in " + span(left) : "any moment now";
      } else if (old) {
        el.textContent = "estimate paused";
      } else {
        el.textContent = left > 0 ? "~" + span(left) : "any moment now";
      }
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
