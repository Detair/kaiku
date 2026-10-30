import { Component, createMemo, Show } from "solid-js";
import { MonitorUp, MonitorOff } from "lucide-solid";
import { voiceState } from "@/stores/voice";
import { currentUser } from "@/stores/auth";
import { t } from "@/i18n";
import { canShareScreen } from "@/lib/browserCapabilities";

/** Maximum concurrent screen shares per user. */
const MAX_SCREEN_SHARES = 3;

interface ScreenShareButtonProps {
  /** Show source picker first (native capture), then quality picker. */
  onShowSourcePicker?: () => void;
  onShowQualityPicker?: () => void;
}

/**
 * Screen share toggle button.
 *
 * In multi-stream mode the button always opens the source/quality picker to
 * start a new stream (up to `MAX_SCREEN_SHARES`). Stopping individual
 * streams is handled via the viewer UI.
 */
const ScreenShareButton: Component<ScreenShareButtonProps> = (props) => {
  /** Number of screen shares the local user currently owns. */
  const ownShareCount = createMemo(() => {
    const userId = currentUser()?.id;
    if (!userId) return 0;
    return voiceState.screenShares.filter((s) => s.user_id === userId).length;
  });

  const atLimit = createMemo(() => ownShareCount() >= MAX_SCREEN_SHARES);

  const handleClick = () => {
    if (atLimit()) return; // Disabled at limit — stop individual streams via viewer UI
    if (props.onShowSourcePicker) {
      props.onShowSourcePicker();
    } else {
      props.onShowQualityPicker?.();
    }
  };

  const buttonTitle = () => {
    if (atLimit()) return t("voice.screenShare.limit", { count: MAX_SCREEN_SHARES, max: MAX_SCREEN_SHARES });
    if (voiceState.screenSharing) return t("voice.screenShare.another", { count: ownShareCount(), max: MAX_SCREEN_SHARES });
    return t("voice.screenShare.start");
  };

  // Browsers without getDisplayMedia (most mobile ones) can't capture a
  // screen at all; hide the button instead of offering a dead action.
  return (
    <Show when={canShareScreen()}>
    <button
      type="button"
      aria-label={buttonTitle()}
      onClick={handleClick}
      disabled={voiceState.state !== "connected" || atLimit()}
      class={`p-2 rounded-full transition-colors ${
        voiceState.screenSharing
          ? "bg-accent-success/20 text-text-primary hover:bg-accent-success/30"
          : "bg-surface-layer2 text-text-secondary hover:bg-surface-highlight hover:text-text-primary"
      } ${atLimit() ? "opacity-50 cursor-not-allowed" : ""}`}
      title={buttonTitle()}
    >
      {atLimit() ? (
        <MonitorOff class="w-5 h-5" />
      ) : (
        <MonitorUp class="w-5 h-5" />
      )}
    </button>
    </Show>
  );
};

export default ScreenShareButton;
