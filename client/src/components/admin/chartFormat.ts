/**
 * Pure formatting helpers for the Command Center trend charts (uPlot axes).
 *
 * Kept separate from the component so they can be unit tested without
 * rendering a canvas.
 */

/** How a chart's Y values are rendered on the axis. */
export type ChartValueFormat = "number" | "bytes" | "percent";

const BINARY_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"] as const;

/**
 * Human-readable binary byte size for axis ticks (e.g. `512 MiB`, `1.5 GiB`).
 *
 * One decimal below 10 units, none above, so tick labels stay short.
 */
export function formatBinaryBytes(bytes: number): string {
  if (!Number.isFinite(bytes)) return "";
  const sign = bytes < 0 ? "-" : "";
  let v = Math.abs(bytes);
  let u = 0;
  while (v >= 1024 && u < BINARY_UNITS.length - 1) {
    v /= 1024;
    u += 1;
  }
  const digits = u === 0 || v >= 10 ? 0 : 1;
  // Drop a trailing ".0" so "1.0 GiB" reads as "1 GiB".
  const num = v.toFixed(digits).replace(/\.0$/, "");
  return `${sign}${num} ${BINARY_UNITS[u]}`;
}

/** Compact number for axis ticks (e.g. `950`, `12k`, `3.4M`). */
export function formatCompactNumber(n: number): string {
  if (!Number.isFinite(n)) return "";
  const abs = Math.abs(n);
  const trim = (s: string) => s.replace(/\.0$/, "");
  if (abs >= 1_000_000) return `${trim((n / 1_000_000).toFixed(1))}M`;
  if (abs >= 10_000) return `${trim((n / 1_000).toFixed(0))}k`;
  if (abs >= 1_000) return `${trim((n / 1_000).toFixed(1))}k`;
  if (abs > 0 && abs < 100 && !Number.isInteger(n)) return trim(n.toFixed(1));
  return String(Math.round(n));
}

/** Format one axis tick value according to the chart's value format. */
export function formatAxisValue(
  value: number,
  format: ChartValueFormat,
): string {
  switch (format) {
    case "bytes":
      return formatBinaryBytes(value);
    case "percent":
      return `${formatCompactNumber(value)}%`;
    default:
      return formatCompactNumber(value);
  }
}

/**
 * Y-axis width in px that fits the longest tick label (11px sans-serif,
 * ~6.5px per character, plus tick + gap). Never narrower than 40px.
 */
export function axisWidthFor(
  labels: readonly (string | null | undefined)[] | null,
): number {
  const longest = (labels ?? []).reduce(
    (max, l) => Math.max(max, l?.length ?? 0),
    0,
  );
  return Math.max(40, Math.ceil(longest * 6.5) + 16);
}

/** True when a uPlot data set has at least one non-null Y value. */
export function hasPlottableValues(
  series: ArrayLike<number | null | undefined>,
): boolean {
  return Array.from(series).some((v) => v != null && Number.isFinite(v));
}
