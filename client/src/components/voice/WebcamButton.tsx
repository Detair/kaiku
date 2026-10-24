import { Component, Show, createSignal } from "solid-js";
import { Camera, CameraOff } from "lucide-solid";
import { voiceState, startWebcam, stopWebcam } from "@/stores/voice";
import { t } from "@/i18n";

/**
 * Webcam toggle button for voice controls.
 */
const WebcamButton: Component = () => {
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleClick = async () => {
    setError(null);

    if (voiceState.webcamActive) {
      setLoading(true);
      try {
        await stopWebcam();
      } finally {
        setLoading(false);
      }
    } else {
      setLoading(true);
      try {
        const result = await startWebcam("medium");
        if (!result.ok) {
          setError(result.error || "Failed to start webcam");
          setTimeout(() => setError(null), 3000);
        }
      } finally {
        setLoading(false);
      }
    }
  };

  return (
    <div class="relative">
      <button
        type="button"
        aria-label={voiceState.webcamActive ? t("voice.webcamStop") : t("voice.webcamStart")}
        onClick={handleClick}
        disabled={voiceState.state !== "connected" || loading()}
        class={`p-2 rounded-full transition-colors ${
          voiceState.webcamActive
            ? "bg-success/20 text-success hover:bg-danger/20 hover:text-danger"
            : "bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary"
        } ${loading() ? "opacity-50 cursor-wait" : ""}`}
        title={voiceState.webcamActive ? "Stop Camera" : "Start Camera"}
      >
        {voiceState.webcamActive ? (
          <CameraOff class="w-5 h-5" />
        ) : (
          <Camera class="w-5 h-5" />
        )}
      </button>

      <Show when={error()}>
        <div class="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 px-2 py-1 bg-danger text-white text-xs rounded whitespace-nowrap">
          {error()}
        </div>
      </Show>
    </div>
  );
};

export default WebcamButton;
