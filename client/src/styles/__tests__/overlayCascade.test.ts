/**
 * White overlays in the light theme (UX4a W1).
 *
 * border-white/N and bg-white/N take their colour from --color-overlay-rgb.
 * These tests run the real UnoCSS config, then resolve the cascade for one
 * element the way a browser does (specificity, then source order, with the
 * border-color shorthand split into its four sides) and check which class
 * wins. A remap that is too strong (the old `:root[data-theme] .border-white/10`
 * rules) fails here: it beats focus, hover, checked and side colours.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createGenerator } from "unocss";
import { describe, expect, it } from "vitest";
import config from "../../../uno.config";

const stylesDir = resolve(__dirname, "..");
const read = (file: string) => readFileSync(resolve(stylesDir, file), "utf8");
const globalCss = read("global.css");
const themesCss = read("themes.css");

const unoPromise = createGenerator({ ...config, safelist: [] });

type State = "hover" | "focus" | "checked";
interface Decl {
  className: string;
  specificity: number;
  order: number;
  value: string;
}

const SIDES = ["top", "right", "bottom", "left"] as const;

function longhands(prop: string, value: string): [string, string][] {
  if (prop === "border-color") return SIDES.map((s) => [`border-${s}-color`, value]);
  return [[prop, value]];
}

const unescape = (s: string) => s.replace(/\\(.)/g, "$1");

/** Parse UnoCSS output (flat `selector{decls}` lines) into ordered rules. */
function parseRules(css: string) {
  const rules: { selector: string; decls: [string, string][] }[] = [];
  const body = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const re = /([^{}]+)\{([^{}]*)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(body))) {
    const decls = m[2]
      .split(";")
      .map((d) => d.trim())
      .filter(Boolean)
      .map((d) => {
        const i = d.indexOf(":");
        return [d.slice(0, i).trim(), d.slice(i + 1).trim()] as [string, string];
      });
    rules.push({ selector: m[1].trim(), decls });
  }
  return rules;
}

/**
 * Winning value per longhand property for one element with `classes` in
 * `states`, in the light theme. Sources in load order (index.tsx): UnoCSS
 * output, then global.css. Rules of global.css may be scoped to the light
 * theme (`:root[data-theme="solarized-light"] .x`), which adds (0,2,0).
 */
async function cascade(classes: string, states: State[] = []) {
  const uno = await unoPromise;
  const { css } = await uno.generate(classes, { preflights: false });
  const own = new Set(classes.split(/\s+/).filter(Boolean));
  const winners = new Map<string, Decl>();
  const rules = [...parseRules(css), ...parseRules(globalCss)].flatMap((rule) =>
    rule.selector.split(",").map((selector) => ({ selector: selector.trim(), decls: rule.decls })),
  );
  rules.forEach((rule, order) => {
    // `.class` or `.class:state`; the class name itself may contain escaped colons.
    const scoped = rule.selector.startsWith(`:root[data-theme="${LIGHT}"] `);
    const selector = scoped ? rule.selector.slice(`:root[data-theme="${LIGHT}"] `.length) : rule.selector;
    const m = /^\.((?:\\.|[^:\\\s])+)(?::([a-z-]+))?$/.exec(selector);
    if (!m) return;
    const className = unescape(m[1]);
    const pseudo = m[2] as State | undefined;
    if (!own.has(className)) return;
    if (pseudo && !states.includes(pseudo)) return;
    const specificity = 10 + (pseudo ? 10 : 0) + (scoped ? 20 : 0);
    for (const [prop, value] of rule.decls) {
      for (const [p, v] of longhands(prop, value)) {
        const prev = winners.get(p);
        if (!prev || specificity > prev.specificity || (specificity === prev.specificity && order > prev.order)) {
          winners.set(p, { className, specificity, order, value: v });
        }
      }
    }
  });
  return winners;
}

/** Value of a custom property declared in a `:root[data-theme="…"]` block. */
function themeVar(theme: string, name: string): string | undefined {
  const start = themesCss.indexOf(`:root[data-theme="${theme}"] {`);
  if (start < 0) return undefined;
  const block = themesCss.slice(start, themesCss.indexOf("}", start));
  return new RegExp(`${name}:\\s*([^;]+);`).exec(block)?.[1].trim();
}

/** Class names in global.css that set --color-overlay-rgb back to white. */
function whiteOverlaySurfaces(): string[] {
  const m = /([^{}]+)\{\s*--color-overlay-rgb:\s*255 255 255;\s*\}/.exec(globalCss);
  if (!m) return [];
  return m[1]
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split(",")
    .map((s) => unescape(s.trim()).replace(/^\./, ""))
    .filter(Boolean);
}

/** The colour an overlay value ends up as for a theme and the ancestors' classes. */
function resolveOverlay(value: string, theme: string, ancestors: string[] = []): string {
  const surfaces = new Set(whiteOverlaySurfaces());
  const onWhiteSurface = ancestors.some((cls) => cls.split(/\s+/).some((c) => surfaces.has(c)));
  let overlay = onWhiteSurface ? "255 255 255" : themeVar(theme, "--color-overlay-rgb");
  const ref = overlay && /^var\((--[\w-]+)\)$/.exec(overlay);
  if (ref) overlay = themeVar(theme, ref[1]);
  return value.replace(/var\(--color-overlay-rgb, ([^)]+)\)/, (_, fallback) => overlay ?? fallback);
}

/** The class string of the first element in a source file that contains `needle`. */
function classFrom(file: string, needle: string): string {
  const src = readFileSync(resolve(stylesDir, "..", file), "utf8");
  const m = new RegExp(`(?:class|backdropClass)=\\{?[\`"]([^\`"]*${needle.replace(/[/.*+?^${}()|[\]\\]/g, "\\$&")}[^\`"]*)[\`"]`).exec(src);
  if (!m) throw new Error(`${needle} not found in ${file}`);
  return m[1].replace(/\$\{[^}]*\}/g, " ");
}

const LIGHT = "solarized-light";
const TEXT_PRIMARY = "43 67 74";

describe("white overlays: state and side colours still win (W1)", () => {
  it("light theme draws the overlay in the text colour", () => {
    expect(themeVar(LIGHT, "--color-overlay-rgb")).toBe("var(--color-text-primary-rgb)");
    expect(themeVar(LIGHT, "--color-text-primary-rgb")).toBe(TEXT_PRIMARY);
  });

  it("dark themes keep white (no overlay variable outside the light theme)", async () => {
    const w = await cascade("border border-white/10");
    expect(w.get("border-top-color")?.value).toBe("rgb(var(--color-overlay-rgb, 255 255 255) / 0.1)");
    const count = themesCss.match(/--color-overlay-rgb:/g)?.length ?? 0;
    expect(count).toBe(1);
  });

  it("global.css no longer targets overlay classes with theme selectors", () => {
    expect(globalCss).not.toMatch(/\.(?:hover\\:)?(?:bg|border)-white\\\/\d+/);
  });

  it("focus outline of a plain input: focus colour wins", async () => {
    // AuditLogPanel, UsersPanel, ReportsPanel: raw inputs with outline-none.
    const input = "w-full px-3 py-2 border border-white/10 rounded-lg focus:outline-none focus:border-accent-primary";
    const rest = await cascade(input);
    expect(resolveOverlay(rest.get("border-left-color")!.value, LIGHT)).toBe(`rgb(${TEXT_PRIMARY} / 0.1)`);

    const focused = await cascade(input, ["focus"]);
    for (const side of SIDES) {
      expect(focused.get(`border-${side}-color`)?.className).toBe("focus:border-accent-primary");
    }
  });

  it("spinner keeps its coloured top edge (LazyFallback)", async () => {
    const spinner = classFrom("components/ui/LazyFallback.tsx", "border-t-accent-primary");
    const w = await cascade(spinner);
    expect(w.get("border-top-color")?.className).toBe("border-t-accent-primary");
    expect(w.get("border-left-color")?.className).toBe("border-white/10");
  });

  it("button spinner keeps its white top edge (Login)", async () => {
    const w = await cascade("w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin");
    expect(w.get("border-top-color")?.className).toBe("border-t-white");
    expect(w.get("border-right-color")?.className).toBe("border-white/30");
  });

  it("banner keeps its coloured left stripe (ProbationBanner)", async () => {
    const w = await cascade("border-b border-white/10 border-l-4 border-l-status-warning");
    expect(w.get("border-left-color")?.className).toBe("border-l-status-warning");
    expect(w.get("border-bottom-color")?.className).toBe("border-white/10");
  });

  it.each([
    ["bg-white/5 rounded-lg hover:bg-status-success/20", "hover:bg-status-success/20"], // FriendsList accept
    ["bg-white/10 hover:bg-accent-primary/20", "hover:bg-accent-primary/20"], // FocusSettings chip
    ["bg-white/10 hover:bg-white/20", "hover:bg-white/20"], // admin and settings buttons
  ])("hover background wins: %s", async (classes, expected) => {
    const rest = await cascade(classes);
    const hovered = await cascade(classes, ["hover"]);
    expect(hovered.get("background-color")?.className).toBe(expected);
    expect(resolveOverlay(hovered.get("background-color")!.value, LIGHT)).not.toBe(
      resolveOverlay(rest.get("background-color")!.value, LIGHT),
    );
  });

  it("checked checkbox shows its accent border", async () => {
    const w = await cascade(
      "w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary",
      ["checked"],
    );
    expect(w.get("border-top-color")?.className).toBe("checked:border-accent-primary");
  });

  it("image viewer close button keeps a white hover on its black backdrop", async () => {
    const backdrop = classFrom("components/ui/ImageLightbox.tsx", "bg-black/90");
    const button = classFrom("components/ui/ImageLightbox.tsx", "hover:bg-white/10");
    const w = await cascade(button, ["hover"]);
    const value = w.get("background-color")!.value;
    expect(resolveOverlay(value, LIGHT, [backdrop])).toBe("rgb(255 255 255 / 0.1)");
    // Without the dark backdrop the same class would use the text colour.
    expect(resolveOverlay(value, LIGHT)).toBe(`rgb(${TEXT_PRIMARY} / 0.1)`);
  });

  it("overlay rules sit before shortcuts and utilities", async () => {
    const uno = await unoPromise;
    const { css } = await uno.generate("border-white/10 item-hover border-t-white", { preflights: false });
    const overlay = css.indexOf("/* layer: overlay */");
    expect(overlay).toBeGreaterThanOrEqual(0);
    expect(overlay).toBeLessThan(css.indexOf("/* layer: shortcuts */"));
    expect(overlay).toBeLessThan(css.indexOf("/* layer: default */"));
  });
});
