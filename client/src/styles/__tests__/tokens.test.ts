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
      // Check icons rendered on the accent-filled radio/checkbox indicator.
      if (/bg-accent-primary["'`]?[^\n]*\n[\s\S]{0,200}?<Check[^>]*text-white/.test(f.text)) {
        hits.push(`${f.path}: Check icon text-white on accent fill`);
      }
    }
    expect(hits).toEqual([]);
  });
});
