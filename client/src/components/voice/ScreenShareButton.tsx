import { Component, Show, createSignal } from "solid-js";
import { MonitorUp, MonitorOff } from "lucide-solid";
import { voiceState, stopScreenShare } from "@/stores/voice";
import { t } from "@/i18n";
import { canShareScreen } from "@/lib/browserCapabilities";

interface ScreenShareButtonProps {
  /** Show source picker first (native capture), then quality picker. */
  onShowSourcePicker?: () => void;
  onShowQualityPicker?: () => void;
}

/**
 * Screen share toggle: starts a share, and while sharing the same button
 * stops it (one share per person for 1.0). Reachable from every view that
 * shows the voice controls, by keyboard too.
 */
const ScreenShareButton: Component<ScreenShareButtonProps> = (props) => {
  const [stopping, setStopping] = createSignal(false);

  const handleClick = async () => {
    if (voiceState.screenSharing) {
      setStopping(true);
      try {
        await stopScreenShare();
      } finally {
        setStopping(false);
      }
      return;
    }
    if (props.onShowSourcePicker) {
      props.onShowSourcePicker();
    } else {
      props.onShowQualityPicker?.();
    }
  };

  const label = () =>
    voiceState.screenSharing ? t("voice.screenShare.stop") : t("voice.screenShare.start");

  // Browsers without getDisplayMedia (most mobile ones) can't capture a
  // screen at all; hide the button instead of offering a dead action.
  return (
    <Show when={canShareScreen()}>
      <button
        type="button"
        data-testid="screen-share-toggle"
        aria-label={label()}
        aria-pressed={voiceState.screenSharing}
        onClick={handleClick}
        disabled={voiceState.state !== "connected" || stopping()}
        class={`p-2 rounded-full transition-colors ${
          voiceState.screenSharing
            ? "bg-accent-danger/20 text-text-primary hover:bg-accent-danger/30"
            : "bg-surface-layer2 text-text-secondary hover:bg-surface-highlight hover:text-text-primary"
        } state-disabled-quiet`}
        title={label()}
      >
        {voiceState.screenSharing ? <MonitorOff class="w-5 h-5" /> : <MonitorUp class="w-5 h-5" />}
      </button>
    </Show>
  );
};

export default ScreenShareButton;
