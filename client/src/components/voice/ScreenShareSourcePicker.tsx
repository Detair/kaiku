import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Monitor, AppWindow, Loader2 } from "lucide-solid";
import DialogShell from "@/components/auth/DialogShell";
import type { CaptureSource } from "@/lib/webrtc/types";
import { t } from "@/i18n";

interface ScreenShareSourcePickerProps {
  onSelect: (sourceId: string) => void;
  onClose: () => void;
}

/**
 * Source picker for native screen sharing (Windows, macOS). Linux skips it:
 * the system dialog (portal) chooses the screen there.
 */
const ScreenShareSourcePicker: Component<ScreenShareSourcePickerProps> = (props) => {
  const [sources, setSources] = createSignal<CaptureSource[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [failed, setFailed] = createSignal(false);

  onMount(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      setSources(await invoke<CaptureSource[]>("enumerate_capture_sources"));
    } catch (err) {
      console.error("[ScreenShareSourcePicker] Could not list sources:", err);
      setFailed(true);
    } finally {
      setLoading(false);
    }
  });

  const monitors = () => sources().filter((s) => s.source_type === "monitor");
  const windows = () => sources().filter((s) => s.source_type === "window");

  return (
    <DialogShell title={t("voice.screenShare.dialogTitle")} onClose={props.onClose} wide>
      <Show when={loading()}>
        <div role="status" class="flex items-center justify-center py-8">
          <Loader2 class="w-6 h-6 text-text-secondary animate-spin" />
          <span class="ml-2 text-text-secondary">{t("voice.screenShare.sourcesLoading")}</span>
        </div>
      </Show>

      <Show when={failed()}>
        <p role="alert" class="text-sm text-text-primary py-4">
          {t("voice.screenShare.error.noSource")}
        </p>
      </Show>

      <Show when={!loading() && !failed()}>
        <Show when={monitors().length > 0}>
          <div>
            <h3 class="text-sm font-medium text-text-secondary mb-2 flex items-center gap-1.5">
              <Monitor class="w-4 h-4" />
              {t("voice.screenShare.screens")}
            </h3>
            <div class="grid grid-cols-2 gap-2">
              <For each={monitors()}>
                {(source) => <SourceCard source={source} onSelect={props.onSelect} />}
              </For>
            </div>
          </div>
        </Show>
        <Show when={windows().length > 0}>
          <div>
            <h3 class="text-sm font-medium text-text-secondary mb-2 flex items-center gap-1.5">
              <AppWindow class="w-4 h-4" />
              {t("voice.screenShare.windows")}
            </h3>
            <div class="grid grid-cols-2 gap-2">
              <For each={windows()}>
                {(source) => <SourceCard source={source} onSelect={props.onSelect} />}
              </For>
            </div>
          </div>
        </Show>
        <Show when={sources().length === 0}>
          <p class="text-center text-text-secondary py-8 text-sm">
            {t("voice.screenShare.error.noSource")}
          </p>
        </Show>
      </Show>
    </DialogShell>
  );
};

/** Individual source card. */
const SourceCard: Component<{
  source: CaptureSource;
  onSelect: (id: string) => void;
}> = (props) => (
  <button
    type="button"
    onClick={() => props.onSelect(props.source.id)}
    class="flex flex-col items-center gap-2 p-3 rounded-lg border border-white/10 hover:bg-white/5 hover:border-accent-primary transition-colors text-left w-full"
  >
    <div class="w-full aspect-video bg-white/5 rounded flex items-center justify-center overflow-hidden">
      <Show
        when={props.source.thumbnail}
        fallback={
          props.source.source_type === "monitor" ? (
            <Monitor class="w-8 h-8 text-text-secondary" />
          ) : (
            <AppWindow class="w-8 h-8 text-text-secondary" />
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
    <span class="text-xs text-text-primary truncate w-full text-center">{props.source.name}</span>
  </button>
);

export default ScreenShareSourcePicker;
