import { Component, createSignal, Show } from "solid-js";
import { Mic, MicOff, Headphones, VolumeX, Settings } from "lucide-solid";
import { voiceState, toggleMute, toggleDeafen } from "@/stores/voice";
import MicrophoneTest from "./MicrophoneTest";

/**
 * Voice controls for mute/deafen/settings.
 */
const VoiceControls: Component = () => {
  const [showMicTest, setShowMicTest] = createSignal(false);

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
    </>
  );
};

export default VoiceControls;
