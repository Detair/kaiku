import { Component, createSignal, For } from "solid-js";
import { X, Monitor } from "lucide-solid";
import { startScreenShare } from "@/stores/voice";
import type { ScreenShareQuality } from "@/lib/webrtc/types";

interface ScreenShareQualityPickerProps {
  /** Native capture source ID (passed from ScreenShareSourcePicker). */
  sourceId?: string;
  onClose: () => void;
}

const qualityOptions: {
  value: ScreenShareQuality;
  label: string;
  description: string;
  premium?: boolean;
}[] = [
  {
    value: "low",
    label: "480p 15fps",
    description: "Best for slow connections",
  },
  { value: "medium", label: "720p 30fps", description: "Recommended" },
  { value: "high", label: "1080p 30fps", description: "Good connections" },
  {
    value: "premium",
    label: "1080p 60fps",
    description: "Premium only",
    premium: true,
  },
];

/**
 * Quality selection dialog shown before starting screen share.
 */
const ScreenShareQualityPicker: Component<ScreenShareQualityPickerProps> = (
  props,
) => {
  const [selectedQuality, setSelectedQuality] =
    createSignal<ScreenShareQuality>("medium");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleStart = async () => {
    setError(null);
    setLoading(true);

    try {
      const result = await startScreenShare(selectedQuality(), props.sourceId);

      if (!result.ok) {
        setError(result.error ?? "Failed to start screen share");
        return;
      }

      props.onClose();
    } finally {
      setLoading(false);
    }
  };

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <div
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      onClick={handleBackdropClick}
    >
      <div class="bg-background-secondary rounded-lg shadow-xl w-full max-w-sm mx-4">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-background-primary">
          <div class="flex items-center gap-2">
            <Monitor class="w-5 h-5 text-primary" />
            <h2 class="text-lg font-semibold text-text-primary">
              Share Screen
            </h2>
          </div>
          <button
            onClick={props.onClose}
            class="p-1 text-text-muted hover:text-text-primary transition-colors"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Quality options */}
        <div class="p-4 space-y-2">
          <p class="text-sm text-text-secondary mb-3">Select quality:</p>

          <For each={qualityOptions}>
            {(option) => (
              <label
                class={`flex items-center gap-3 p-3 rounded-lg cursor-pointer transition-colors ${
                  selectedQuality() === option.value
                    ? "bg-primary/20 border border-primary"
                    : "bg-background-primary hover:bg-background-tertiary border border-transparent"
                } ${option.premium ? "opacity-50 cursor-not-allowed" : ""}`}
              >
                <input
                  type="radio"
                  name="quality"
                  value={option.value}
                  checked={selectedQuality() === option.value}
                  onChange={() =>
                    !option.premium && setSelectedQuality(option.value)
                  }
                  disabled={option.premium}
                  class="w-4 h-4 text-primary"
                />
                <div class="flex-1">
                  <div class="flex items-center gap-2">
                    <span class="text-sm font-medium text-text-primary">
                      {option.label}
                    </span>
                    {option.premium && (
                      <span class="text-xs px-1.5 py-0.5 bg-warning/20 text-warning rounded">
                        Premium
                      </span>
                    )}
                  </div>
                  <span class="text-xs text-text-muted">
                    {option.description}
                  </span>
                </div>
              </label>
            )}
          </For>
        </div>

        {/* Error message */}
        {error() && (
          <div class="px-4 pb-2">
            <p class="text-sm text-danger">{error()}</p>
          </div>
        )}

        {/* Actions */}
        <div class="flex justify-end gap-2 p-4 border-t border-background-primary">
          <button
            onClick={props.onClose}
            class="px-4 py-2 text-sm text-text-secondary hover:text-text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleStart}
            disabled={loading()}
            class="px-4 py-2 text-sm bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {loading() ? "Starting..." : "Start Sharing"}
          </button>
        </div>
      </div>
    </div>
  );
};

export default ScreenShareQualityPicker;
