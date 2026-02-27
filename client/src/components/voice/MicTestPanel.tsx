/**
 * MicTestPanel - Core microphone/speaker test UI.
 *
 * Extracted from MicrophoneTest so it can be embedded inline
 * (e.g. in onboarding wizard) or wrapped in a modal.
 */

import {
  createSignal,
  createUniqueId,
  onMount,
  onCleanup,
  Show,
  For,
} from "solid-js";
import {
  createVoiceAdapter,
  type AudioDevice,
  type VoiceError,
} from "@/lib/webrtc";

interface MicTestPanelProps {
  /** Reduced spacing for embedding in compact layouts */
  compact?: boolean;
}

function MicTestPanel(props: MicTestPanelProps) {
  const inputId = createUniqueId();
  const outputId = createUniqueId();
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

  onMount(async () => {
    try {
      adapter = await createVoiceAdapter();
      const result = await adapter.getAudioDevices();
      if (result.ok) {
        setInputDevices(result.value.inputs);
        setOutputDevices(result.value.outputs);
        const defaultInput = result.value.inputs.find((d) => d.isDefault);
        const defaultOutput = result.value.outputs.find((d) => d.isDefault);
        if (defaultInput) setSelectedInput(defaultInput.deviceId);
        if (defaultOutput) setSelectedOutput(defaultOutput.deviceId);
      } else {
        setError(result.error);
      }
    } catch (err) {
      console.error("Failed to initialize mic test:", err);
      setError({
        type: "unknown",
        message: "Failed to initialize audio system.",
      } as VoiceError);
    }
  });

  onCleanup(() => {
    if (levelInterval) clearInterval(levelInterval);
    if (adapter) void adapter.stopMicTest(); // fire-and-forget; SolidJS doesn't await cleanup
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

    levelInterval = window.setInterval(() => {
      const level = adapter!.getMicTestLevel();
      setMicLevel(level);

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

  const playTestSound = async () => {
    try {
      const ctx = new AudioContext();

      // Route to selected output device if supported
      const devId = selectedOutput();
      if (devId && "setSinkId" in ctx) {
        try {
          await (
            ctx as AudioContext & { setSinkId(id: string): Promise<void> }
          ).setSinkId(devId);
        } catch (err) {
          console.warn(
            "Could not route test sound to selected output device:",
            err,
          );
        }
      }

      const oscillator = ctx.createOscillator();
      const gainNode = ctx.createGain();

      oscillator.frequency.value = 440;
      gainNode.gain.value = 0.3;

      oscillator.connect(gainNode);
      gainNode.connect(ctx.destination);

      oscillator.start();
      gainNode.gain.exponentialRampToValueAtTime(0.01, ctx.currentTime + 0.5);

      setTimeout(() => {
        oscillator.stop();
        void ctx.close();
      }, 500);
    } catch (err) {
      console.error("Failed to play test sound:", err);
      setError({
        type: "unknown",
        message: "Could not play test sound. Check your audio settings.",
      } as VoiceError);
    }
  };

  const getErrorMessage = (err: VoiceError): string => {
    switch (err.type) {
      case "permission_denied":
        return "Microphone access denied. Please allow microphone in browser settings.";
      case "device_not_found":
        return "No microphone found. Please connect a microphone.";
      case "device_in_use":
        return "Microphone is being used by another app.";
      case "ice_failed":
      case "cancelled":
      case "not_found":
      case "hardware_error":
      case "constraint_error":
      case "unknown":
        return err.message;
      case "server_rejected":
        return `Server rejected: ${err.message} (${err.code})`;
      case "connection_failed":
        return `Connection failed: ${err.reason}`;
      case "timeout":
        return `Timeout during ${err.operation}`;
      case "already_connected":
        return `Already connected to channel ${err.channelId}`;
      case "not_connected":
        return "Not connected to voice channel";
    }
  };

  const spacing = () => (props.compact ? "space-y-3" : "space-y-4");

  return (
    <div class={spacing()}>
      {/* Device Selection */}
      <div>
        <label
          for={inputId}
          class="block text-sm font-medium text-text-secondary mb-1"
        >
          Input Device:
        </label>
        <select
          id={inputId}
          value={selectedInput()}
          onChange={(e) => setSelectedInput(e.target.value)}
          disabled={isTesting()}
          class="w-full px-3 py-2 bg-surface-layer2 border border-white/10 rounded-lg text-text-primary focus:outline-none focus:ring-2 focus:ring-accent-primary/50 disabled:opacity-50 text-sm"
        >
          <For each={inputDevices()}>
            {(device) => (
              <option value={device.deviceId}>{device.label}</option>
            )}
          </For>
        </select>
      </div>

      <div>
        <label
          for={outputId}
          class="block text-sm font-medium text-text-secondary mb-1"
        >
          Output Device:
        </label>
        <select
          id={outputId}
          value={selectedOutput()}
          onChange={(e) => setSelectedOutput(e.target.value)}
          class="w-full px-3 py-2 bg-surface-layer2 border border-white/10 rounded-lg text-text-primary focus:outline-none focus:ring-2 focus:ring-accent-primary/50 text-sm"
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
        <div class="h-2 bg-surface-layer2 rounded-full overflow-hidden">
          <div
            class="h-full bg-gradient-to-r from-green-500 via-yellow-500 to-red-500 transition-all duration-100"
            style={{ width: `${micLevel()}%` }}
          />
        </div>
        <div class="text-xs text-text-secondary mt-1 text-center">
          Mic Level: {micLevel()}%
        </div>
      </div>

      {/* Controls */}
      <div class="flex gap-2">
        <button
          onClick={playTestSound}
          class="flex-1 px-3 py-2 bg-surface-layer2 hover:bg-surface-highlight text-text-primary rounded-lg transition-colors text-sm"
        >
          Play Test Sound
        </button>
        <Show
          when={!isTesting()}
          fallback={
            <button
              onClick={stopTest}
              class="flex-1 px-3 py-2 bg-red-600 hover:bg-red-700 text-white rounded-lg transition-colors text-sm"
            >
              Stop Test
            </button>
          }
        >
          <button
            onClick={startTest}
            class="flex-1 px-3 py-2 bg-accent-primary hover:bg-accent-hover text-white rounded-lg transition-colors text-sm"
          >
            Start Test
          </button>
        </Show>
      </div>

      {/* Status Messages */}
      <Show when={error()}>
        <div
          class="p-3 rounded-lg text-sm"
          style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)"
        >
          {getErrorMessage(error()!)}
        </div>
      </Show>

      <Show when={testPassed()}>
        <div class="p-3 bg-green-900/50 border border-green-700 rounded-lg text-green-200 text-sm">
          Microphone is working!
        </div>
      </Show>

      {/* Instructions */}
      <Show when={!props.compact}>
        <div class="text-xs text-text-secondary space-y-1">
          <p>1. Select your microphone and speakers</p>
          <p>2. Click "Start Test" and speak into your microphone</p>
          <p>3. Watch the level meter respond to your voice</p>
          <p>4. Click "Play Test Sound" to test your speakers</p>
        </div>
      </Show>
    </div>
  );
}

export default MicTestPanel;
