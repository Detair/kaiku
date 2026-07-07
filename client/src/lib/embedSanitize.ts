/**
 * Sanitized markdown rendering for bot embed text.
 *
 * Embed description/field values are attacker-influenced (bots are
 * semi-trusted), so they go through markdown + an ISOLATED DOMPurify instance
 * (per the #631 global-hook gotcha — never share a purifier across renderers).
 * Kept in its own module so the sanitization is unit-testable without rendering
 * a Solid component (the vitest config only collects `*.test.ts`).
 */

import { marked } from "marked";
import { createIsolatedPurifier } from "@/lib/sanitizer";

const purifier = createIsolatedPurifier();

// Allowlist: basic formatting only. No id/class/style, no event handlers.
const EMBED_PURIFY_CONFIG = {
  ALLOWED_TAGS: [
    "b", "i", "em", "strong", "a", "code", "pre", "br", "p", "ul", "ol", "li",
    "blockquote", "span", "del", "h1", "h2", "h3",
  ],
  ALLOWED_ATTR: ["href", "target", "rel"],
};

/** Render embed markdown text to sanitized HTML. */
export function renderEmbedRich(text: string): string {
  return purifier.sanitize(
    marked.parse(text, { async: false }) as string,
    EMBED_PURIFY_CONFIG,
  ) as string;
}

/** Convert a 24-bit int color to a CSS hex string (or the accent fallback). */
export function embedColorHex(c?: number): string {
  return c === undefined
    ? "var(--color-accent-primary)"
    : `#${(c & 0xffffff).toString(16).padStart(6, "0")}`;
}
