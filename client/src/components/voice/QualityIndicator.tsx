import { Component, Show } from "solid-js";
import type { ConnectionMetrics } from "../../lib/webrtc/types";
import type { QualityLevel } from "../../lib/types";

interface QualityIndicatorProps {
  metrics: ConnectionMetrics | "unknown" | null;
  mode: "circle" | "number";
  class?: string;
}

// Map semantic quality levels to theme-aware colors
const qualityColors: Record<QualityLevel, string> = {
  good: "bg-accent-success",
  warning: "bg-accent-warning",
  poor: "bg-accent-danger",
  unknown: "bg-text-secondary",
};

const qualityTextColors: Record<QualityLevel, string> = {
  good: "text-accent-success",
  warning: "text-accent-warning",
  poor: "text-accent-danger",
  unknown: "text-text-secondary",
};

export const QualityIndicator: Component<QualityIndicatorProps> = (props) => {
  const isLoading = () => props.metrics === null || props.metrics === "unknown";
  const metrics = () =>
    typeof props.metrics === "object" ? props.metrics : null;

  return (
    <div class={`inline-flex items-center ${props.class ?? ""}`}>
      <Show
        when={!isLoading()}
        fallback={
          <div class="w-2 h-2 rounded-full bg-text-secondary animate-pulse" />
        }
      >
        <Show
          when={props.mode === "circle"}
          fallback={
            <span
              class={`text-xs font-mono ${qualityTextColors[metrics()!.quality]}`}
            >
              {metrics()!.latency}ms
            </span>
          }
        >
          <div
            class={`w-2 h-2 rounded-full ${qualityColors[metrics()!.quality]}`}
          />
        </Show>
      </Show>
    </div>
  );
};

export default QualityIndicator;
