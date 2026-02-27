import { Component, createSignal, Show } from "solid-js";
import { Mic, MicOff, Headphones, VolumeX, Settings } from "lucide-solid";
import { voiceState, toggleMute, toggleDeafen } from "@/stores/voice";
import MicrophoneTest from "./MicrophoneTest";
import ScreenShareButton from "./ScreenShareButton";
import ScreenShareQualityPicker from "./ScreenShareQualityPicker";
import ScreenShareSourcePicker from "./ScreenShareSourcePicker";
import WebcamButton from "./WebcamButton";

// Detect if running in Tauri (native source picker available)
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

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

  const handleSourceSelected = (sourceId: string) => {
    setShowSourcePicker(false);
    setSelectedSourceId(sourceId);
    setShowQualityPicker(true);
  };

  return (
    <>
      <div class="px-3 py-2 flex items-center justify-center gap-2 border-t border-background-secondary">
        {/* Mute button */}
        <button
          onClick={() => toggleMute()}
          class={`p-2 rounded-full transition-colors ${
            voiceState.muted
              ? "bg-danger/20 text-danger hover:bg-danger/30"
              : "bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary"
          }`}
          title={voiceState.muted ? "Unmute" : "Mute"}
          disabled={voiceState.state !== "connected"}
        >
          {voiceState.muted ? (
            <MicOff class="w-5 h-5" />
          ) : (
            <Mic class="w-5 h-5" />
          )}
        </button>

        {/* Deafen button */}
        <button
          onClick={() => toggleDeafen()}
          class={`p-2 rounded-full transition-colors ${
            voiceState.deafened
              ? "bg-danger/20 text-danger hover:bg-danger/30"
              : "bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary"
          }`}
          title={voiceState.deafened ? "Undeafen" : "Deafen"}
          disabled={voiceState.state !== "connected"}
        >
          {voiceState.deafened ? (
            <VolumeX class="w-5 h-5" />
          ) : (
            <Headphones class="w-5 h-5" />
          )}
        </button>

        {/* Screen share button */}
        <ScreenShareButton
          onShowSourcePicker={
            isTauri ? () => setShowSourcePicker(true) : undefined
          }
          onShowQualityPicker={() => setShowQualityPicker(true)}
        />

        {/* Webcam button */}
        <WebcamButton />

        {/* Settings button */}
        <button
          onClick={() => setShowMicTest(true)}
          class="p-2 rounded-full bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary transition-colors"
          title="Voice Settings"
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
