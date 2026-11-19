import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const css = readFileSync(join(__dirname, "..", "global.css"), "utf8");

/** Body of every `@media (prefers-reduced-motion: reduce)` block. */
function reducedMotionBlocks(): string[] {
  const out: string[] = [];
  const re = /@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{/g;
  for (let m = re.exec(css); m; m = re.exec(css)) {
    let depth = 1;
    let i = re.lastIndex;
    while (depth > 0 && i < css.length) {
      if (css[i] === "{") depth++;
      if (css[i] === "}") depth--;
      i++;
    }
    out.push(css.slice(re.lastIndex, i - 1));
  }
  return out;
}

describe("prefers-reduced-motion (A11Y-9)", () => {
  const body = reducedMotionBlocks().join("\n");

  it.each([".animate-pulse", ".animate-bounce", ".animate-ping"])(
    "switches off %s",
    (selector) => {
      const rule = body.match(new RegExp(`${selector.replace(".", "\\.")}[^{]*\\{([^}]*)\\}`));
      expect(rule, selector).not.toBeNull();
      expect(rule![1]).toMatch(/animation:\s*none\s*!important/);
    },
  );

  it("leaves loading spinners turning", () => {
    expect(body).not.toContain("animate-spin");
  });
});
