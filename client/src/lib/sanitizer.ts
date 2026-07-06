import DOMPurify from "dompurify";

/**
 * Create an **isolated** DOMPurify instance.
 *
 * `DOMPurify.addHook` registers hooks on the shared default instance, so a hook
 * added by one consumer runs on *every* `sanitize()` call across the app. That
 * cross-contamination is fragile: e.g. the message renderer's class allowlist
 * would silently strip classes in the wiki-page renderer, and each context ends
 * up depending on hooks another module happens to register. Giving every
 * consumer its own instance makes each sanitization self-contained.
 *
 * The shared external-link hardening hook — `target="_blank"` +
 * `rel="noopener noreferrer"` on `http(s)` links, which prevents reverse
 * tabnabbing — is pre-registered here so every context gets it consistently.
 * Callers add any context-specific hooks (e.g. a class-value allowlist) to the
 * returned instance.
 */
export function createIsolatedPurifier() {
  const purifier = DOMPurify(window);

  purifier.addHook("afterSanitizeAttributes", (node) => {
    const el = node as Element;
    if (el.tagName === "A") {
      const href = el.getAttribute("href") || "";
      if (href.startsWith("http://") || href.startsWith("https://")) {
        el.setAttribute("target", "_blank");
        el.setAttribute("rel", "noopener noreferrer");
      }
    }
  });

  return purifier;
}
