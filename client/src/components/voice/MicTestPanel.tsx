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
import { t } from "@/i18n";
import { canSelectOutputDevice } from "@/lib/browserCapabilities";
import { voiceErrorText } from "@/lib/webrtc/voiceErrorText";

// Errors raised by the panel itself get their own types so the message is
// translated at render time, while adapter errors keep their own text.
type PanelError = VoiceError | { type: "init_failed" } | { type: "play_failed" };

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
  const [error, setError] = createSignal<PanelError | null>(null);
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
      setError({ type: "init_failed" });
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
      setError({ type: "play_failed" });
    }
  };

  const getErrorMessage = (err: PanelError): string => {
    switch (err.type) {
      case "init_failed":
        return t("voice.micTest.error.initFailed");
      case "play_failed":
        return t("voice.micTest.error.playFailed");
      default:
        return voiceErrorText(err);
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
          {t("voice.micTest.inputDevice")}
        </label>
        <select
          id={inputId}
          value={selectedInput()}
          onChange={(e) => setSelectedInput(e.target.value)}
          disabled={isTesting()}
          class="w-full px-3 py-2 bg-surface-layer2 border border-white/10 rounded-lg text-text-primary focus:outline-none focus:ring-2 focus:ring-accent-primary/50 disabled:opacity-50 text-sm"
        >
          <For each={inputDevices()}>
            {(device, i) => (
              <option value={device.deviceId}>
                {device.label || t("voice.audio.microphoneFallback", { id: i() + 1 })}
              </option>
            )}
          </For>
        </select>
      </div>

      <Show when={canSelectOutputDevice()}>
      <div>
        <label
          for={outputId}
          class="block text-sm font-medium text-text-secondary mb-1"
        >
          {t("voice.micTest.outputDevice")}
        </label>
        <select
          id={outputId}
          value={selectedOutput()}
          onChange={(e) => setSelectedOutput(e.target.value)}
          class="w-full px-3 py-2 bg-surface-layer2 border border-white/10 rounded-lg text-text-primary focus:outline-none focus:ring-2 focus:ring-accent-primary/50 text-sm"
        >
          <For each={outputDevices()}>
            {(device, i) => (
              <option value={device.deviceId}>
                {device.label || t("voice.audio.speakerFallback", { id: i() + 1 })}
              </option>
            )}
          </For>
        </select>
      </div>
      </Show>

      {/* Level Meter — empty track that fills left-to-right with the current
          mic level (like Discord/TeamSpeak). The green→yellow→red gradient is
          anchored to the full track, so a given position always maps to the
          same colour (right edge = loud/clipping) and is revealed by a cover
          overlay shrinking from the right as the level rises.
          NB: the cover MUST use an opaque colour — the `bg-surface-layer2`
          utility does not generate under this UnoCSS setup, so use the
          rgb-var escape hatch (the same workaround as the `input-field`
          shortcut). Without it the cover is transparent and the bar reads
          full at every level. */}
      <div>
        <div class="relative h-2 rounded-full overflow-hidden bg-[rgb(var(--color-surface-layer2-rgb))]">
          {/* Full-width gradient, position-anchored */}
          <div class="absolute inset-0 bg-gradient-to-r from-green-500 via-yellow-500 to-red-500" />
          {/* Cover overlay — hides the gradient beyond the current level */}
          <div
            class="absolute top-0 right-0 h-full bg-[rgb(var(--color-surface-layer2-rgb))] transition-all duration-100"
            style={{ width: `${100 - micLevel()}%` }}
          />
        </div>
        <div class="text-xs text-text-secondary mt-1 text-center">
          {t("voice.micTest.level", { level: micLevel() })}
        </div>
      </div>

      {/* Controls */}
      <div class="flex gap-2">
        <button
          onClick={playTestSound}
          class="flex-1 px-3 py-2 bg-surface-layer2 hover:bg-surface-highlight text-text-primary rounded-lg transition-colors text-sm"
        >
          {t("voice.micTest.playSound")}
        </button>
        <Show
          when={!isTesting()}
          fallback={
            <button
              onClick={stopTest}
              class="flex-1 px-3 py-2 bg-red-600 hover:bg-red-700 text-white rounded-lg transition-colors text-sm"
            >
              {t("voice.micTest.stop")}
            </button>
          }
        >
          <button
            onClick={startTest}
            class="flex-1 px-3 py-2 bg-accent-primary hover:bg-primary-hover text-on-accent rounded-lg transition-colors text-sm"
          >
            {t("voice.micTest.start")}
          </button>
        </Show>
      </div>

      {/* Status Messages */}
      <Show when={error()}>
        <div
          class="p-3 rounded-lg text-sm"
          style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
        >
          {getErrorMessage(error()!)}
        </div>
      </Show>

      <Show when={testPassed()}>
        <div class="p-3 bg-green-900/50 border border-green-700 rounded-lg text-green-200 text-sm">
          {t("voice.micTest.working")}
        </div>
      </Show>

      {/* Instructions */}
      <Show when={!props.compact}>
        <div class="text-xs text-text-secondary space-y-1">
          <p>{t("voice.micTest.step1")}</p>
          <p>{t("voice.micTest.step2")}</p>
          <p>{t("voice.micTest.step3")}</p>
          <p>{t("voice.micTest.step4")}</p>
        </div>
      </Show>
    </div>
  );
}

export default MicTestPanel;
