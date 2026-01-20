import { Component, Show } from 'solid-js';
import type { ConnectionMetrics, QualityLevel } from '../../lib/webrtc/types';

interface QualityIndicatorProps {
  metrics: ConnectionMetrics | 'unknown' | null;
  mode: 'circle' | 'number';
  class?: string;
}

const qualityColors: Record<QualityLevel, string> = {
  green: 'bg-green-500',
  yellow: 'bg-yellow-500',
  orange: 'bg-orange-500',
  red: 'bg-red-500',
};

const qualityTextColors: Record<QualityLevel, string> = {
  green: 'text-green-500',
  yellow: 'text-yellow-500',
  orange: 'text-orange-500',
  red: 'text-red-500',
};

export const QualityIndicator: Component<QualityIndicatorProps> = (props) => {
  const isLoading = () => props.metrics === null || props.metrics === 'unknown';
  const metrics = () => (typeof props.metrics === 'object' ? props.metrics : null);

  return (
    <div class={`inline-flex items-center ${props.class ?? ''}`}>
      <Show
        when={!isLoading()}
        fallback={
          <div class="w-2 h-2 rounded-full bg-gray-500 animate-pulse" />
        }
      >
        <Show
          when={props.mode === 'circle'}
          fallback={
            <span class={`text-xs font-mono ${qualityTextColors[metrics()!.quality]}`}>
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
