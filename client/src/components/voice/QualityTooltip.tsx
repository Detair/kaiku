import { Component } from "solid-js";
import type { ConnectionMetrics } from "../../lib/webrtc/types";
import type { QualityLevel } from "../../lib/types";

interface QualityTooltipProps {
  metrics: ConnectionMetrics;
}

// Map semantic quality levels to user-friendly labels
const qualityLabels: Record<QualityLevel, string> = {
  good: "Excellent",
  warning: "Fair",
  poor: "Poor",
  unknown: "Unknown",
};

// Thresholds for individual metric status (uses warning/poor levels)
const thresholds = {
  latency: { warning: 100, poor: 200 },
  packetLoss: { warning: 1, poor: 3 },
  jitter: { warning: 30, poor: 50 },
};

function getMetricStatus(
  value: number,
  metric: keyof typeof thresholds,
): "ok" | "warning" | "critical" {
  const t = thresholds[metric];
  if (value >= t.poor) return "critical";
  if (value >= t.warning) return "warning";
  return "ok";
}

export const QualityTooltip: Component<QualityTooltipProps> = (props) => {
  const latencyStatus = () => getMetricStatus(props.metrics.latency, "latency");
  const lossStatus = () =>
    getMetricStatus(props.metrics.packetLoss, "packetLoss");
  const jitterStatus = () => getMetricStatus(props.metrics.jitter, "jitter");

  const statusIcon = (status: "ok" | "warning" | "critical") => {
    switch (status) {
      case "ok":
        return "✓";
      case "warning":
        return "⚠";
      case "critical":
        return "✗";
    }
  };

  const statusColor = (status: "ok" | "warning" | "critical") => {
    switch (status) {
      case "ok":
        return "text-green-400";
      case "warning":
        return "text-yellow-400";
      case "critical":
        return "text-red-400";
    }
  };

  return (
    <div class="bg-surface-layer2 rounded-lg p-3 shadow-lg min-w-48">
      <div class="text-sm font-medium text-text-primary mb-2">
        Connection Quality
      </div>
      <div class="border-t border-surface-layer1 my-2" />

      <div class="space-y-1.5 text-xs">
        <div class="flex justify-between items-center">
          <span class="text-text-secondary">Latency:</span>
          <span class="flex items-center gap-1">
            <span
              class={
                latencyStatus() !== "ok"
                  ? "font-medium text-text-primary"
                  : "text-text-secondary"
              }
            >
              {props.metrics.latency}ms
            </span>
            <span class={statusColor(latencyStatus())}>
              {statusIcon(latencyStatus())}
            </span>
          </span>
        </div>

        <div class="flex justify-between items-center">
          <span class="text-text-secondary">Packet Loss:</span>
          <span class="flex items-center gap-1">
            <span
              class={
                lossStatus() !== "ok"
                  ? "font-medium text-text-primary"
                  : "text-text-secondary"
              }
            >
              {props.metrics.packetLoss.toFixed(1)}%
            </span>
            <span class={statusColor(lossStatus())}>
              {statusIcon(lossStatus())}
            </span>
          </span>
        </div>

        <div class="flex justify-between items-center">
          <span class="text-text-secondary">Jitter:</span>
          <span class="flex items-center gap-1">
            <span
              class={
                jitterStatus() !== "ok"
                  ? "font-medium text-text-primary"
                  : "text-text-secondary"
              }
            >
              {props.metrics.jitter}ms
            </span>
            <span class={statusColor(jitterStatus())}>
              {statusIcon(jitterStatus())}
            </span>
          </span>
        </div>
      </div>

      <div class="border-t border-surface-layer1 my-2" />

      <div class="text-xs text-text-secondary">
        Quality:{" "}
        <span class="text-text-primary">
          {qualityLabels[props.metrics.quality]}
        </span>
      </div>
    </div>
  );
};

export default QualityTooltip;
