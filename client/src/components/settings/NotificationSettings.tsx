/**
 * Notification Settings
 *
 * Sound notification settings with sound selection, volume control, and test button.
 */

import { Component, For, createSignal } from "solid-js";
import { Check, Volume2, Play } from "lucide-solid";
import {
  soundSettings,
  setSoundEnabled,
  setSoundVolume,
  setSelectedSound,
} from "@/stores/sound";
import { AVAILABLE_SOUNDS, type SoundInfo } from "@/lib/sound/types";
import { testSound } from "@/lib/sound";

const NotificationSettings: Component = () => {
  const [isTesting, setIsTesting] = createSignal(false);

  const handleTestSound = async () => {
    if (isTesting()) return;
    setIsTesting(true);
    try {
      await testSound(soundSettings().selectedSound);
    } catch (err) {
      console.error("Failed to play test sound:", err);
    } finally {
      // Reset after brief delay for visual feedback
      setTimeout(() => setIsTesting(false), 500);
    }
  };

  const handleSoundSelect = async (soundId: string) => {
    setSelectedSound(soundId as any);
    // Play the newly selected sound for preview
    try {
      await testSound(soundId as any);
    } catch (err) {
      console.error("Failed to play preview sound:", err);
    }
  };

  return (
    <div class="space-y-6">
      {/* Master enable toggle */}
      <div>
        <h3 class="text-lg font-semibold mb-4 text-text-primary">
          Sound Notifications
        </h3>

        <label class="flex items-center gap-3 cursor-pointer">
          <input
            type="checkbox"
            checked={soundSettings().enabled}
            onChange={(e) => setSoundEnabled(e.currentTarget.checked)}
            class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
          />
          <span class="text-text-primary">Enable notification sounds</span>
        </label>
      </div>

      {/* Sound selection */}
      <div
        classList={{
          "opacity-50 pointer-events-none": !soundSettings().enabled,
        }}
      >
        <h4 class="text-base font-medium mb-3 text-text-primary">
          Notification Sound
        </h4>
        <p class="text-sm text-text-secondary mb-4">
          Choose the sound that plays for new messages
        </p>

        <div class="space-y-3">
          <For each={AVAILABLE_SOUNDS}>
            {(sound: SoundInfo) => (
              <button
                onClick={() => handleSoundSelect(sound.id)}
                class="w-full text-left p-4 rounded-xl border-2 transition-all duration-200"
                classList={{
                  "border-accent-primary bg-accent-primary/10":
                    soundSettings().selectedSound === sound.id,
                  "border-white/10 hover:border-accent-primary/50 hover:bg-white/5":
                    soundSettings().selectedSound !== sound.id,
                }}
              >
                <div class="flex items-start gap-3">
                  {/* Radio indicator */}
                  <div
                    class="w-5 h-5 rounded-full border-2 flex items-center justify-center flex-shrink-0 mt-0.5 transition-colors"
                    classList={{
                      "border-accent-primary bg-accent-primary":
                        soundSettings().selectedSound === sound.id,
                      "border-white/30":
                        soundSettings().selectedSound !== sound.id,
                    }}
                  >
                    {soundSettings().selectedSound === sound.id && (
                      <Check class="w-3 h-3 text-surface-base" />
                    )}
                  </div>

                  {/* Sound info */}
                  <div class="flex-1">
                    <span class="font-semibold text-text-primary">
                      {sound.name}
                    </span>
                    <div class="text-sm text-text-secondary mt-0.5">
                      {sound.description}
                    </div>
                  </div>
                </div>
              </button>
            )}
          </For>
        </div>
      </div>

      {/* Volume control */}
      <div
        classList={{
          "opacity-50 pointer-events-none": !soundSettings().enabled,
        }}
      >
        <h4 class="text-base font-medium mb-3 text-text-primary">Volume</h4>

        <div class="flex items-center gap-4">
          <Volume2 class="w-5 h-5 text-text-secondary flex-shrink-0" />

          <input
            type="range"
            min="0"
            max="100"
            value={soundSettings().volume}
            onInput={(e) => setSoundVolume(parseInt(e.currentTarget.value))}
            class="flex-1 h-2 rounded-full bg-surface-highlight appearance-none cursor-pointer
                   [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4
                   [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-accent-primary
                   [&::-webkit-slider-thumb]:cursor-pointer [&::-webkit-slider-thumb]:transition-transform
                   [&::-webkit-slider-thumb]:hover:scale-110
                   [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:rounded-full
                   [&::-moz-range-thumb]:bg-accent-primary [&::-moz-range-thumb]:cursor-pointer [&::-moz-range-thumb]:border-0"
          />

          <span class="text-sm text-text-secondary w-12 text-right">
            {soundSettings().volume}%
          </span>

          <button
            onClick={handleTestSound}
            disabled={isTesting()}
            class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface-highlight hover:bg-white/10
                   text-text-primary text-sm font-medium transition-colors disabled:opacity-50"
          >
            <Play class="w-4 h-4" />
            Test
          </button>
        </div>
      </div>

      {/* Info text */}
      <p class="text-xs text-text-muted">
        Sounds will only play for messages from others, and respect per-channel
        notification settings.
      </p>
    </div>
  );
};

export default NotificationSettings;
