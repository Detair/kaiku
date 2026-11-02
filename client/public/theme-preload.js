// Preload the first-view hero illustration before the JS bundle parses.
// The image is a static, stably-named theme asset, and the active theme is
// known synchronously from localStorage (vc:preferences), so we can kick off
// the fetch immediately — this is the LCP element for logged-out users on a
// cold cache. Kept tiny and dependency-free; failures are swallowed so a
// malformed value can never block boot. A separate file (not inline in
// index.html) so the web image's CSP can keep script-src 'self'.
(function () {
  try {
    var theme = "focused-hybrid";
    var raw = localStorage.getItem("vc:preferences");
    if (raw) {
      var parsed = JSON.parse(raw);
      if (parsed && parsed.data && typeof parsed.data.theme === "string") {
        theme = parsed.data.theme;
      }
    } else {
      var legacy = localStorage.getItem("theme");
      if (legacy) theme = legacy;
    }
    if (theme === "pixel-cozy") theme = "focused-hybrid";
    var link = document.createElement("link");
    link.rel = "preload";
    link.as = "image";
    link.href = "/themes/" + theme + "/images/floki_auth_welcome.webp";
    link.setAttribute("fetchpriority", "high");
    document.head.appendChild(link);
  } catch (e) {
    /* preload is best-effort; never block boot */
  }
})();
    
