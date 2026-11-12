import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// jsdom has no layout, so the hit area is checked on the rule itself.
const css = readFileSync(join(__dirname, "..", "global.css"), "utf8");

function block(selectorRe: RegExp, from = 0): string {
  const m = selectorRe.exec(css.slice(from));
  if (!m) return "";
  const start = from + m.index + m[0].length;
  return css.slice(start, css.indexOf("}", start));
}

describe("tap-target (A11Y-12, NAV-19)", () => {
  it("grows the hit area to at least 24 px around the button without resizing it", () => {
    const after = block(/\.tap-target::after\s*\{/);
    expect(after).toMatch(/position:\s*absolute/);
    expect(after).toMatch(/width:\s*max\(100%,\s*24px\)/);
    expect(after).toMatch(/height:\s*max\(100%,\s*24px\)/);
    expect(after).toMatch(/translate\(-50%,\s*-50%\)/);
  });

  it("uses 44 px on touch screens", () => {
    const media = css.indexOf("@media (pointer: coarse)");
    expect(media).toBeGreaterThan(-1);
    const after = block(/\.tap-target::after\s*\{/, media);
    expect(after).toMatch(/width:\s*max\(100%,\s*44px\)/);
    expect(after).toMatch(/height:\s*max\(100%,\s*44px\)/);
  });

  it("does not override position utilities on the same button", () => {
    expect(css).toMatch(/:where\(\.tap-target\)\s*\{\s*position:\s*relative;/);
  });
});
