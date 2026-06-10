/**
 * Tests for the shared connection-metrics helpers used by both voice
 * adapters: quality tier thresholds and the native (Tauri) stats payload
 * mapping (TD-16).
 */
import { describe, it, expect } from "vitest";
import { computeQualityLevel, rawStatsToMetrics } from "../types";

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

describe("rawStatsToMetrics", () => {
  it("maps the snake_case native payload to ConnectionMetrics", () => {
    const metrics = rawStatsToMetrics(
      { rtt_ms: 42.6, loss_percent: 1.234, jitter_ms: 7.4 },
      1234567890,
    );
    expect(metrics).toEqual({
      latency: 43, // rounded
      packetLoss: 1.23, // 2 decimal places
      jitter: 7, // rounded
      quality: "warning", // loss 1.23 > 1
      timestamp: 1234567890,
    });
  });

  it("produces a good quality tier for a clean connection", () => {
    const metrics = rawStatsToMetrics(
      { rtt_ms: 25, loss_percent: 0, jitter_ms: 2 },
      0,
    );
    expect(metrics.quality).toBe("good");
  });

  it("flags poor quality on high jitter alone", () => {
    const metrics = rawStatsToMetrics(
      { rtt_ms: 20, loss_percent: 0, jitter_ms: 80 },
      0,
    );
    expect(metrics.quality).toBe("poor");
    expect(metrics.jitter).toBe(80);
  });

  it("handles zeroed stats before the first RTCP receiver report", () => {
    const metrics = rawStatsToMetrics(
      { rtt_ms: 0, loss_percent: 0, jitter_ms: 0 },
      0,
    );
    expect(metrics).toMatchObject({
      latency: 0,
      packetLoss: 0,
      jitter: 0,
      quality: "good",
    });
  });
});
