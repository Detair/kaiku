import { Component, createSignal, For, Show, onMount } from "solid-js";
import { X, Monitor, AppWindow, Loader2 } from "lucide-solid";
import type { CaptureSource } from "@/lib/webrtc/types";

interface ScreenShareSourcePickerProps {
  onSelect: (sourceId: string) => void;
  onClose: () => void;
}

/**
 * Source picker modal for native screen sharing.
 * Shows available monitors and windows as clickable cards.
 */
const ScreenShareSourcePicker: Component<ScreenShareSourcePickerProps> = (
  props,
) => {
  const [sources, setSources] = createSignal<CaptureSource[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<CaptureSource[]>("enumerate_capture_sources");
      setSources(result);
    } catch (err) {
      setError(typeof err === "string" ? err : String(err));
    } finally {
      setLoading(false);
    }
  });

  const monitors = () => sources().filter((s) => s.source_type === "monitor");
  const windows = () => sources().filter((s) => s.source_type === "window");

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
      <div class="bg-background-secondary rounded-lg shadow-xl w-full max-w-lg mx-4 max-h-[80vh] flex flex-col">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-background-primary shrink-0">
          <h2 class="text-lg font-semibold text-text-primary">Share Screen</h2>
          <button
            onClick={props.onClose}
            class="p-1 text-text-muted hover:text-text-primary transition-colors"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div class="p-4 overflow-y-auto flex-1">
          <Show when={loading()}>
            <div class="flex items-center justify-center py-8">
              <Loader2 class="w-6 h-6 text-text-muted animate-spin" />
              <span class="ml-2 text-text-secondary">Detecting sources...</span>
            </div>
          </Show>

          <Show when={error()}>
            <div class="text-center py-8">
              <p class="text-danger text-sm">{error()}</p>
              <button
                onClick={props.onClose}
                class="mt-3 px-4 py-2 text-sm text-text-secondary hover:text-text-primary"
              >
                Close
              </button>
            </div>
          </Show>

          <Show when={!loading() && !error()}>
            {/* Monitors */}
            <Show when={monitors().length > 0}>
              <div class="mb-4">
                <h3 class="text-sm font-medium text-text-secondary mb-2 flex items-center gap-1.5">
                  <Monitor class="w-4 h-4" />
                  Screens
                </h3>
                <div class="grid grid-cols-2 gap-2">
                  <For each={monitors()}>
                    {(source) => (
                      <SourceCard source={source} onSelect={props.onSelect} />
                    )}
                  </For>
                </div>
              </div>
            </Show>

            {/* Windows */}
            <Show when={windows().length > 0}>
              <div>
                <h3 class="text-sm font-medium text-text-secondary mb-2 flex items-center gap-1.5">
                  <AppWindow class="w-4 h-4" />
                  Windows
                </h3>
                <div class="grid grid-cols-2 gap-2">
                  <For each={windows()}>
                    {(source) => (
                      <SourceCard source={source} onSelect={props.onSelect} />
                    )}
                  </For>
                </div>
              </div>
            </Show>

            <Show when={sources().length === 0}>
              <p class="text-center text-text-muted py-8 text-sm">
                No capture sources found.
              </p>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
};

/** Individual source card. */
const SourceCard: Component<{
  source: CaptureSource;
  onSelect: (id: string) => void;
}> = (props) => {
  return (
    <button
      onClick={() => props.onSelect(props.source.id)}
      class="flex flex-col items-center gap-2 p-3 rounded-lg bg-background-primary hover:bg-background-tertiary border border-transparent hover:border-primary transition-colors text-left w-full"
    >
      {/* Thumbnail or placeholder */}
      <div class="w-full aspect-video bg-background-tertiary rounded flex items-center justify-center overflow-hidden">
        <Show
          when={props.source.thumbnail}
          fallback={
            props.source.source_type === "monitor" ? (
              <Monitor class="w-8 h-8 text-text-muted" />
            ) : (
              <AppWindow class="w-8 h-8 text-text-muted" />
            )
          }
        >
          <img
            src={`data:image/png;base64,${props.source.thumbnail}`}
            alt={props.source.name}
            class="w-full h-full object-contain"
          />
        </Show>
      </div>

      {/* Label */}
      <span class="text-xs text-text-primary truncate w-full text-center">
        {props.source.name}
        {props.source.is_primary && (
          <span class="text-text-muted ml-1">(Primary)</span>
        )}
      </span>
    </button>
  );
};

export default ScreenShareSourcePicker;
