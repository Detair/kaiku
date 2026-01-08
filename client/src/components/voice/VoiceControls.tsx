import { Component } from "solid-js";
import { Mic, MicOff, Headphones, VolumeX, Settings } from "lucide-solid";
import { voiceState, toggleMute, toggleDeafen } from "@/stores/voice";

/**
 * Voice controls for mute/deafen/settings.
 */
const VoiceControls: Component = () => {
  return (
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
      >
        {voiceState.deafened ? (
          <VolumeX class="w-5 h-5" />
        ) : (
          <Headphones class="w-5 h-5" />
        )}
      </button>

      {/* Settings button */}
      <button
        class="p-2 rounded-full bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary transition-colors"
        title="Voice Settings"
      >
        <Settings class="w-5 h-5" />
      </button>
    </div>
  );
};

export default VoiceControls;
