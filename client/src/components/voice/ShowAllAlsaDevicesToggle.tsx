import { Component, Show } from "solid-js";
import type { AudioDevice } from "@/lib/webrtc";
import {
  hasHiddenDevices,
  setShowAllAlsaDevices,
  showAllAlsaDevices,
} from "@/lib/audioDevices";
import { t } from "@/i18n";

/**
 * Advanced option to list every ALSA PCM (Linux desktop app). Rendered only
 * when the device lists contain entries the curated list hides.
 */
const ShowAllAlsaDevicesToggle: Component<{ devices: readonly AudioDevice[] }> = (props) => (
  <Show when={hasHiddenDevices(props.devices)}>
    <label class="flex items-start gap-3 cursor-pointer">
      <input
        type="checkbox"
        checked={showAllAlsaDevices()}
        onChange={(e) => setShowAllAlsaDevices(e.currentTarget.checked)}
        class="mt-0.5 w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
      />
      <span>
        <span class="block text-sm text-text-primary">{t("voice.audio.showAllAlsaDevices")}</span>
        <span class="block text-xs text-text-secondary">
          {t("voice.audio.showAllAlsaDevicesHint")}
        </span>
      </span>
    </label>
  </Show>
);

export default ShowAllAlsaDevicesToggle;
