import { describe, it, expect } from "vitest";
import { render } from "@solidjs/testing-library";
import CodeBlock from "../CodeBlock";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const stylesDir = join(__dirname, "../../../styles");
const highlightCss = readFileSync(join(stylesDir, "highlight-theme.css"), "utf8");
const pixelHighlightCss = readFileSync(join(stylesDir, "themes-pixel-highlight.css"), "utf8");

/** Declarations of every rule whose selector list targets `.hljs` itself. */
function hljsRuleBodies(css: string): string[] {
  const bodies: string[] = [];
  for (const match of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const selectors = match[1].split(",").map((s) => s.trim());
    if (selectors.some((s) => /\.hljs$/.test(s))) bodies.push(match[2]);
  }
  return bodies;
}

describe("CodeBlock", () => {
  it("puts the box styles on the <pre>, not on the highlighted inline <code>", () => {
    const { container } = render(() => (
      <CodeBlock language="rust">{"let a = 1;\nlet b = 2;"}</CodeBlock>
    ));
    const pre = container.querySelector("pre")!;
    const code = pre.querySelector("code")!;
    expect(pre.classList.contains("code-block")).toBe(true);
    expect(code.classList.contains("hljs")).toBe(true);
  });

  // highlight.js adds .hljs to the inline <code>. Padding, background or a
  // border there repeat on every line fragment and paint over the line above,
  // which clipped every line of a code block but the last.
  it.each([
    ["highlight-theme.css", highlightCss],
    ["themes-pixel-highlight.css", pixelHighlightCss],
  ])("%s gives .hljs no box styles", (_name, css) => {
    expect(css.length).toBeGreaterThan(0);
    for (const body of hljsRuleBodies(css)) {
      // shorthands and longhands (padding-top, background-color, border-*)
      expect(body).not.toMatch(/(^|[;\s])(padding|background|border|margin)(-[a-z-]+)?\s*:/);
    }
  });

  it("gives the pixel theme's code block shape to the <pre>", () => {
    expect(pixelHighlightCss).toMatch(
      /\[data-theme\^="pixel-"\] \.code-block\s*\{[^}]*border-radius:[^}]*border:/,
    );
  });
});
