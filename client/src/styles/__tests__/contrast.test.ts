/**
 * WCAG contrast checks for every theme × the state pairs people complained
 * about: disabled buttons, toggle on/off, placeholders and focus rings.
 *
 * Colours are read straight from themes.css, so a token change that breaks a
 * pair fails here. Translucent values (rgba) are composited over the surface
 * they are drawn on before the ratio is taken.
 *
 * Thresholds (WCAG 2.2):
 * - 4.5:1 for button labels (1.4.3, normal text).
 * - 3:1 for UI component boundaries, knobs, focus rings (1.4.11) and for the
 *   label of a disabled control (1.4.3 exempts disabled controls, but people
 *   still need to read what the button would do).
 * - Placeholder: 3:1 against the field (see the note on --color-text-placeholder
 *   in themes.css for why not 4.5) and at least 2:1 against typed text, so an
 *   empty field never looks filled in.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

type Rgb = [number, number, number];
type Rgba = [number, number, number, number];

const css = readFileSync(join(__dirname, "..", "themes.css"), "utf8");

function parseBlocks(source: string): Map<string, Map<string, string>> {
  const out = new Map<string, Map<string, string>>();
  const noComments = source.replace(/\/\*[\s\S]*?\*\//g, "");
  for (const m of noComments.matchAll(/:root(?:\[data-theme="([^"]+)"\])?\s*\{([^}]*)\}/g)) {
    const name = m[1] ?? "default";
    const vars = new Map<string, string>();
    for (const d of m[2].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
      vars.set(d[1], d[2].trim());
    }
    out.set(name, vars);
  }
  return out;
}

const themes = parseBlocks(css);

function parseColor(value: string): Rgba {
  const v = value.trim();
  const hex = v.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) {
    const h = hex[1].length === 3 ? [...hex[1]].map((c) => c + c).join("") : hex[1];
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16), 1];
  }
  const rgba = v.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+))?\s*\)$/);
  if (rgba) {
    return [Number(rgba[1]), Number(rgba[2]), Number(rgba[3]), rgba[4] === undefined ? 1 : Number(rgba[4])];
  }
  throw new Error(`unsupported colour value: ${value}`);
}

function resolve(theme: Map<string, string>, token: string, depth = 0): string {
  const raw = theme.get(token);
  if (raw === undefined) throw new Error(`token ${token} is not defined`);
  const ref = raw.match(/^var\((--[\w-]+)\)$/);
  if (ref && depth < 8) return resolve(theme, ref[1], depth + 1);
  return raw;
}

function over(fg: Rgba, bg: Rgb): Rgb {
  const a = fg[3];
  return [0, 1, 2].map((i) => fg[i] * a + bg[i] * (1 - a)) as Rgb;
}

function luminance([r, g, b]: Rgb): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function ratio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Opaque colour of a token, composited over the given opaque surface token. */
function colour(theme: Map<string, string>, token: string, surface?: string): Rgb {
  const c = parseColor(resolve(theme, token));
  if (c[3] >= 1) return [c[0], c[1], c[2]];
  if (!surface) throw new Error(`${token} is translucent; a surface is required`);
  return over(c, colour(theme, surface));
}

const SURFACES = [
  "--color-surface-base",
  "--color-surface-layer1",
  "--color-surface-layer2",
] as const;

interface Pair {
  fg: string;
  bg: string;
  /** Surface under the bg (for translucent backgrounds). */
  under?: string;
  min: number;
}

function pairs(): Pair[] {
  const list: Pair[] = [
    // Documented tiers (CLAUDE.md "UI Contrast Rules").
    { fg: "--color-text-primary", bg: "--color-surface-base", min: 7 },
    { fg: "--color-text-secondary", bg: "--color-surface-layer1", min: 4.5 },
    { fg: "--color-text-muted", bg: "--color-surface-layer1", min: 3 },
    // Button labels, default state.
    { fg: "--color-text-on-accent", bg: "--color-accent-primary", min: 4.5 },
    // Danger buttons use their own darker fill; accent-danger stays the
    // danger text colour on dark surfaces.
    { fg: "--color-text-on-danger", bg: "--color-danger-fill", min: 4.5 },
    // Disabled buttons: the label on its own fill.
    { fg: "--color-disabled-text", bg: "--color-disabled-bg", min: 3 },
    // Toggle: knob vs its track in both states.
    { fg: "--color-toggle-knob-on", bg: "--color-toggle-track-on", min: 3 },
    { fg: "--color-toggle-knob-off", bg: "--color-toggle-track-off", min: 3 },
  ];
  for (const s of SURFACES) {
    // A disabled ghost button has no fill, so its label sits on the surface.
    list.push({ fg: "--color-disabled-text", bg: s, min: 3 });
    list.push({ fg: "--color-disabled-border", bg: s, min: 3 });
    // Toggle outline: the track must be findable on every surface.
    list.push({ fg: "--color-toggle-track-on", bg: s, min: 3 });
    list.push({ fg: "--color-toggle-border-off", bg: s, min: 3 });
    // Placeholder vs the field background (inputs sit on all three).
    list.push({ fg: "--color-text-placeholder", bg: s, min: 3 });
    // Focus indicator.
    list.push({ fg: "--color-focus-ring", bg: s, min: 3 });
  }
  return list;
}

const THEME_NAMES = [...themes.keys()];

describe("theme contrast", () => {
  it("parses every theme from themes.css", () => {
    expect(THEME_NAMES).toEqual(
      expect.arrayContaining([
        "default",
        "focused-hybrid",
        "solarized-dark",
        "solarized-light",
        "pixel-cozy",
      ]),
    );
  });

  describe.each(THEME_NAMES)("%s", (name) => {
    const theme = themes.get(name)!;

    it.each(pairs().map((p) => [`${p.fg} on ${p.bg}`, p] as const))(
      "%s",
      (_label, p) => {
        const bg = colour(theme, p.bg, p.under ?? "--color-surface-layer2");
        const fg = parseColor(resolve(theme, p.fg));
        const r = ratio(fg[3] >= 1 ? [fg[0], fg[1], fg[2]] : over(fg, bg), bg);
        expect(Number(r.toFixed(2)), `${name}: ${p.fg} on ${p.bg}`).toBeGreaterThanOrEqual(p.min);
      },
    );

    it("placeholder reads clearly dimmer than typed text", () => {
      for (const s of SURFACES) {
        const bg = colour(theme, s);
        const ph = colour(theme, "--color-text-placeholder", s);
        const input = colour(theme, "--color-text-input", s);
        expect(ratio(ph, input), `${name}: placeholder vs input text`).toBeGreaterThanOrEqual(2);
        expect(ratio(ph, bg), `${name}: placeholder must be dimmer than input text on ${s}`).toBeLessThan(
          ratio(input, bg),
        );
      }
    });

    it("toggle on and off tracks differ", () => {
      for (const s of SURFACES) {
        const on = colour(theme, "--color-toggle-track-on", s);
        const off = colour(theme, "--color-toggle-track-off", s);
        expect(ratio(on, off), `${name}: on vs off track on ${s}`).toBeGreaterThanOrEqual(2);
      }
    });

    it("disabled fill differs from the enabled accent fill", () => {
      const disabled = colour(theme, "--color-disabled-bg", "--color-surface-layer2");
      const accent = colour(theme, "--color-accent-primary");
      expect(ratio(disabled, accent), `${name}: disabled vs accent`).toBeGreaterThanOrEqual(1.5);
    });
  });
});
