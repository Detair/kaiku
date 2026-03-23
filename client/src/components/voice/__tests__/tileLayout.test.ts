import { describe, it, expect } from "vitest";
import { calculateGrid } from "../tileLayout";

describe("calculateGrid", () => {
  it("returns 0 cols for 0 tiles", () => {
    const result = calculateGrid(0, 800, 600);
    expect(result.cols).toBe(0);
    expect(result.rows).toBe(0);
  });

  it("returns 1x1 for 1 tile", () => {
    const result = calculateGrid(1, 800, 600);
    expect(result.cols).toBe(1);
    expect(result.rows).toBe(1);
  });

  it("returns 2x1 for 2 tiles in landscape", () => {
    const result = calculateGrid(2, 800, 400);
    expect(result.cols).toBe(2);
    expect(result.rows).toBe(1);
  });

  it("returns 2x2 for 4 tiles in square-ish container", () => {
    const result = calculateGrid(4, 800, 600);
    expect(result.cols).toBe(2);
    expect(result.rows).toBe(2);
  });

  it("caps at 5 columns", () => {
    const result = calculateGrid(20, 1920, 1080);
    expect(result.cols).toBeLessThanOrEqual(5);
  });

  it("returns tile dimensions with 4:3 aspect ratio", () => {
    const result = calculateGrid(4, 800, 600);
    const ratio = result.tileWidth / result.tileHeight;
    expect(ratio).toBeCloseTo(4 / 3, 1);
  });

  it("respects minimum tile width of 120px", () => {
    // In a 800x600 container with 20 tiles, the algorithm should not
    // pick a column count that would shrink tiles below 120px.
    const result = calculateGrid(20, 800, 600);
    expect(result.tileWidth).toBeGreaterThanOrEqual(120);
  });
});
