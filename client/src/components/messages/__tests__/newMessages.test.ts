import { describe, it, expect } from "vitest";
import { countAppended } from "../newMessages";

const ids = (...xs: string[]) => xs.map((id) => ({ id }));

describe("countAppended", () => {
  it("counts messages after the previous newest one", () => {
    expect(countAppended(ids("a", "b", "c", "d"), "b", 2)).toBe(2);
  });

  it("does not count a page of older messages loaded in front", () => {
    expect(countAppended(ids("x", "y", "z", "a", "b"), "b", 2)).toBe(0);
  });

  it("counts only the new tail when older and newer arrive together", () => {
    expect(countAppended(ids("x", "a", "b", "c"), "b", 2)).toBe(1);
  });

  it("falls back to the count growth without an anchor", () => {
    expect(countAppended(ids("a", "b", "c"), undefined, 1)).toBe(2);
    // pending message replaced by its confirmed copy: no growth, nothing new
    expect(countAppended(ids("a", "b2"), "pending:b", 2)).toBe(0);
  });
});
