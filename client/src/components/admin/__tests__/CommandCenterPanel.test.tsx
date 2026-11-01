import { describe, it, expect, vi, beforeAll } from "vitest";
import { render, screen, within } from "@solidjs/testing-library";

// uPlot needs a real canvas; record the options instead.
const uplotOptions = vi.hoisted(() => [] as unknown[]);
vi.mock("uplot", () => ({
  default: class {
    constructor(opts: unknown) {
      uplotOptions.push(opts);
    }
    setData() {}
    setSize() {}
    destroy() {}
  },
}));
vi.mock("uplot/dist/uPlot.min.css", () => ({}));

const MiB = vi.hoisted(() => 1024 * 1024);

vi.mock("@/stores/admin", () => ({
  adminState: {
    obsSummary: null,
    obsTrends: {
      metrics: [
        // Voice: no sessions yet → no datapoints at all.
        { metric_name: "kaiku_voice_sessions_active", datapoints: [] },
        {
          metric_name: "kaiku_process_memory_bytes",
          datapoints: [
            {
              ts: "2026-11-01T10:00:00Z",
              metric_name: "kaiku_process_memory_bytes",
              value_count: 300 * MiB,
              value_sum: null,
              value_p50: null,
              value_p95: null,
              value_p99: null,
            },
            {
              ts: "2026-11-01T10:01:00Z",
              metric_name: "kaiku_process_memory_bytes",
              value_count: 310 * MiB,
              value_sum: null,
              value_p50: null,
              value_p95: null,
              value_p99: null,
            },
          ],
        },
        {
          // f64 gauge: stored in value_sum, not value_count.
          metric_name: "kaiku_process_cpu_percent",
          datapoints: [
            {
              ts: "2026-11-01T10:00:00Z",
              metric_name: "kaiku_process_cpu_percent",
              value_count: null,
              value_sum: 3.5,
              value_p50: null,
              value_p95: null,
              value_p99: null,
            },
          ],
        },
      ],
    },
    obsTopRoutes: { routes: [] },
    obsTopErrors: { error_categories: [] },
    obsLogs: [],
    obsTraces: [],
    obsLinks: null,
    obsTimeRange: "1h",
    obsLastRefresh: null,
    isObsSummaryLoading: false,
    isObsTrendsLoading: false,
    isObsTopRoutesLoading: false,
    isObsTopErrorsLoading: false,
    isObsLogsLoading: false,
    isObsTracesLoading: false,
  },
  loadObsSummary: vi.fn(),
  loadObsTrends: vi.fn(),
  loadObsTopRoutes: vi.fn(),
  loadObsTopErrors: vi.fn(),
  loadObsLogs: vi.fn(),
  loadObsTraces: vi.fn(),
  loadObsLinks: vi.fn(),
  setObsTimeRange: vi.fn(),
}));

import CommandCenterPanel from "../CommandCenterPanel";

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

const chartCard = (title: string) =>
  screen
    .getByRole("heading", { level: 4, name: title })
    .closest("div.rounded-xl") as HTMLElement;

describe("CommandCenterPanel trend charts", () => {
  it("shows a No data hint instead of an empty box when a metric has no datapoints", () => {
    render(() => <CommandCenterPanel />);
    expect(
      within(chartCard("Voice Sessions")).getByText(
        "No data in this time range",
      ),
    ).toBeTruthy();
    expect(
      within(chartCard("Process Memory")).queryByText(
        "No data in this time range",
      ),
    ).toBeNull();
  });

  it("plots CPU from value_sum (f64 gauge) so the CPU chart is not empty", () => {
    render(() => <CommandCenterPanel />);
    expect(
      within(chartCard("CPU %")).queryByText("No data in this time range"),
    ).toBeNull();
  });

  it("formats the memory Y axis as MiB with a width that fits the labels", () => {
    uplotOptions.length = 0;
    render(() => <CommandCenterPanel />);
    type Axis = {
      values: (u: unknown, splits: number[]) => string[];
      size: (u: unknown, values: string[]) => number;
    };
    const ticks = [256 * MiB, 512 * MiB];
    const formatted = uplotOptions
      .map((o) => (o as { axes: Axis[] }).axes[1])
      .map((axis) => axis.values(null, ticks));
    expect(formatted).toContainEqual(["256 MiB", "512 MiB"]);
    const memAxis = uplotOptions
      .map((o) => (o as { axes: Axis[] }).axes[1])
      .find((axis) => axis.values(null, ticks)[0] === "256 MiB")!;
    expect(memAxis.size(null, ["256 MiB", "512 MiB"])).toBeGreaterThan(50);
  });

  it("shows the empty hint for Top Routes and Top Errors", () => {
    render(() => <CommandCenterPanel />);
    expect(screen.getByText("No route data available")).toBeTruthy();
    expect(screen.getByText("No error data available")).toBeTruthy();
  });
});
