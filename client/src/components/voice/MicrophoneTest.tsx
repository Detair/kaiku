/**
 * Microphone Test Component
 *
 * Allows users to test their microphone and speaker setup before joining voice.
 */

import { createSignal, onMount, onCleanup, Show, For } from "solid-js";
import { createVoiceAdapter, type AudioDevice, type VoiceError } from "@/lib/webrtc";

interface Props {
  onClose: () => void;
}

function MicrophoneTest(props: Props) {
  const [inputDevices, setInputDevices] = createSignal<AudioDevice[]>([]);
  const [outputDevices, setOutputDevices] = createSignal<AudioDevice[]>([]);
  const [selectedInput, setSelectedInput] = createSignal<string>("");
  const [selectedOutput, setSelectedOutput] = createSignal<string>("");
  const [micLevel, setMicLevel] = createSignal(0);
  const [isTesting, setIsTesting] = createSignal(false);
  const [error, setError] = createSignal<VoiceError | null>(null);
  const [testPassed, setTestPassed] = createSignal(false);

  let levelInterval: number | undefined;
  let adapter: Awaited<ReturnType<typeof createVoiceAdapter>> | null = null;

  // Load devices on mount
  onMount(async () => {
    try {
      adapter = await createVoiceAdapter();
      const result = await adapter.getAudioDevices();
      if (result.ok) {
        setInputDevices(result.value.inputs);
        setOutputDevices(result.value.outputs);
        // Select defaults
        const defaultInput = result.value.inputs.find((d) => d.isDefault);
        const defaultOutput = result.value.outputs.find((d) => d.isDefault);
        if (defaultInput) setSelectedInput(defaultInput.deviceId);
        if (defaultOutput) setSelectedOutput(defaultOutput.deviceId);
      } else {
        setError(result.error);
      }
    } catch (err) {
      console.error("Failed to initialize mic test:", err);
    }
  });

  // Cleanup on unmount
  onCleanup(async () => {
    if (levelInterval) clearInterval(levelInterval);
    if (adapter) await adapter.stopMicTest();
  });

  const startTest = async () => {
    if (!adapter) return;
    setError(null);
    setTestPassed(false);

    const result = await adapter.startMicTest(selectedInput() || undefined);
    if (!result.ok) {
      setError(result.error);
      return;
    }

    setIsTesting(true);

    // Poll mic level
    levelInterval = window.setInterval(() => {
      const level = adapter!.getMicTestLevel();
      setMicLevel(level);

      // Auto-detect if mic is working (level > 20 for 500ms)
      if (level > 20) {
        setTestPassed(true);
      }
    }, 50);
  };

  const stopTest = async () => {
    if (levelInterval) clearInterval(levelInterval);
    if (adapter) await adapter.stopMicTest();
    setIsTesting(false);
    setMicLevel(0);
  };

  const playTestSound = () => {
    // Play a short test tone through selected output
    const ctx = new AudioContext();
    const oscillator = ctx.createOscillator();
    const gainNode = ctx.createGain();

    oscillator.frequency.value = 440; // A4 note
    gainNode.gain.value = 0.3; // Reduce volume

    oscillator.connect(gainNode);
    gainNode.connect(ctx.destination);

    oscillator.start();
    gainNode.gain.exponentialRampToValueAtTime(0.01, ctx.currentTime + 0.5);

    setTimeout(() => {
      oscillator.stop();
      ctx.close();
    }, 500);
  };

  const getErrorMessage = (error: VoiceError): string => {
    switch (error.type) {
      case "permission_denied":
        return "Microphone access denied. Please allow microphone in browser settings.";
      case "device_not_found":
        return "No microphone found. Please connect a microphone.";
      case "device_in_use":
        return "Microphone is being used by another app.";
      case "ice_failed":
      case "unknown":
        return error.message;
      case "server_rejected":
        return `Server rejected: ${error.message} (${error.code})`;
      case "connection_failed":
        return `Connection failed: ${error.reason}`;
      case "timeout":
        return `Timeout during ${error.operation}`;
      case "already_connected":
        return `Already connected to channel ${error.channelId}`;
      case "not_connected":
        return "Not connected to voice channel";
    }
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div class="bg-gray-800 rounded-lg shadow-xl max-w-md w-full mx-4">
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-gray-700">
          <h3 class="text-lg font-semibold text-white">🎤 Microphone Test</h3>
          <button
            onClick={props.onClose}
            class="text-gray-400 hover:text-white transition-colors"
          >
            ×
          </button>
        </div>

        {/* Content */}
        <div class="p-4 space-y-4">
          {/* Device Selection */}
          <div>
            <label class="block text-sm font-medium text-gray-300 mb-1">
              Input Device:
            </label>
            <select
              value={selectedInput()}
              onChange={(e) => setSelectedInput(e.target.value)}
              disabled={isTesting()}
              class="w-full px-3 py-2 bg-gray-700 border border-gray-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-indigo-500 disabled:opacity-50"
            >
              <For each={inputDevices()}>
                {(device) => (
                  <option value={device.deviceId}>{device.label}</option>
                )}
              </For>
            </select>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-300 mb-1">
              Output Device:
            </label>
            <select
              value={selectedOutput()}
              onChange={(e) => setSelectedOutput(e.target.value)}
              class="w-full px-3 py-2 bg-gray-700 border border-gray-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-indigo-500"
            >
              <For each={outputDevices()}>
                {(device) => (
                  <option value={device.deviceId}>{device.label}</option>
                )}
              </For>
            </select>
          </div>

          {/* Level Meter */}
          <div>
            <div class="h-2 bg-gray-700 rounded-full overflow-hidden">
              <div
                class="h-full bg-gradient-to-r from-green-500 via-yellow-500 to-red-500 transition-all duration-100"
                style={{ width: `${micLevel()}%` }}
              />
            </div>
            <div class="text-sm text-gray-400 mt-1 text-center">
              Mic Level: {micLevel()}%
            </div>
          </div>

          {/* Controls */}
          <div class="flex gap-2">
            <button
              onClick={playTestSound}
              class="flex-1 px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg transition-colors"
            >
              🔊 Play Test Sound
            </button>
            <Show
              when={!isTesting()}
              fallback={
                <button
                  onClick={stopTest}
                  class="flex-1 px-4 py-2 bg-red-600 hover:bg-red-700 text-white rounded-lg transition-colors"
                >
                  ⏹ Stop Test
                </button>
              }
            >
              <button
                onClick={startTest}
                class="flex-1 px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white rounded-lg transition-colors"
              >
                🎤 Start Test
              </button>
            </Show>
          </div>

          {/* Status Messages */}
          <Show when={error()}>
            <div class="p-3 rounded-lg text-sm" style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)">
              {getErrorMessage(error()!)}
            </div>
          </Show>

          <Show when={testPassed()}>
            <div class="p-3 bg-green-900/50 border border-green-700 rounded-lg text-green-200 text-sm">
              ✓ Microphone is working!
            </div>
          </Show>

          {/* Instructions */}
          <div class="text-xs text-gray-400 space-y-1">
            <p>1. Select your microphone and speakers</p>
            <p>2. Click "Start Test" and speak into your microphone</p>
            <p>3. Watch the level meter respond to your voice</p>
            <p>4. Click "Play Test Sound" to test your speakers</p>
          </div>
        </div>
      </div>
    </div>
  );
}

export default MicrophoneTest;
