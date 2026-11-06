import { createGenerator } from "unocss";
import { describe, expect, it } from "vitest";
import config from "../../../uno.config";

// Without the safelist: a safelisted class would be listed as "matched" even
// when it produces no rule, and that hid this bug for a long time.
const unoPromise = createGenerator({ ...config, safelist: [] });

async function cssFor(className: string): Promise<string> {
  const uno = await unoPromise;
  const { css } = await uno.generate(className, { preflights: false });
  return css;
}

/** Every leaf of `theme.colors` as a class-name suffix, like `surface-layer2`. */
function themeColorNames(): string[] {
  const names: string[] = [];
  const walk = (node: object, prefix: string) => {
    for (const [key, value] of Object.entries(node)) {
      if (key === "DEFAULT") {
        names.push(prefix);
        continue;
      }
      // A numeric key is written straight after its parent: layer.2 is `layer2`.
      const name = !prefix ? key : /^\d+$/.test(key) ? `${prefix}${key}` : `${prefix}-${key}`;
      if (typeof value === "string") names.push(name);
      else walk(value as object, name);
    }
  };
  walk((config.theme as { colors?: object })?.colors ?? {}, "");
  return names;
}

describe("UnoCSS theme colors", () => {
  it.each([
    ["bg-surface-layer1", "--color-surface-layer1-rgb"],
    ["bg-surface-layer2", "--color-surface-layer2-rgb"],
    ["hover:bg-surface-layer1", "--color-surface-layer1-rgb"],
    ["hover:bg-surface-layer2", "--color-surface-layer2-rgb"],
    ["border-surface-layer1", "--color-surface-layer1-rgb"],
    ["border-surface-layer2", "--color-surface-layer2-rgb"],
    ["text-surface-layer2", "--color-surface-layer2-rgb"],
    ["ring-surface-layer2", "--color-surface-layer2-rgb"],
    ["from-surface-layer2", "--color-surface-layer2-rgb"],
  ])("generates %s from %s", async (className, variable) => {
    expect(await cssFor(className)).toContain(variable);
  });

  it.each([
    ["bg-surface-layer1/80", "--color-surface-layer1-rgb) / 0.8"],
    ["bg-surface-layer2/50", "--color-surface-layer2-rgb) / 0.5"],
    ["hover:bg-surface-layer2/60", "--color-surface-layer2-rgb) / 0.6"],
  ])("applies the opacity in %s", async (className, expected) => {
    expect(await cssFor(className)).toContain(expected);
  });

  it("generates a background utility for every theme color", async () => {
    const names = themeColorNames();
    expect(names).toContain("surface-layer1");
    expect(names).toContain("surface-layer2");

    const missing: string[] = [];
    for (const name of names) {
      if (!(await cssFor(`bg-${name}`))) missing.push(`bg-${name}`);
    }
    expect(missing).toEqual([]);
  });
});
