/**
 * Notification Settings
 *
 * Sound notification settings with sound selection, volume control, and test button.
 * Desktop notification settings with OS notification toggles and test button.
 */

import { Component, For, createSignal, createMemo } from "solid-js";
import { Check, Volume2, Play, Moon, Clock, Bell } from "lucide-solid";
import {
  soundSettings,
  setSoundEnabled,
  setSoundVolume,
  setSelectedSound,
  getQuietHours,
  setQuietHoursEnabled,
  setQuietHoursTime,
  isWithinQuietHours,
  type SoundOption,
} from "@/stores/sound";
import { AVAILABLE_SOUNDS, type SoundInfo } from "@/lib/sound/types";
import { testSound } from "@/lib/sound";
import { preferences, updatePreference } from "@/stores/preferences";
import { sendOsNotification } from "@/lib/notifications";

const NotificationSettings: Component = () => {
  const [isTesting, setIsTesting] = createSignal(false);

  // Quiet hours status preview
  const quietHoursStatus = createMemo(() => {
    const quietHours = getQuietHours();
    if (!quietHours.enabled) {
      return null;
    }
    if (isWithinQuietHours()) {
      return { active: true, text: "Quiet hours active" };
    }
    return {
      active: false,
      text: `Next quiet period: ${quietHours.start_time}`,
    };
  });

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
    setSelectedSound(soundId as SoundOption);
    // Play the newly selected sound for preview
    try {
      await testSound(soundId as SoundOption);
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
                      <Check class="w-3 h-3 text-white" />
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

      {/* Quiet Hours */}
      <div>
        <h3 class="text-lg font-semibold mb-4 text-text-primary flex items-center gap-2">
          <Moon class="w-5 h-5" />
          Quiet Hours
        </h3>

        <p class="text-sm text-text-secondary mb-4">
          Automatically suppress notification sounds during scheduled times
        </p>

        <label class="flex items-center gap-3 cursor-pointer mb-4">
          <input
            type="checkbox"
            checked={getQuietHours().enabled}
            onChange={(e) => setQuietHoursEnabled(e.currentTarget.checked)}
            class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
          />
          <span class="text-text-primary">Enable quiet hours</span>
        </label>

        <div
          classList={{
            "opacity-50 pointer-events-none": !getQuietHours().enabled,
          }}
        >
          <div class="flex items-center gap-4 mb-4">
            <div class="flex items-center gap-2">
              <Clock class="w-4 h-4 text-text-secondary" />
              <span class="text-sm text-text-secondary">From</span>
              <input
                type="time"
                value={getQuietHours().start_time}
                onChange={(e) =>
                  setQuietHoursTime(
                    e.currentTarget.value,
                    getQuietHours().end_time,
                  )
                }
                class="px-3 py-1.5 rounded-lg bg-surface-highlight border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary transition-colors"
              />
            </div>

            <div class="flex items-center gap-2">
              <span class="text-sm text-text-secondary">To</span>
              <input
                type="time"
                value={getQuietHours().end_time}
                onChange={(e) =>
                  setQuietHoursTime(
                    getQuietHours().start_time,
                    e.currentTarget.value,
                  )
                }
                class="px-3 py-1.5 rounded-lg bg-surface-highlight border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary transition-colors"
              />
            </div>
          </div>

          {/* Status preview */}
          {quietHoursStatus() && (
            <div
              class="flex items-center gap-2 px-3 py-2 rounded-lg text-sm"
              classList={{
                "bg-accent-primary/10 text-accent-primary":
                  quietHoursStatus()?.active,
                "bg-surface-highlight text-text-secondary":
                  !quietHoursStatus()?.active,
              }}
            >
              <Moon class="w-4 h-4" />
              <span>{quietHoursStatus()?.text}</span>
            </div>
          )}
        </div>
      </div>

      {/* Desktop Notifications */}
      <div>
        <h3 class="text-lg font-semibold mb-4 text-text-primary flex items-center gap-2">
          <Bell class="w-5 h-5" />
          Desktop Notifications
        </h3>

        <p class="text-sm text-text-secondary mb-4">
          Show native OS notifications when the window is not focused
        </p>

        <div class="space-y-3">
          <label class="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={preferences().notifications?.os_enabled ?? true}
              onChange={(e) =>
                updatePreference("notifications", {
                  ...(preferences().notifications ?? { os_enabled: true, show_content: true, flash_taskbar: true }),
                  os_enabled: e.currentTarget.checked,
                })
              }
              class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
            />
            <span class="text-text-primary">Enable desktop notifications</span>
          </label>

          <label
            class="flex items-center gap-3 cursor-pointer"
            classList={{
              "opacity-50 pointer-events-none": !(preferences().notifications?.os_enabled ?? true),
            }}
          >
            <input
              type="checkbox"
              checked={preferences().notifications?.show_content ?? true}
              onChange={(e) =>
                updatePreference("notifications", {
                  ...(preferences().notifications ?? { os_enabled: true, show_content: true, flash_taskbar: true }),
                  show_content: e.currentTarget.checked,
                })
              }
              class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
            />
            <span class="text-text-primary">Show message content in notifications</span>
          </label>

          <label
            class="flex items-center gap-3 cursor-pointer"
            classList={{
              "opacity-50 pointer-events-none": !(preferences().notifications?.os_enabled ?? true),
            }}
          >
            <input
              type="checkbox"
              checked={preferences().notifications?.flash_taskbar ?? true}
              onChange={(e) =>
                updatePreference("notifications", {
                  ...(preferences().notifications ?? { os_enabled: true, show_content: true, flash_taskbar: true }),
                  flash_taskbar: e.currentTarget.checked,
                })
              }
              class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
            />
            <span class="text-text-primary">Flash taskbar on notification</span>
          </label>
        </div>

        <button
          onClick={() => {
            sendOsNotification(
              { type: "message_dm", channelId: "test", isDm: true },
              { username: "Kaiku", content: "This is a test notification!", guildName: null, channelName: null },
              preferences().notifications?.show_content ?? true,
            );
          }}
          disabled={!(preferences().notifications?.os_enabled ?? true)}
          class="mt-4 flex items-center gap-2 px-4 py-2 rounded-lg bg-surface-highlight hover:bg-white/10
                 text-text-primary text-sm font-medium transition-colors disabled:opacity-50"
        >
          <Bell class="w-4 h-4" />
          Send test notification
        </button>
      </div>

      {/* Info text */}
      <p class="text-xs text-text-muted">
        Sounds will only play for messages from others, and respect per-channel
        notification settings. Setting your status to "Do Not Disturb" will also
        suppress sounds.
      </p>
    </div>
  );
};

export default NotificationSettings;
