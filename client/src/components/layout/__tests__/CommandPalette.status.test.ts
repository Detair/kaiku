import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// The palette offers no status commands today; this guards against an
// "invisible" status command coming back (v1.0 has no invisible status).
describe("CommandPalette status commands", () => {
  it("offers no invisible status", () => {
    const source = readFileSync(join(__dirname, "..", "CommandPalette.tsx"), "utf8");
    expect(source).not.toMatch(/invisible/i);
    expect(source).not.toMatch(/updateStatus|setMyStatus\(\s*["']offline["']/);
  });
});
