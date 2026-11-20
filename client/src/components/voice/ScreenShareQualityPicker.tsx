import { Component, createSignal, For, Show } from "solid-js";
import DialogShell from "@/components/auth/DialogShell";
import { startScreenShare, cancelMediaStart } from "@/stores/voice";
import type { ScreenShareQuality } from "@/lib/webrtc/types";
import { t, type MessageKey } from "@/i18n";

interface ScreenShareQualityPickerProps {
  /** Native capture source ID (from ScreenShareSourcePicker; none on Linux). */
  sourceId?: string;
  /** Linux desktop: the system dialog (portal) chooses the screen. */
  systemPicker?: boolean;
  onClose: () => void;
}

/** Three plain tiers; the server still accepts "premium" from older clients. */
const TIERS: { value: ScreenShareQuality; label: MessageKey; hint: MessageKey }[] = [
  { value: "low", label: "voice.screenShare.tier.low", hint: "voice.screenShare.tier.lowHint" },
  {
    value: "medium",
    label: "voice.screenShare.tier.medium",
    hint: "voice.screenShare.tier.mediumHint",
  },
  { value: "high", label: "voice.screenShare.tier.high", hint: "voice.screenShare.tier.highHint" },
];

/**
 * Quality choice before a screen share starts. A cancelled browser picker
 * closes quietly; a refusal or error stays visible here.
 */
const ScreenShareQualityPicker: Component<ScreenShareQualityPickerProps> = (props) => {
  const [quality, setQuality] = createSignal<ScreenShareQuality>("medium");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [note, setNote] = createSignal<string | null>(null);

  /** Closing while a start runs aborts it (no share after Cancel). */
  const close = () => {
    if (loading()) void cancelMediaStart();
    props.onClose();
  };

  const handleStart = async () => {
    setError(null);
    setNote(null);
    setLoading(true);
    try {
      const result = await startScreenShare(quality(), props.sourceId);
      if (result.ok) {
        props.onClose();
      } else if (result.error) {
        setError(result.error);
      } else if (result.note) {
        setNote(result.note);
      } else {
        // Cancelled in the browser's own picker: nothing to say.
        props.onClose();
      }
    } finally {
      setLoading(false);
    }
  };

  return (
    <DialogShell
      title={t("voice.screenShare.dialogTitle")}
      onClose={close}
      testId="screen-share-quality"
    >
      <fieldset class="space-y-2">
        <legend class="text-sm text-text-secondary mb-2">
          {t("voice.screenShare.chooseQuality")}
        </legend>
        <For each={TIERS}>
          {(tier) => (
            <label
              class="flex items-start gap-3 p-3 rounded-lg cursor-pointer transition-colors border"
              classList={{
                "bg-accent-primary/20 border-accent-primary": quality() === tier.value,
                "border-white/10 hover:bg-white/5": quality() !== tier.value,
              }}
            >
              <input
                type="radio"
                name="screen-share-quality"
                value={tier.value}
                checked={quality() === tier.value}
                onChange={() => setQuality(tier.value)}
                class="mt-1 w-4 h-4"
              />
              <span class="flex-1">
                <span class="block text-sm font-medium text-text-primary">{t(tier.label)}</span>
                <span class="block text-xs text-text-secondary">{t(tier.hint)}</span>
              </span>
            </label>
          )}
        </For>
      </fieldset>

      <Show when={props.systemPicker}>
        <p class="text-sm text-text-secondary">{t("voice.screenShare.portalHint")}</p>
      </Show>
      <Show when={note()}>
        <p role="status" class="text-sm text-text-primary">
          {note()}
        </p>
      </Show>
      <Show when={error()}>
        <p
          role="alert"
          class="text-sm rounded-lg p-2"
          style={{
            "background-color": "var(--color-error-bg)",
            border: "1px solid var(--color-error-border)",
            color: "var(--color-error-text)",
          }}
        >
          {error()}
        </p>
      </Show>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          onClick={close}
          class="px-4 py-2 text-sm text-text-secondary hover:text-text-primary transition-colors"
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          data-testid="screen-share-start"
          onClick={handleStart}
          disabled={loading()}
          aria-busy={loading()}
          class="px-4 py-2 text-sm bg-accent-primary text-on-accent rounded-lg hover:bg-accent-primary/90 transition-colors state-disabled"
        >
          {loading() ? t("voice.screenShare.starting") : t("voice.screenShare.startButton")}
        </button>
      </div>
    </DialogShell>
  );
};

export default ScreenShareQualityPicker;
