/**
 * Notification Settings
 *
 * Sound notification settings with sound selection, volume control, and test button.
 * Desktop notification settings with OS notification toggles and test button.
 */

import { Component, For, Show, createSignal, createMemo } from "solid-js";
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
import {
  sendOsNotification,
  getBrowserNotificationPermission,
  requestBrowserNotificationPermission,
  type BrowserNotificationPermission,
} from "@/lib/notifications";
import { isTauri } from "@/lib/tauri/common";
import { t } from "@/i18n";

const NotificationSettings: Component = () => {
  const [isTesting, setIsTesting] = createSignal(false);
  const [permission, setPermission] = createSignal<BrowserNotificationPermission>(
    isTauri() ? "granted" : getBrowserNotificationPermission(),
  );
  const [showTestHint, setShowTestHint] = createSignal(false);
  const permissionText = () => {
    switch (permission()) {
      case "granted":
        return t("notifications.permission.granted");
      case "default":
        return t("notifications.permission.default");
      case "denied":
        return t("notifications.permission.denied");
      default:
        return t("notifications.permission.unsupported");
    }
  };
  const permissionMissing = () => !isTauri() && permission() !== "granted";

  const handleAllowNotifications = async () => {
    setPermission(await requestBrowserNotificationPermission());
  };

  // Quiet hours status preview
  const quietHoursStatus = createMemo(() => {
    const quietHours = getQuietHours();
    if (!quietHours.enabled) {
      return null;
    }
    if (isWithinQuietHours()) {
      return { active: true, text: t("notifications.quietHours.active") };
    }
    return {
      active: false,
      text: t("notifications.quietHours.next", { time: quietHours.start_time }),
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
          {t("notifications.sound.title")}
        </h3>

        <label class="flex items-center gap-3 cursor-pointer">
          <input
            type="checkbox"
            checked={soundSettings().enabled}
            onChange={(e) => setSoundEnabled(e.currentTarget.checked)}
            class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
          />
          <span class="text-text-primary">{t("notifications.sound.enable")}</span>
        </label>
      </div>

      {/* Sound selection */}
      <div
        classList={{
          "opacity-50 pointer-events-none": !soundSettings().enabled,
        }}
      >
        <h4 class="text-base font-medium mb-3 text-text-primary">
          {t("notifications.sound.selectTitle")}
        </h4>
        <p class="text-sm text-text-secondary mb-4">
          {t("notifications.sound.selectHint")}
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
                      <Check class="w-3 h-3 text-on-accent" />
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
        <h4 class="text-base font-medium mb-3 text-text-primary">{t("notifications.sound.volume")}</h4>

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
            {t("common.unit.percent", { value: soundSettings().volume })}
          </span>

          <button
            onClick={handleTestSound}
            disabled={isTesting()}
            class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface-highlight hover:bg-white/10
                   text-text-primary text-sm font-medium transition-colors disabled:opacity-50"
          >
            <Play class="w-4 h-4" />
            {t("notifications.sound.test")}
          </button>
        </div>
      </div>

      {/* Quiet Hours */}
      <div>
        <h3 class="text-lg font-semibold mb-4 text-text-primary flex items-center gap-2">
          <Moon class="w-5 h-5" />
          {t("notifications.quietHours.title")}
        </h3>

        <p class="text-sm text-text-secondary mb-4">
          {t("notifications.quietHours.hint")}
        </p>

        <label class="flex items-center gap-3 cursor-pointer mb-4">
          <input
            type="checkbox"
            checked={getQuietHours().enabled}
            onChange={(e) => setQuietHoursEnabled(e.currentTarget.checked)}
            class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
          />
          <span class="text-text-primary">{t("notifications.quietHours.enable")}</span>
        </label>

        <div
          classList={{
            "opacity-50 pointer-events-none": !getQuietHours().enabled,
          }}
        >
          <div class="flex items-center gap-4 mb-4">
            <div class="flex items-center gap-2">
              <Clock class="w-4 h-4 text-text-secondary" />
              <span class="text-sm text-text-secondary">{t("notifications.quietHours.from")}</span>
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
              <span class="text-sm text-text-secondary">{t("notifications.quietHours.to")}</span>
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
          {t("notifications.desktop.title")}
        </h3>

        <p class="text-sm text-text-secondary mb-4">
          {t("notifications.desktop.hint")}
        </p>

        <Show when={!isTauri()}>
          <div class="mb-4 p-3 rounded-lg bg-surface-highlight space-y-2" data-testid="notification-permission">
            <div class="text-sm font-medium text-text-primary">
              {t("notifications.permission.title")}
            </div>
            <p class="text-sm text-text-secondary">
              {permissionText()}
            </p>
            <Show when={permission() === "default"}>
              <button
                onClick={handleAllowNotifications}
                class="px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium transition-colors hover:opacity-90"
              >
                {t("notifications.permission.allow")}
              </button>
            </Show>
          </div>
        </Show>

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
            <span class="text-text-primary">{t("notifications.desktop.enable")}</span>
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
            <span class="text-text-primary">{t("notifications.desktop.showContent")}</span>
          </label>

          <Show when={isTauri()}>
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
            <span class="text-text-primary">{t("notifications.desktop.flashTaskbar")}</span>
          </label>
          </Show>
        </div>

        <button
          onClick={() => {
            if (permissionMissing()) {
              setShowTestHint(true);
              return;
            }
            setShowTestHint(false);
            sendOsNotification(
              { type: "message_dm", channelId: "test", isDm: true },
              { username: "Kaiku", content: t("notifications.desktop.testBody"), guildName: null, channelName: null },
              preferences().notifications?.show_content ?? true,
              true,
            );
          }}
          disabled={!(preferences().notifications?.os_enabled ?? true)}
          class="mt-4 flex items-center gap-2 px-4 py-2 rounded-lg bg-surface-highlight hover:bg-white/10
                 text-text-primary text-sm font-medium transition-colors disabled:opacity-50"
        >
          <Bell class="w-4 h-4" />
          {t("notifications.desktop.sendTest")}
        </button>
        <Show when={showTestHint() && permissionMissing()}>
          <p class="mt-2 text-sm text-text-secondary" role="status">
            {permission() === "default"
              ? t("notifications.permission.testNeeded")
              : permissionText()}
          </p>
        </Show>
      </div>

      {/* Info text */}
      <p class="text-xs text-text-muted">
        {t("notifications.settings.info")}
      </p>
    </div>
  );
};

export default NotificationSettings;
