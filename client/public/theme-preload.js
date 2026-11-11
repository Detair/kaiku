// Preload the first-view hero illustration before the JS bundle parses.
// The image is a static, stably-named theme asset, and the active theme is
// known synchronously from localStorage (vc:preferences), so we can kick off
// the fetch immediately — this is the LCP element for logged-out users on a
// cold cache. Kept tiny and dependency-free; failures are swallowed so a
// malformed value can never block boot. A separate file (not inline in
// index.html) so the web image's CSP can keep script-src 'self'.
// Theme and text size are set here, synchronously, before the first paint, so
// a light theme, "Same as system" or a larger text size never flashes the
// default dark look first. The rules mirror resolveTheme() in
// src/stores/theme.ts; src/stores/__tests__/themePreload.test.ts runs this file
// against it for every theme, so the two cannot drift apart.
(function () {
  try {
    var LIGHT = { "solarized-light": true };
    var theme = "focused-hybrid";
    var follow = false;
    var fontSize = "default";
    var raw = localStorage.getItem("vc:preferences");
    if (raw) {
      var parsed = JSON.parse(raw);
      if (parsed && parsed.data) {
        if (typeof parsed.data.theme === "string") theme = parsed.data.theme;
        follow = parsed.data.theme_follow_system === true;
        if (parsed.data.font_size === "large" || parsed.data.font_size === "xlarge") {
          fontSize = parsed.data.font_size;
        }
      }
    } else {
      var legacy = localStorage.getItem("theme");
      if (legacy) theme = legacy;
    }
    if (follow) {
      var dark = true;
      try {
        dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      } catch (e) {
        /* no matchMedia: assume dark, like the app does */
      }
      if (!dark) theme = "solarized-light";
      else if (LIGHT[theme]) theme = "focused-hybrid";
    }
    var root = document.documentElement;
    root.setAttribute("data-theme", theme);
    if (fontSize !== "default") root.setAttribute("data-font-size", fontSize);
    if (theme === "pixel-cozy") theme = "focused-hybrid";
    // Preload the first-view hero illustration before the JS bundle parses.
    // The image is a static, stably-named theme asset, so we can kick off the
    // fetch immediately: it is the LCP element for logged-out users on a cold
    // cache.
    var link = document.createElement("link");
    link.rel = "preload";
    link.as = "image";
    link.href = "/themes/" + theme + "/images/floki_auth_welcome.webp";
    link.setAttribute("fetchpriority", "high");
    document.head.appendChild(link);
  } catch (e) {
    /* best-effort; never block boot */
  }
})();
