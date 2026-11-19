/**
 * A11Y-15: pages (rules, privacy, library) and the admin area use the theme
 * tokens, not fixed zinc greys, so every theme keeps its colors and contrast.
 */
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const SRC = join(__dirname, "..", "..", "..");
const DIRS = ["components/pages", "components/admin", "components/guilds", "pages/settings"];
const FILES = ["views/PageViewRoute.tsx", "views/LibraryViewRoute.tsx", "views/AdminDashboard.tsx"];

function sources(): string[] {
  const out = FILES.map((f) => join(SRC, f));
  for (const dir of DIRS) {
    for (const name of readdirSync(join(SRC, dir))) {
      if (name.endsWith(".tsx")) out.push(join(SRC, dir, name));
    }
  }
  return out;
}

describe("theme tokens in pages and admin (A11Y-15)", () => {
  it("uses no fixed zinc colors", () => {
    const offenders = sources().filter((f) => /\bzinc-\d/.test(readFileSync(f, "utf8")));
    expect(offenders).toEqual([]);
  });
});
