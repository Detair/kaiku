/**
 * Pixel themes draw buttons at 18px (A11Y-11), but buttons that ask to be
 * small with text-xs or text-sm keep 16px (UX4a K9). Checked on the CSS text;
 * the selector specificity is spelled out so a rewrite that loses to the
 * 18px rule fails here.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const css = readFileSync(join(__dirname, "..", "themes-pixel.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

function fontSizeOf(selector: string): string | undefined {
  for (const m of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    // Split on top-level commas only; :is(a, b) stays one selector.
    const selectors: string[] = [];
    let depth = 0;
    let cur = "";
    for (const ch of m[1]) {
      if (ch === "(") depth++;
      if (ch === ")") depth--;
      if (ch === "," && depth === 0) {
        selectors.push(cur.trim());
        cur = "";
      } else cur += ch;
    }
    selectors.push(cur.trim());
    if (selectors.includes(selector)) return /font-size:\s*([^;]+);/.exec(m[2])?.[1].trim();
  }
  return undefined;
}

describe("pixel theme button sizes", () => {
  it("draws ordinary buttons at 18px", () => {
    expect(fontSizeOf('[data-theme^="pixel-"] button')).toBe("1.125rem");
  });

  it("keeps explicitly small buttons at 16px with a stronger selector", () => {
    const small = '[data-theme^="pixel-"] :is(button, [class*="btn"]):is(.text-xs, .text-sm)';
    expect(fontSizeOf(small)).toBe("1rem");
    // Specificity (0,3,0) for the small rule against (0,1,1) for the 18px rule.
  });
});
