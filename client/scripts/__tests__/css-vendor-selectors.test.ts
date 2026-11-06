// @vitest-environment node
/**
 * A browser drops a whole rule when any selector in its list has a pseudo-element
 * it does not know. So `.w-4,.x::-moz-range-thumb{width:1rem}` is thrown away by
 * Chromium and WebKit, and `w-4` silently stops working everywhere in the app.
 *
 * UnoCSS builds such lists when `mergeSelectors` is on, because it joins rules
 * with identical bodies. These tests generate the app's real CSS and fail if a
 * vendor-only pseudo-element shares a selector list with anything else.
 */
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createGenerator } from "unocss";
import { describe, expect, it } from "vitest";
import unoConfig from "../../uno.config";

const CLIENT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const VENDOR_PSEUDO = /::?-(moz|webkit|ms|o)-[a-z-]+/;

/** Split on top-level commas, skipping escapes (`\,`) and `(...)`/`[...]` groups. */
function splitSelectorList(prelude: string): string[] {
  const parts: string[] = [];
  let current = "";
  let depth = 0;
  for (let i = 0; i < prelude.length; i++) {
    const char = prelude[i];
    if (char === "\\") {
      current += char + (prelude[++i] ?? "");
    } else if (char === "," && depth === 0) {
      parts.push(current);
      current = "";
    } else {
      if (char === "(" || char === "[") depth++;
      if (char === ")" || char === "]") depth--;
      current += char;
    }
  }
  parts.push(current);
  return parts.map((part) => part.trim());
}

/** The vendor pseudo-element of a selector, ignoring the escaped text of class names. */
function vendorOf(selector: string): string | null {
  return selector.replace(/\\./g, "_").match(VENDOR_PSEUDO)?.[1] ?? null;
}

/**
 * Selector lists a browser could throw away because of one vendor pseudo-element.
 * Lists from a single vendor are fine: every member is understood by the same
 * engines, and `::-webkit-*` is accepted by all of them.
 */
function findUnsafeSelectorLists(css: string): string[] {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const unsafe: string[] = [];
  for (const [, prelude] of withoutComments.matchAll(/(?<=^|[{};])\s*([^{};]+)\{/g)) {
    if (prelude.startsWith("@")) continue;
    const selectors = splitSelectorList(prelude);
    const vendors = new Set(selectors.map(vendorOf));
    if (selectors.length > 1 && vendors.size > 1) unsafe.push(prelude.trim());
  }
  return unsafe;
}

function sourceFiles(dir: string, extensions: RegExp): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path, extensions);
    return extensions.test(entry.name) ? [path] : [];
  });
}

async function generateAppCss(): Promise<string> {
  const uno = await createGenerator(unoConfig);
  const source = [
    join(CLIENT_ROOT, "index.html"),
    ...sourceFiles(join(CLIENT_ROOT, "src"), /\.(tsx?|html)$/),
  ]
    .map((file) => readFileSync(file, "utf8"))
    .join("\n");
  const { css } = await uno.generate(source);
  return css;
}

describe("findUnsafeSelectorLists", () => {
  it("flags a plain class that shares a list with a vendor pseudo-element", () => {
    const css = String.raw`.\[\&\:\:-moz-range-thumb\]\:w-4::-moz-range-thumb,.w-4{width:1rem}`;
    expect(findUnsafeSelectorLists(css)).toHaveLength(1);
  });

  it("flags two different vendors in one list", () => {
    expect(
      findUnsafeSelectorLists(".a::-moz-range-thumb,.a::-webkit-slider-thumb{width:1rem}"),
    ).toHaveLength(1);
  });

  it("flags a list inside an at-rule", () => {
    expect(
      findUnsafeSelectorLists("@media (min-width:1px){.w-4,.a::-moz-range-thumb{width:1rem}}"),
    ).toHaveLength(1);
  });

  it("accepts vendor rules on their own and plain lists", () => {
    const css = String.raw`
      .\[\&\:\:-moz-range-thumb\]\:w-4::-moz-range-thumb{width:1rem}
      .a::-webkit-inner-spin-button,.a::-webkit-outer-spin-button{margin:0}
      .b,.c:is(.d,.e){color:red}
    `;
    expect(findUnsafeSelectorLists(css)).toEqual([]);
  });
});

describe("generated app CSS", () => {
  it("keeps vendor-only pseudo-elements out of shared selector lists", async () => {
    const css = await generateAppCss();
    // Guards against the source scan finding nothing and the test passing on empty input.
    expect(css).toContain("::-moz-range-thumb");
    expect(findUnsafeSelectorLists(css)).toEqual([]);
  }, 60_000);

  it("keeps vendor-only pseudo-elements out of shared selector lists in hand-written CSS", () => {
    for (const file of sourceFiles(join(CLIENT_ROOT, "src"), /\.css$/)) {
      expect(findUnsafeSelectorLists(readFileSync(file, "utf8")), file).toEqual([]);
    }
  });
});
