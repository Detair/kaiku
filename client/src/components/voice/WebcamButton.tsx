import { Component, Show, createSignal, onCleanup } from "solid-js";
import { Camera, CameraOff } from "lucide-solid";
import { voiceState, startWebcam, stopWebcam } from "@/stores/voice";
import { t } from "@/i18n";

/**
 * Webcam toggle button for voice controls.
 */
const WebcamButton: Component = () => {
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let errorTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(errorTimer));

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
          if (result.error) setError(result.error);
          clearTimeout(errorTimer);
          errorTimer = setTimeout(() => setError(null), 3000);
        }
      } finally {
        setLoading(false);
      }
    }
  };

  // Starting needs a connected call; switching the camera off never waits
  // for one (it keeps capturing while the call reconnects).
  const disabled = () =>
    loading() || (!voiceState.webcamActive && voiceState.state !== "connected");

  return (
    <div class="relative">
      <button
        type="button"
        aria-label={voiceState.webcamActive ? t("voice.webcam.stop") : t("voice.webcam.start")}
        onClick={handleClick}
        disabled={disabled()}
        class={`p-2 rounded-full transition-colors ${
          voiceState.webcamActive
            ? "bg-success/20 text-success hover:bg-danger/20 hover:text-danger"
            : "bg-background-secondary text-text-secondary hover:bg-background-primary hover:text-text-primary"
        } ${loading() ? "opacity-50 cursor-wait" : ""} state-disabled`}
        title={voiceState.webcamActive ? t("voice.webcam.stop") : t("voice.webcam.start")}
      >
        {voiceState.webcamActive ? (
          <CameraOff class="w-5 h-5" />
        ) : (
          <Camera class="w-5 h-5" />
        )}
      </button>

      <Show when={error()}>
        <div
          role="alert"
          class="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 px-2 py-1 bg-danger text-white text-xs rounded whitespace-nowrap"
        >
          {error()}
        </div>
      </Show>
    </div>
  );
};

export default WebcamButton;
