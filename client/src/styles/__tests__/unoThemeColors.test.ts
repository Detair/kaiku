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

  describe("red as text (A11Y-7)", () => {
    it.each(["text-accent-danger", "text-danger", "text-status-error", "hover:text-accent-danger"])(
      "%s uses the text token",
      async (className) => {
        const css = await cssFor(className);
        expect(css).toContain("--color-danger-text-rgb");
        expect(css).not.toContain("--color-accent-danger-rgb");
      },
    );

    it("keeps the opacity modifier", async () => {
      expect(await cssFor("text-accent-danger/80")).toContain("--color-danger-text-rgb) / 0.8");
    });

    it.each(["bg-accent-danger", "border-accent-danger", "bg-accent-danger/20", "ring-accent-danger"])(
      "%s keeps the plain danger colour",
      async (className) => {
        expect(await cssFor(className)).toContain("--color-accent-danger-rgb");
      },
    );

    it("does not touch the danger fill classes", async () => {
      expect(await cssFor("text-on-danger")).toContain("--color-text-on-danger");
      expect(await cssFor("bg-accent-danger-fill")).toContain("--color-danger-fill-rgb");
    });
  });

  describe("text field outline (A11Y-6)", () => {
    it("input-field draws its outline with the input border token", async () => {
      const uno = await unoPromise;
      const { css } = await uno.generate("input-field", { preflights: false });
      expect(css).toContain("--color-border-input");
      expect(css).not.toContain("255 255 255 / 0.05");
    });

    it("exposes border-border-input and text-on-warning", async () => {
      expect(await cssFor("border-border-input")).toContain("--color-border-input");
      expect(await cssFor("text-on-warning")).toContain("--color-text-on-warning");
    });
  });
});
