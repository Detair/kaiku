import { Component, createSignal, Show, onMount, onCleanup } from "solid-js";
import { Mic, Headphones, Settings } from "lucide-solid";
import { voiceState, toggleMute, toggleDeafen, isPttActive } from "@/stores/voice";
import MicrophoneTest from "./MicrophoneTest";
import ScreenShareButton from "./ScreenShareButton";
import ScreenShareQualityPicker from "./ScreenShareQualityPicker";
import ScreenShareSourcePicker from "./ScreenShareSourcePicker";
import WebcamButton from "./WebcamButton";
import { isTauri } from "@/lib/tauri/common";
import { t } from "@/i18n";

// Shortcut labels are not translated.
const MUTE_SHORTCUT = "Ctrl+Shift+M";
const DEAFEN_SHORTCUT = "Ctrl+Shift+D";

/**
 * Voice controls for mute/deafen/settings.
 */
const VoiceControls: Component = () => {
  const [showMicTest, setShowMicTest] = createSignal(false);
  const [showQualityPicker, setShowQualityPicker] = createSignal(false);
  const [showSourcePicker, setShowSourcePicker] = createSignal(false);
  const [selectedSourceId, setSelectedSourceId] = createSignal<
    string | undefined
  >(undefined);

  // Keyboard shortcuts
  const handleKeyDown = (e: KeyboardEvent) => {
    if (voiceState.state !== "connected") return;
    if (e.ctrlKey && e.shiftKey && e.key === "M") {
      e.preventDefault();
      if (!isPttActive()) toggleMute();
    } else if (e.ctrlKey && e.shiftKey && e.key === "D") {
      e.preventDefault();
      toggleDeafen();
    }
  };

  onMount(() => {
    window.addEventListener("keydown", handleKeyDown);
    onCleanup(() => window.removeEventListener("keydown", handleKeyDown));
  });

  const handleSourceSelected = (sourceId: string) => {
    setShowSourcePicker(false);
    setSelectedSourceId(sourceId);
    setShowQualityPicker(true);
  };

  return (
    <>
      <div class="px-3 py-2 flex items-center justify-center gap-2 border-t border-white/10">
        {/* Mute button */}
        <button
          type="button"
          data-testid="voice-mute"
          aria-label={t("voice.controls.muteLabel")}
          aria-pressed={voiceState.muted}
          onClick={() => { if (!isPttActive()) toggleMute(); }}
          class={`p-2 rounded-full transition-colors ${
            voiceState.muted
              ? "bg-accent-danger/20 text-accent-danger hover:bg-accent-danger/30"
              : "bg-white/5 text-text-secondary hover:bg-white/10 hover:text-text-primary"
          } ${isPttActive() ? "opacity-50 cursor-not-allowed" : ""}`}
          title={
            isPttActive()
              ? t("voice.controls.pttControlled")
              : voiceState.muted
                ? t("voice.controls.unmute", { shortcut: MUTE_SHORTCUT })
                : t("voice.controls.mute", { shortcut: MUTE_SHORTCUT })
          }
          disabled={voiceState.state !== "connected" || isPttActive()}
        >
          {voiceState.muted ? (
            <div style={{ "background-image": "var(--icon-mute)" }} class="w-6 h-6 bg-contain bg-center bg-no-repeat" />
          ) : (
            <Mic class="w-5 h-5" />
          )}
        </button>

        {/* Deafen button */}
        <button
          type="button"
          data-testid="voice-deafen"
          aria-label={t("voice.controls.deafenLabel")}
          aria-pressed={voiceState.deafened}
          onClick={() => toggleDeafen()}
          class={`p-2 rounded-full transition-colors ${
            voiceState.deafened
              ? "bg-accent-danger/20 text-accent-danger hover:bg-accent-danger/30"
              : "bg-white/5 text-text-secondary hover:bg-white/10 hover:text-text-primary"
          }`}
          title={
            voiceState.deafened
              ? t("voice.controls.undeafen", { shortcut: DEAFEN_SHORTCUT })
              : t("voice.controls.deafen", { shortcut: DEAFEN_SHORTCUT })
          }
          disabled={voiceState.state !== "connected"}
        >
          {voiceState.deafened ? (
            <div style={{ "background-image": "var(--icon-deafen)" }} class="w-6 h-6 bg-contain bg-center bg-no-repeat" />
          ) : (
            <Headphones class="w-5 h-5" />
          )}
        </button>

        {/* Screen share button */}
        <ScreenShareButton
          onShowSourcePicker={
            isTauri() ? () => setShowSourcePicker(true) : undefined
          }
          onShowQualityPicker={() => setShowQualityPicker(true)}
        />

        {/* Webcam button */}
        <WebcamButton />

        {/* Settings button */}
        <button
          type="button"
          data-testid="voice-settings"
          aria-label={t("voice.controls.settingsLabel")}
          onClick={() => setShowMicTest(true)}
          class="p-2 rounded-full bg-white/5 text-text-secondary hover:bg-white/10 hover:text-text-primary transition-colors"
          title={t("voice.controls.settings")}
        >
          <Settings class="w-5 h-5" />
        </button>
      </div>

      {/* Microphone Test Modal */}
      <Show when={showMicTest()}>
        <MicrophoneTest onClose={() => setShowMicTest(false)} />
      </Show>

      {/* Native Source Picker (Tauri only) */}
      <Show when={showSourcePicker()}>
        <ScreenShareSourcePicker
          onSelect={handleSourceSelected}
          onClose={() => setShowSourcePicker(false)}
        />
      </Show>

      {/* Screen Share Quality Picker */}
      <Show when={showQualityPicker()}>
        <ScreenShareQualityPicker
          sourceId={selectedSourceId()}
          onClose={() => {
            setShowQualityPicker(false);
            setSelectedSourceId(undefined);
          }}
        />
      </Show>
    </>
  );
};

export default VoiceControls;
