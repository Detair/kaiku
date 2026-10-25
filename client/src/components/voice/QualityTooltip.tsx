import { Component } from "solid-js";
import type { ConnectionMetrics } from "../../lib/webrtc/types";
import type { QualityLevel } from "../../lib/types";
import { t, type MessageKey } from "@/i18n";

interface QualityTooltipProps {
  metrics: ConnectionMetrics;
}

// Map semantic quality levels to user-friendly labels
const qualityLabels: Record<QualityLevel, MessageKey> = {
  good: "voice.quality.level.good",
  warning: "voice.quality.level.warning",
  poor: "voice.quality.level.poor",
  unknown: "voice.quality.level.unknown",
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
  const limits = thresholds[metric];
  if (value >= limits.poor) return "critical";
  if (value >= limits.warning) return "warning";
  return "ok";
}

export const QualityTooltip: Component<QualityTooltipProps> = (props) => {
  const m = () => props.metrics;
  const latencyStatus = () => m() ? getMetricStatus(m()!.latency, "latency") : "ok" as const;
  const lossStatus = () => m() ? getMetricStatus(m()!.packetLoss, "packetLoss") : "ok" as const;
  const jitterStatus = () => m() ? getMetricStatus(m()!.jitter, "jitter") : "ok" as const;

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
        {t("voice.quality.title")}
      </div>
      <div class="border-t border-surface-layer1 my-2" />

      <div class="space-y-1.5 text-xs">
        <div class="flex justify-between items-center">
          <span class="text-text-secondary">{t("voice.quality.latency")}</span>
          <span class="flex items-center gap-1">
            <span
              class={
                latencyStatus() !== "ok"
                  ? "font-medium text-text-primary"
                  : "text-text-secondary"
              }
            >
              {t("common.unit.ms", { value: m()?.latency ?? "" })}
            </span>
            <span class={statusColor(latencyStatus())}>
              {statusIcon(latencyStatus())}
            </span>
          </span>
        </div>

        <div class="flex justify-between items-center">
          <span class="text-text-secondary">{t("voice.quality.packetLoss")}</span>
          <span class="flex items-center gap-1">
            <span
              class={
                lossStatus() !== "ok"
                  ? "font-medium text-text-primary"
                  : "text-text-secondary"
              }
            >
              {t("common.unit.percent", {
                value: m()?.packetLoss?.toFixed(1) ?? "0.0",
              })}
            </span>
            <span class={statusColor(lossStatus())}>
              {statusIcon(lossStatus())}
            </span>
          </span>
        </div>

        <div class="flex justify-between items-center">
          <span class="text-text-secondary">{t("voice.quality.jitter")}</span>
          <span class="flex items-center gap-1">
            <span
              class={
                jitterStatus() !== "ok"
                  ? "font-medium text-text-primary"
                  : "text-text-secondary"
              }
            >
              {t("common.unit.ms", { value: m()?.jitter ?? "" })}
            </span>
            <span class={statusColor(jitterStatus())}>
              {statusIcon(jitterStatus())}
            </span>
          </span>
        </div>
      </div>

      <div class="border-t border-surface-layer1 my-2" />

      <div class="text-xs text-text-secondary">
        {t("voice.quality.summary", {
          level: t(qualityLabels[m()?.quality ?? "unknown"]),
        })}
      </div>
    </div>
  );
};

export default QualityTooltip;
