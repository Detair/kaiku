import { describe, expect, it } from "vitest";
import { generateTemporaryPassword } from "../passwords";

describe("generateTemporaryPassword", () => {
  it("has the requested length and only allowed characters", () => {
    expect(generateTemporaryPassword()).toHaveLength(16);
    const pw = generateTemporaryPassword(40);
    expect(pw).toHaveLength(40);
    expect(pw).toMatch(/^[A-Za-z2-9]+$/);
    expect(pw).not.toMatch(/[0OolI1]/);
  });

  it("100 generated passwords are unique", () => {
    const set = new Set(Array.from({ length: 100 }, () => generateTemporaryPassword()));
    expect(set.size).toBe(100);
  });
});
