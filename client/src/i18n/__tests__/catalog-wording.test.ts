import { describe, it, expect } from "vitest";
import { en } from "../en";

describe("English catalog wording", () => {
  it("no English catalog value says guild", () => {
    const offenders = Object.entries(en)
      .filter(([, value]) => /\bguilds?\b/i.test(value))
      .map(([key, value]) => `${key}: ${value}`);
    expect(offenders).toEqual([]);
  });
});
