/**
 * AudioDeviceSettings - Audio Input/Output Device Selection Modal
 *
 * Features:
 * - Device enumeration for microphones and speakers
 * - Microphone test with real-time volume indicator and color-coded guidance
 * - Speaker test (440Hz tone for 1 second)
 * - Loading states during device enumeration
 * - User-friendly error messages (VoiceError type discrimination)
 * - VoiceAdapter instance caching for performance
 *
 * Accessed via VoiceIsland settings button when in voice.
 */

import {
  Component,
  createSignal,
  onMount,
  onCleanup,
  Show,
  For,
  createEffect,
} from "solid-js";
import { Portal } from "solid-js/web";
import { X, Mic, Headphones, Loader2 } from "lucide-solid";
import { createVoiceAdapter } from "@/lib/webrtc";
import type { AudioDeviceList, VoiceError, VoiceAdapter } from "@/lib/webrtc";
import { setSpeaking } from "@/stores/voice";
import { showToast } from "@/components/ui/Toast";

interface AudioDeviceSettingsProps {
  onClose: () => void;
  /** Parent position for smart positioning */
  parentPosition?: { x: number; y: number };
}

/**
 * Convert VoiceError to user-friendly message
 */
function getDeviceErrorMessage(error: VoiceError): string {
  switch (error.type) {
    case "permission_denied":
      return "Microphone access denied. Please grant permission in your browser settings.";
    case "device_not_found":
      return "Audio device not found. Please check your device connections.";
    case "device_in_use":
      return "Device is already in use by another application.";
    case "unknown":
      return `Audio device error: ${error.message}`;
    default:
      return `Failed to access audio device: ${error.type}`;
  }
}

const AudioDeviceSettings: Component<AudioDeviceSettingsProps> = (props) => {
  const [devices, setDevices] = createSignal<AudioDeviceList>({
    inputs: [],
    outputs: [],
  });
  const [selectedInput, setSelectedInput] = createSignal<string>("");
  const [selectedOutput, setSelectedOutput] = createSignal<string>("");
  const [isTesting, setIsTesting] = createSignal(false);
  const [testLevel, setTestLevel] = createSignal(0);
  const [isTestingSpeaker, setIsTestingSpeaker] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(true);
  const [error, setError] = createSignal<string>("");
  const [noiseSuppression, setNoiseSuppression] = createSignal(true);

  // Cached voice adapter instance
  const [adapter, setAdapter] = createSignal<VoiceAdapter | null>(null);

  // Draggable modal state
  const [modalPosition, setModalPosition] = createSignal<{
    x: number;
    y: number;
  } | null>(null);
  const [isDragging, setIsDragging] = createSignal(false);
  const [dragOffset, setDragOffset] = createSignal({ x: 0, y: 0 });
  let modalRef: HTMLDivElement | undefined;
  let dragRafId: number | null = null;

  let testInterval: number | undefined;
  let audioContext: AudioContext | undefined;
  let oscillator: OscillatorNode | undefined;

  // Load available devices on mount
  onMount(async () => {
    setIsLoading(true);
    try {
      // Cache the voice adapter instance
      const voiceAdapter = await createVoiceAdapter();
      setAdapter(voiceAdapter);

      const result = await voiceAdapter.getAudioDevices();

      if (!result.ok) {
        setError(getDeviceErrorMessage(result.error));
        console.error("Device enumeration failed:", result.error);
        setIsLoading(false);
        return;
      }

      setDevices(result.value);
      setNoiseSuppression(voiceAdapter.isNoiseSuppressionEnabled());

      // Set default selections (first device or default device)
      if (result.value.inputs.length > 0) {
        const defaultInput =
          result.value.inputs.find((d) => d.isDefault) ||
          result.value.inputs[0];
        setSelectedInput(defaultInput.deviceId);
      }

      if (result.value.outputs.length > 0) {
        const defaultOutput =
          result.value.outputs.find((d) => d.isDefault) ||
          result.value.outputs[0];
        setSelectedOutput(defaultOutput.deviceId);
      }

      setIsLoading(false);
    } catch (err) {
      console.error("Failed to load devices:", err);
      setError("Failed to load audio devices");
      setIsLoading(false);
    }
  });

  // Toggle Noise Suppression
  const toggleNoiseSuppression = async () => {
    const newVal = !noiseSuppression();
    setNoiseSuppression(newVal);
    const voiceAdapter = adapter();
    if (voiceAdapter) {
      await voiceAdapter.setNoiseSuppression(newVal);
    }
  };

  // ESC key handler
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    }
  };

  // Drag handlers for modal
  const handleModalMouseDown = (e: MouseEvent) => {
    // Only drag from header area
    if (!(e.target as HTMLElement).closest(".modal-header")) return;
    if ((e.target as HTMLElement).closest("button")) return;

    setIsDragging(true);
    const currentPos = modalPosition() || getInitialModalPosition();
    setDragOffset({
      x: e.clientX - currentPos.x,
      y: e.clientY - currentPos.y,
    });
    e.preventDefault();
  };

  const handleModalMouseMove = (e: MouseEvent) => {
    if (!isDragging() || !modalRef) return;

    if (dragRafId !== null) {
      cancelAnimationFrame(dragRafId);
    }

    dragRafId = requestAnimationFrame(() => {
      const newX = e.clientX - dragOffset().x;
      const newY = e.clientY - dragOffset().y;

      const rect = modalRef!.getBoundingClientRect();
      const maxX = window.innerWidth - rect.width;
      const maxY = window.innerHeight - rect.height;

      setModalPosition({
        x: Math.max(0, Math.min(newX, maxX)),
        y: Math.max(0, Math.min(newY, maxY)),
      });
    });
  };

  const handleModalMouseUp = () => {
    setIsDragging(false);
    if (dragRafId !== null) {
      cancelAnimationFrame(dragRafId);
      dragRafId = null;
    }
  };

  // Register global listeners
  onMount(() => {
    window.addEventListener("keydown", handleKeyDown);
  });

  // Cleanup on unmount
  onCleanup(async () => {
    window.removeEventListener("keydown", handleKeyDown);
    window.removeEventListener("mousemove", handleModalMouseMove);
    window.removeEventListener("mouseup", handleModalMouseUp);
    if (dragRafId !== null) {
      cancelAnimationFrame(dragRafId);
    }
    if (isTesting()) {
      await stopMicTest();
    }
    if (isTestingSpeaker()) {
      stopSpeakerTest();
    }
  });

  // Dragging effect
  createEffect(() => {
    if (isDragging()) {
      window.addEventListener("mousemove", handleModalMouseMove);
      window.addEventListener("mouseup", handleModalMouseUp);
    } else {
      window.removeEventListener("mousemove", handleModalMouseMove);
      window.removeEventListener("mouseup", handleModalMouseUp);
    }
  });

  // Handle input device change
  const handleInputChange = async (deviceId: string) => {
    setSelectedInput(deviceId);
    setError("");

    const voiceAdapter = adapter();
    if (!voiceAdapter) return;

    try {
      const result = await voiceAdapter.setInputDevice(deviceId);

      if (!result.ok) {
        showToast({
          type: "error",
          title: getDeviceErrorMessage(result.error),
          duration: 8000,
        });
        console.error("Set input device failed:", result.error);
      }
    } catch (err) {
      console.error("Failed to set input device:", err);
      showToast({
        type: "error",
        title: "Unexpected error while changing input device",
        duration: 8000,
      });
    }
  };

  // Handle output device change
  const handleOutputChange = async (deviceId: string) => {
    setSelectedOutput(deviceId);
    setError("");

    const voiceAdapter = adapter();
    if (!voiceAdapter) return;

    try {
      const result = await voiceAdapter.setOutputDevice(deviceId);

      if (!result.ok) {
        showToast({
          type: "error",
          title: getDeviceErrorMessage(result.error),
          duration: 8000,
        });
        console.error("Set output device failed:", result.error);
      }
    } catch (err) {
      console.error("Failed to set output device:", err);
      showToast({
        type: "error",
        title: "Unexpected error while changing output device",
        duration: 8000,
      });
    }
  };

  // Start microphone test
  const startMicTest = async () => {
    setError("");
    const voiceAdapter = adapter();
    if (!voiceAdapter) return;

    try {
      const result = await voiceAdapter.startMicTest(selectedInput());

      if (!result.ok) {
        showToast({
          type: "error",
          title: getDeviceErrorMessage(result.error),
          duration: 8000,
        });
        console.error("Mic test start failed:", result.error);
        return;
      }

      setIsTesting(true);

      // Poll for mic level every 50ms
      testInterval = window.setInterval(() => {
        const level = voiceAdapter.getMicTestLevel();
        setTestLevel(level);
        // Temporary: Trigger speaking indicator when mic level is above threshold
        // @phase1 - Remove when backend VAD is implemented
        setSpeaking(level > 20);
      }, 50);
    } catch (err) {
      console.error("Failed to start mic test:", err);
      showToast({
        type: "error",
        title: "Unexpected error while starting microphone test",
        duration: 8000,
      });
    }
  };

  // Stop microphone test
  const stopMicTest = async () => {
    const voiceAdapter = adapter();
    if (!voiceAdapter) return;

    try {
      if (testInterval !== undefined) {
        clearInterval(testInterval);
        testInterval = undefined;
      }

      await voiceAdapter.stopMicTest();
      setIsTesting(false);
      setTestLevel(0);
      // Reset speaking indicator
      setSpeaking(false);
    } catch (err) {
      console.error("Failed to stop mic test:", err);
    }
  };

  // Test speaker by playing a 440Hz tone (A4 note) for 1 second
  const testSpeaker = () => {
    setError("");
    try {
      // Create audio context if not exists
      if (!audioContext) {
        audioContext = new AudioContext();
      }

      // Create oscillator for test tone
      oscillator = audioContext.createOscillator();
      const gainNode = audioContext.createGain();

      oscillator.connect(gainNode);
      gainNode.connect(audioContext.destination);

      // 440Hz tone (musical note A4)
      oscillator.frequency.value = 440;
      oscillator.type = "sine";

      // Fade in/out for smoother sound
      gainNode.gain.setValueAtTime(0, audioContext.currentTime);
      gainNode.gain.linearRampToValueAtTime(
        0.3,
        audioContext.currentTime + 0.1,
      );
      gainNode.gain.linearRampToValueAtTime(
        0.3,
        audioContext.currentTime + 0.9,
      );
      gainNode.gain.linearRampToValueAtTime(0, audioContext.currentTime + 1);

      oscillator.start(audioContext.currentTime);
      oscillator.stop(audioContext.currentTime + 1);

      setIsTestingSpeaker(true);

      // Reset button state after 1 second
      setTimeout(() => {
        setIsTestingSpeaker(false);
        oscillator = undefined;
      }, 1000);
    } catch (err) {
      console.error("Failed to test speaker:", err);
      showToast({
        type: "error",
        title: "Failed to play test sound. Check your browser permissions.",
        duration: 8000,
      });
      setIsTestingSpeaker(false);
    }
  };

  // Stop speaker test (if playing)
  const stopSpeakerTest = () => {
    if (oscillator) {
      try {
        oscillator.stop();
        oscillator = undefined;
      } catch (err) {
        // Oscillator may already be stopped
      }
    }
    if (audioContext) {
      audioContext.close();
      audioContext = undefined;
    }
    setIsTestingSpeaker(false);
  };

  // Calculate initial modal position relative to VoiceIsland
  const getInitialModalPosition = (): { x: number; y: number } => {
    const modalWidth = 448;
    const modalHeight = 600; // Approximate height

    // Use window inner dimensions for browser viewport
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    if (!props.parentPosition) {
      // Center on viewport
      const x = Math.max(16, (viewportWidth - modalWidth) / 2);
      const y = Math.max(16, (viewportHeight - modalHeight) / 2);
      return { x, y };
    }

    const parentY = props.parentPosition.y;
    const spacing = 10;

    // Center the modal horizontally in the viewport
    const modalLeft = (viewportWidth - modalWidth) / 2;

    // Ensure modal stays within horizontal bounds
    const leftPosition = Math.max(
      16, // Min margin from left
      Math.min(
        modalLeft,
        viewportWidth - modalWidth - 16, // Max margin from right
      ),
    );

    // Vertical: Position below VoiceIsland
    const voiceIslandHeight = 70;
    const topPosition = parentY + voiceIslandHeight + spacing;

    // Ensure modal stays within vertical bounds
    const finalTop = Math.max(
      16, // Min margin from top
      Math.min(
        topPosition,
        viewportHeight - modalHeight - 16, // Max margin from bottom
      ),
    );

    return {
      x: leftPosition,
      y: finalTop,
    };
  };

  // Use dragged position or initial position
  const finalPosition = () => modalPosition() || getInitialModalPosition();

  return (
    <Portal mount={document.body}>
      {/* Backdrop with semi-transparent background - click to close */}
      <div
        class="fixed inset-0 bg-black/70 backdrop-blur-sm"
        style={{ "z-index": "9998" }}
        onClick={props.onClose}
      />
      {/* Modal - separate from backdrop for correct click handling */}
      <div
        ref={modalRef}
        class="bg-surface-base rounded-xl shadow-2xl w-full max-w-md border border-accent-primary/20 max-h-[80vh] overflow-hidden flex flex-col"
        classList={{
          "cursor-grabbing": isDragging(),
        }}
        style={{
          position: "fixed",
          left: `${finalPosition().x}px`,
          top: `${finalPosition().y}px`,
          "z-index": "9999",
        }}
      >
        {/* Header - Draggable */}
        <div
          class="modal-header flex items-center justify-between px-6 py-4 border-b border-white/10 cursor-move select-none"
          onMouseDown={handleModalMouseDown}
        >
          <h2 class="text-lg font-semibold text-text-primary">
            Audio Settings
          </h2>
          <button
            onClick={props.onClose}
            class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-all duration-200"
            title="Close"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Content - Scrollable */}
        <div class="px-6 py-4 space-y-6 overflow-y-auto flex-1">
          {/* Loading state */}
          <Show when={isLoading()}>
            <div class="flex flex-col items-center justify-center py-8">
              <Loader2 class="w-8 h-8 text-accent-primary animate-spin mb-3" />
              <p class="text-sm text-text-secondary">
                Loading audio devices...
              </p>
            </div>
          </Show>

          {/* Error message */}
          <Show when={error() && !isLoading()}>
            <div
              class="px-4 py-3 rounded-xl"
              style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border)"
            >
              <p class="text-sm" style="color: var(--color-error-text)">
                {error()}
              </p>
            </div>
          </Show>

          {/* Device selectors - only show when loaded */}
          <Show when={!isLoading()}>
            {/* Input Device */}
            <div>
              <label class="flex items-center gap-2 text-sm font-medium text-text-primary mb-2">
                <div class="w-5 h-5 text-text-secondary">
                  <Mic />
                </div>
                Input Device
              </label>
              <select
                value={selectedInput()}
                onChange={(e) => handleInputChange(e.currentTarget.value)}
                class="w-full px-3 py-2.5 bg-surface-base rounded-xl text-sm text-text-primary border border-white/10 outline-none focus:ring-2 focus:ring-accent-primary/30 transition-all"
              >
                <For each={devices().inputs}>
                  {(device) => (
                    <option value={device.deviceId}>
                      {device.label ||
                        `Microphone ${device.deviceId.slice(0, 8)}`}
                      {device.isDefault ? " (Default)" : ""}
                    </option>
                  )}
                </For>
              </select>

              {/* Mic Test */}
              <div class="mt-3 space-y-2">
                <button
                  onClick={() => (isTesting() ? stopMicTest() : startMicTest())}
                  class="px-4 py-2 rounded-xl font-medium text-sm transition-all duration-200"
                  classList={{
                    "bg-accent-primary text-white hover:bg-accent-primary/90":
                      !isTesting(),
                    "bg-accent-danger text-white hover:bg-accent-danger/90":
                      isTesting(),
                  }}
                >
                  {isTesting() ? "Stop Test" : "Test Microphone"}
                </button>

                {/* Volume indicator */}
                <Show when={isTesting()}>
                  <div class="w-full h-2 bg-surface-base rounded-full overflow-hidden">
                    <div
                      class="h-full transition-all duration-75"
                      classList={{
                        "bg-amber-500": testLevel() < 20,
                        "bg-accent-primary":
                          testLevel() >= 20 && testLevel() <= 70,
                        "bg-accent-danger": testLevel() > 70,
                      }}
                      style={{ width: `${testLevel()}%` }}
                    />
                  </div>
                  <p
                    class="text-xs mt-1.5"
                    classList={{
                      "text-amber-500": testLevel() < 20 && testLevel() > 0,
                      "text-accent-primary":
                        testLevel() >= 20 && testLevel() <= 70,
                      "text-accent-danger": testLevel() > 70,
                      "text-text-secondary": testLevel() === 0,
                    }}
                  >
                    {testLevel() === 0
                      ? "Speak into your microphone to test"
                      : testLevel() < 20
                        ? "Too quiet - speak louder or move mic closer"
                        : testLevel() > 70
                          ? "Too loud - may distort, reduce volume or move mic away"
                          : "Good level - keep speaking like this"}
                  </p>
                </Show>
              </div>

              {/* Noise Suppression Toggle */}
              <div class="mt-4 flex items-center justify-between">
                <div>
                  <div class="text-sm font-medium text-text-primary">
                    Noise Suppression
                  </div>
                  <div class="text-xs text-text-secondary">
                    Reduces background noise
                  </div>
                </div>
                <button
                  onClick={toggleNoiseSuppression}
                  class={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-accent-primary focus:ring-offset-2 focus:ring-offset-surface-base ${
                    noiseSuppression()
                      ? "bg-accent-primary"
                      : "bg-surface-highlight"
                  }`}
                >
                  <span
                    class={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                      noiseSuppression() ? "translate-x-6" : "translate-x-1"
                    }`}
                  />
                </button>
              </div>
            </div>

            {/* Output Device */}
            <div>
              <label class="flex items-center gap-2 text-sm font-medium text-text-primary mb-2">
                <div class="w-5 h-5 text-text-secondary">
                  <Headphones />
                </div>
                Output Device
              </label>
              <select
                value={selectedOutput()}
                onChange={(e) => handleOutputChange(e.currentTarget.value)}
                class="w-full px-3 py-2.5 bg-surface-base rounded-xl text-sm text-text-primary border border-white/10 outline-none focus:ring-2 focus:ring-accent-primary/30 transition-all"
              >
                <For each={devices().outputs}>
                  {(device) => (
                    <option value={device.deviceId}>
                      {device.label || `Speaker ${device.deviceId.slice(0, 8)}`}
                      {device.isDefault ? " (Default)" : ""}
                    </option>
                  )}
                </For>
              </select>

              {/* Speaker Test */}
              <div class="mt-3">
                <button
                  onClick={testSpeaker}
                  disabled={isTestingSpeaker()}
                  class="px-4 py-2 rounded-xl font-medium text-sm transition-all duration-200"
                  classList={{
                    "bg-accent-primary text-white hover:bg-accent-primary/90":
                      !isTestingSpeaker(),
                    "bg-accent-primary/50 text-white cursor-not-allowed":
                      isTestingSpeaker(),
                  }}
                >
                  {isTestingSpeaker() ? "Playing..." : "Test Speaker"}
                </button>
                <p class="text-xs text-text-secondary mt-2">
                  Plays a short test tone through your selected output device
                </p>
              </div>
            </div>
          </Show>
        </div>

        {/* Footer */}
        <div class="px-6 py-4 border-t border-white/10 flex justify-end">
          <button
            onClick={props.onClose}
            class="px-5 py-2.5 bg-accent-primary hover:bg-accent-primary/90 text-white rounded-xl font-medium text-sm transition-all duration-200"
          >
            Done
          </button>
        </div>
      </div>
    </Portal>
  );
};

export default AudioDeviceSettings;
