import { Component, createSignal } from "solid-js";
import { MonitorUp, MonitorOff } from "lucide-solid";
import { voiceState, stopScreenShare } from "@/stores/voice";
import { showToast } from "@/components/ui/Toast";

interface ScreenShareButtonProps {
  /** Show source picker first (native capture), then quality picker. */
  onShowSourcePicker?: () => void;
  onShowQualityPicker?: () => void;
}

/**
 * Screen share toggle button.
 */
const ScreenShareButton: Component<ScreenShareButtonProps> = (props) => {
  const [loading, setLoading] = createSignal(false);

  const handleClick = async () => {
    if (voiceState.screenSharing) {
      // Stop sharing
      setLoading(true);
      try {
        await stopScreenShare();
      } catch (err) {
        console.error("Failed to stop screen share:", err);
        showToast({
          type: "error",
          title: "Could not stop screen share.",
          duration: 8000,
        });
      } finally {
        setLoading(false);
      }
    } else {
      // Show source picker first (native), falls back to quality picker (browser)
      if (props.onShowSourcePicker) {
        props.onShowSourcePicker();
      } else {
        props.onShowQualityPicker?.();
      }
    }
  };

  return (
    <button
      onClick={handleClick}
      disabled={voiceState.state !== "connected" || loading()}
      class={`p-2 rounded-full transition-colors ${
        voiceState.screenSharing
          ? "bg-success/20 text-success hover:bg-danger/20 hover:text-danger"
          : "bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary"
      } ${loading() ? "opacity-50 cursor-wait" : ""}`}
      title={voiceState.screenSharing ? "Stop Sharing" : "Share Screen"}
    >
      {voiceState.screenSharing ? (
        <MonitorOff class="w-5 h-5" />
      ) : (
        <MonitorUp class="w-5 h-5" />
      )}
    </button>
  );
};

export default ScreenShareButton;
