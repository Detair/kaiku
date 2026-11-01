import { describe, it, expect } from "vitest";
import {
  axisWidthFor,
  formatAxisValue,
  formatBinaryBytes,
  formatCompactNumber,
  hasPlottableValues,
} from "../chartFormat";

describe("formatBinaryBytes", () => {
  it("uses binary units instead of raw byte counts", () => {
    expect(formatBinaryBytes(0)).toBe("0 B");
    expect(formatBinaryBytes(512)).toBe("512 B");
    expect(formatBinaryBytes(1536)).toBe("1.5 KiB");
    expect(formatBinaryBytes(200 * 1024 * 1024)).toBe("200 MiB");
    expect(formatBinaryBytes(1024 ** 3)).toBe("1 GiB");
    expect(formatBinaryBytes(2.5 * 1024 ** 3)).toBe("2.5 GiB");
  });

  it("returns an empty label for non-finite input", () => {
    expect(formatBinaryBytes(Number.NaN)).toBe("");
  });
});

describe("formatCompactNumber", () => {
  it("shortens large counts", () => {
    expect(formatCompactNumber(36)).toBe("36");
    expect(formatCompactNumber(1500)).toBe("1.5k");
    expect(formatCompactNumber(250_000)).toBe("250k");
    expect(formatCompactNumber(3_400_000)).toBe("3.4M");
    expect(formatCompactNumber(0.25)).toBe("0.3");
  });
});

describe("formatAxisValue", () => {
  it("formats by chart value format", () => {
    expect(formatAxisValue(300 * 1024 * 1024, "bytes")).toBe("300 MiB");
    expect(formatAxisValue(12.5, "percent")).toBe("12.5%");
    expect(formatAxisValue(1200, "number")).toBe("1.2k");
  });
});

describe("axisWidthFor", () => {
  it("fits the longest memory tick label instead of a fixed 50px", () => {
    const labels = ["0 B", "256 MiB", "512 MiB", "768 MiB"];
    // "512 MiB" = 7 chars → 7 * 6.5 + 16 = 61.5 → 62
    expect(axisWidthFor(labels)).toBe(62);
    expect(axisWidthFor(labels)).toBeGreaterThan(50);
  });

  it("never goes below the minimum width", () => {
    expect(axisWidthFor(["1", "2"])).toBe(40);
    expect(axisWidthFor(null)).toBe(40);
  });
});

describe("hasPlottableValues", () => {
  it("is false for an empty or all-null series", () => {
    expect(hasPlottableValues([])).toBe(false);
    expect(hasPlottableValues([null, null])).toBe(false);
  });

  it("is true when any value is present, including zero", () => {
    expect(hasPlottableValues([null, 0])).toBe(true);
  });
});
