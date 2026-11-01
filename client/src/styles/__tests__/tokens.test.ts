import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const SRC = join(__dirname, "..", "..");

function tsxFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (name === "__tests__") continue;
      out.push(...tsxFiles(p));
    } else if (p.endsWith(".tsx")) {
      out.push(p);
    }
  }
  return out;
}

const files = tsxFiles(SRC).map((p) => ({
  path: p.slice(SRC.length + 1),
  text: readFileSync(p, "utf8"),
}));

describe("colour tokens", () => {
  it("no source file uses undefined colour tokens", () => {
    const banned = [
      "accent-error",
      "accent-hover",
      "--color-surface-raised",
      "--color-success-",
    ];
    const hits: string[] = [];
    for (const f of files) {
      for (const b of banned) {
        if (f.text.includes(b)) hits.push(`${f.path}: ${b}`);
      }
    }
    expect(hits).toEqual([]);
  });

  it("no text-white on accent fills", () => {
    const hits: string[] = [];
    for (const f of files) {
      // Every quoted string literal is treated as one class string.
      for (const m of f.text.matchAll(/(["'`])((?:(?!\1)[^\\\n]|\\.)*)\1/g)) {
        const s = m[2];
        if (/\bbg-accent-primary\b/.test(s) && /\btext-white\b/.test(s)) {
          hits.push(`${f.path}: ${s}`);
        }
      }
      // Heuristic (line-window based, not a parser): a text-white child within
      // a few lines after an element whose class has bg-accent-primary as a solid fill.
      const lines = f.text.split("\n");
      lines.forEach((line, i) => {
        if (!/bg-accent-primary(?![/\w-])/.test(line) || /text-on-/.test(line)) return;
        const window = lines.slice(i + 1, i + 4).join("\n");
        if (/\btext-white\b/.test(window)) {
          hits.push(`${f.path}:${i + 1}: text-white near bg-accent-primary fill`);
        }
      });
    }
    expect(hits).toEqual([]);
  });

});

describe("state styling", () => {
  it("no control fades out with disabled:opacity (use state-disabled)", () => {
    const hits: string[] = [];
    for (const f of files) {
      for (const m of f.text.matchAll(/\b(?:peer-|group-)?disabled:opacity-\d+/g)) {
        hits.push(`${f.path}: ${m[0]}`);
      }
    }
    expect(hits).toEqual([]);
  });

  it("every switch is the shared Toggle component", () => {
    const hits: string[] = [];
    for (const f of files) {
      if (f.path.endsWith("components/ui/Toggle.tsx")) continue;
      if (/role="switch"/.test(f.text)) hits.push(`${f.path}: role="switch"`);
      // Hand-built knob: a track with a knob slid by translate-x-5/6.
      if (/"translate-x-[56]"|\? "translate-x-[56]"|peer-checked:after:translate-x/.test(f.text)) {
        hits.push(`${f.path}: hand-built switch knob`);
      }
    }
    expect(hits).toEqual([]);
  });

  it("placeholders use the placeholder token", () => {
    const hits: string[] = [];
    for (const f of files) {
      for (const m of f.text.matchAll(/placeholder(?::|-)(?!text-placeholder\b|text-text-placeholder\b)[\w/-]+/g)) {
        // Only class names, not the `placeholder=` attribute or i18n keys.
        if (/^placeholder[-:](text|zinc|gray|white|slate|neutral)/.test(m[0])) {
          hits.push(`${f.path}: ${m[0]}`);
        }
      }
    }
    expect(hits).toEqual([]);
  });
});
