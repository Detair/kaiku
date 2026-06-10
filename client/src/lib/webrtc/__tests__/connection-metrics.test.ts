/**
 * Tests for the shared connection-metrics helpers used by both voice
 * adapters: quality tier thresholds, interval-loss calculation, and the
 * metrics assembly shared by the browser and native (Tauri) paths (TD-16).
 */
import { describe, it, expect } from "vitest";
import {
  computeQualityLevel,
  deltaLossPercent,
  buildConnectionMetrics,
} from "../types";

describe("computeQualityLevel", () => {
  it("returns good for healthy connections", () => {
    expect(computeQualityLevel(40, 0.2, 5)).toBe("good");
    expect(computeQualityLevel(0, 0, 0)).toBe("good");
  });

  it("returns warning when any metric crosses the warning threshold", () => {
    expect(computeQualityLevel(101, 0, 0)).toBe("warning"); // latency
    expect(computeQualityLevel(0, 1.5, 0)).toBe("warning"); // loss
    expect(computeQualityLevel(0, 0, 31)).toBe("warning"); // jitter
  });

  it("returns poor when any metric crosses the poor threshold", () => {
    expect(computeQualityLevel(201, 0, 0)).toBe("poor"); // latency
    expect(computeQualityLevel(0, 3.5, 0)).toBe("poor"); // loss
    expect(computeQualityLevel(0, 0, 51)).toBe("poor"); // jitter
  });

  it("treats threshold boundaries as the better tier (exclusive >)", () => {
    expect(computeQualityLevel(100, 1, 30)).toBe("good");
    expect(computeQualityLevel(200, 3, 50)).toBe("warning");
  });
});

describe("deltaLossPercent", () => {
  it("returns 0 on the first sample (no previous counters)", () => {
    expect(deltaLossPercent(null, 10, 1000)).toBe(0);
  });

  it("computes loss from the interval delta, not cumulative totals", () => {
    // 100 lost / 1000 received historically, but this interval: 1 lost, 99 received
    const prev = { lost: 100, received: 1000 };
    expect(deltaLossPercent(prev, 101, 1099)).toBeCloseTo(1.0);
  });

  it("returns 0 for an idle interval (no packets either way)", () => {
    const prev = { lost: 5, received: 500 };
    expect(deltaLossPercent(prev, 5, 500)).toBe(0);
  });

  it("returns 100 when every packet in the interval was lost", () => {
    const prev = { lost: 0, received: 100 };
    expect(deltaLossPercent(prev, 50, 100)).toBe(100);
  });
});

describe("buildConnectionMetrics", () => {
  it("rounds displayed values but computes quality from raw inputs", () => {
    // 100.4ms raw latency is over the warning threshold (>100) even though
    // it rounds to 100 — quality must be computed pre-rounding so browser
    // and Tauri adapters agree at boundaries.
    const metrics = buildConnectionMetrics(100.4, 0, 0, 42);
    expect(metrics.latency).toBe(100);
    expect(metrics.quality).toBe("warning");
  });

  it("maps measurements to the ConnectionMetrics shape", () => {
    const metrics = buildConnectionMetrics(42.6, 1.234, 7.4, 1234567890);
    expect(metrics).toEqual({
      latency: 43, // rounded
      packetLoss: 1.23, // 2 decimal places
      jitter: 7, // rounded
      quality: "warning", // loss 1.234 > 1
      timestamp: 1234567890,
    });
  });

  it("produces a good quality tier for a clean connection", () => {
    expect(buildConnectionMetrics(25, 0, 2, 0).quality).toBe("good");
  });

  it("flags poor quality on high jitter alone", () => {
    const metrics = buildConnectionMetrics(20, 0, 80, 0);
    expect(metrics.quality).toBe("poor");
    expect(metrics.jitter).toBe(80);
  });
});
