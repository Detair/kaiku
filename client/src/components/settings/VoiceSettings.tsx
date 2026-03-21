import { Component, Show, createSignal, onCleanup } from "solid-js";
import { appSettings, updateAudioSetting, updateVoiceSetting, isSettingsLoading } from "@/stores/settings";
import { keyCodeToLabel } from "@/lib/pttManager";

const VoiceSettings: Component = () => {
    return (
        <div class="space-y-6">
            <div>
                <h3 class="text-lg font-semibold text-text-primary mb-1">Voice Settings</h3>
                <p class="text-sm text-text-secondary">
                    Configure how your voice is captured and processed.
                </p>
            </div>

            <Show when={!isSettingsLoading() && appSettings()} fallback={<p class="text-text-secondary">Loading...</p>}>
                {(settings) => (
                    <>
                        {/* Audio Processing */}
                        <div class="space-y-4">
                            <ToggleCard
                                label="Noise Suppression"
                                description="Filters out background noise like keyboard typing and fans"
                                checked={settings().audio.noise_suppression}
                                onChange={(v) => updateAudioSetting("noise_suppression", v)}
                            />
                            <ToggleCard
                                label="Echo Cancellation"
                                description="Prevents your microphone from picking up audio from your speakers"
                                checked={settings().audio.echo_cancellation}
                                onChange={(v) => updateAudioSetting("echo_cancellation", v)}
                            />
                        </div>

                        {/* Input Mode */}
                        <div class="space-y-4 pt-4 border-t border-white/10">
                            {/* VAD */}
                            <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                                <ToggleRow
                                    label="Voice Activity Detection"
                                    description="Automatically activate your microphone when you speak"
                                    checked={settings().voice.voice_activity_detection}
                                    onChange={(v) => updateVoiceSetting("voice_activity_detection", v)}
                                />
                                <Show when={settings().voice.voice_activity_detection}>
                                    <div class="mt-4 pt-4 border-t border-white/10">
                                        <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2">
                                            Sensitivity Threshold
                                        </label>
                                        <input
                                            type="range"
                                            min="0"
                                            max="100"
                                            value={Math.round(settings().voice.vad_threshold * 100)}
                                            onInput={(e) => updateVoiceSetting("vad_threshold", parseInt(e.currentTarget.value, 10) / 100)}
                                            class="w-full h-2 rounded-full bg-surface-highlight appearance-none cursor-pointer accent-accent-primary"
                                        />
                                    </div>
                                </Show>
                            </div>

                            {/* PTT */}
                            <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                                <ToggleRow
                                    label="Push to Talk"
                                    description="Only transmit voice when a specific key is held"
                                    checked={settings().voice.push_to_talk}
                                    onChange={(v) => updateVoiceSetting("push_to_talk", v)}
                                />
                                <Show when={settings().voice.push_to_talk}>
                                    <div class="mt-4 pt-4 border-t border-white/10 space-y-3">
                                        <KeyBindInput
                                            label="PTT Key"
                                            currentKey={settings().voice.push_to_talk_key}
                                            otherKey={settings().voice.push_to_mute_key}
                                            onBind={(code) => updateVoiceSetting("push_to_talk_key", code)}
                                            onClear={() => {
                                                updateVoiceSetting("push_to_talk_key", null);
                                                updateVoiceSetting("push_to_talk", false);
                                            }}
                                            autoCapture={!settings().voice.push_to_talk_key}
                                        />
                                        <DelaySlider
                                            label="Release Delay"
                                            value={settings().voice.push_to_talk_release_delay}
                                            onChange={(v) => updateVoiceSetting("push_to_talk_release_delay", v)}
                                        />
                                    </div>
                                </Show>
                            </div>

                            {/* PTM */}
                            <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                                <ToggleRow
                                    label="Push to Mute"
                                    description="Mute your microphone while a specific key is held"
                                    checked={settings().voice.push_to_mute}
                                    onChange={(v) => updateVoiceSetting("push_to_mute", v)}
                                />
                                <Show when={settings().voice.push_to_mute}>
                                    <div class="mt-4 pt-4 border-t border-white/10 space-y-3">
                                        <KeyBindInput
                                            label="PTM Key"
                                            currentKey={settings().voice.push_to_mute_key}
                                            otherKey={settings().voice.push_to_talk_key}
                                            onBind={(code) => updateVoiceSetting("push_to_mute_key", code)}
                                            onClear={() => {
                                                updateVoiceSetting("push_to_mute_key", null);
                                                updateVoiceSetting("push_to_mute", false);
                                            }}
                                            autoCapture={!settings().voice.push_to_mute_key}
                                        />
                                        <DelaySlider
                                            label="Release Delay"
                                            value={settings().voice.push_to_mute_release_delay}
                                            onChange={(v) => updateVoiceSetting("push_to_mute_release_delay", v)}
                                        />
                                    </div>
                                </Show>
                            </div>
                        </div>
                    </>
                )}
            </Show>
        </div>
    );
};

// ============================================================================
// Sub-components
// ============================================================================

const ToggleCard: Component<{
    label: string;
    description: string;
    checked: boolean;
    onChange: (v: boolean) => void;
}> = (props) => (
    <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
        <ToggleRow {...props} />
    </div>
);

const ToggleRow: Component<{
    label: string;
    description: string;
    checked: boolean;
    onChange: (v: boolean) => void;
}> = (props) => (
    <label class="flex items-center gap-3 cursor-pointer">
        <input
            type="checkbox"
            checked={props.checked}
            onChange={(e) => props.onChange(e.currentTarget.checked)}
            class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
        />
        <div>
            <span class="text-text-primary font-medium">{props.label}</span>
            <p class="text-xs text-text-secondary mt-0.5">{props.description}</p>
        </div>
    </label>
);

const KeyBindInput: Component<{
    label: string;
    currentKey: string | null;
    otherKey: string | null;
    onBind: (code: string) => void;
    onClear: () => void;
    autoCapture?: boolean;
}> = (props) => {
    const [capturing, setCapturing] = createSignal(props.autoCapture ?? false);
    const [error, setError] = createSignal<string | null>(null);
    let activeHandler: ((e: KeyboardEvent) => void) | null = null;

    const stopCapture = () => {
        if (activeHandler) {
            window.removeEventListener("keydown", activeHandler, true);
            activeHandler = null;
        }
        setCapturing(false);
    };

    const startCapture = () => {
        stopCapture(); // clean up any prior listener
        setCapturing(true);
        setError(null);

        const handler = (e: KeyboardEvent) => {
            e.preventDefault();
            e.stopPropagation();

            // Ignore modifier-only presses
            if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;

            if (e.code === "Escape") {
                stopCapture();
                return;
            }

            // Check for conflict with other key
            if (props.otherKey && e.code === props.otherKey) {
                setError("PTT and PTM keys must be different");
                stopCapture();
                return;
            }

            setError(null);
            stopCapture();
            props.onBind(e.code);
        };

        activeHandler = handler;
        window.addEventListener("keydown", handler, true);
    };

    // Clean up listener on component unmount
    onCleanup(() => stopCapture());

    // Auto-capture on mount if needed
    if (props.autoCapture) {
        // Use a microtask to avoid capturing the click/key that opened this
        queueMicrotask(() => startCapture());
    }

    return (
        <div>
            <label class="text-sm font-medium text-text-primary mb-1 block">{props.label}</label>
            <div class="flex items-center gap-2">
                <Show
                    when={!capturing()}
                    fallback={
                        <div class="flex-1 px-3 py-2 rounded-lg border-2 border-accent-primary bg-accent-primary/20 text-text-primary text-sm animate-pulse">
                            Press any key... (Esc to cancel)
                        </div>
                    }
                >
                    <button
                        onClick={startCapture}
                        class="flex-1 px-3 py-2 rounded-lg border border-white/20 bg-surface-base text-text-primary text-sm hover:border-white/40 transition-colors text-left"
                    >
                        <Show
                            when={props.currentKey}
                            fallback={<span class="text-text-secondary">Click to set key...</span>}
                        >
                            <kbd class="px-2 py-0.5 bg-surface-highlight rounded border border-white/10 text-sm font-mono">
                                {keyCodeToLabel(props.currentKey!)}
                            </kbd>
                        </Show>
                    </button>
                </Show>
                <Show when={props.currentKey}>
                    <button
                        onClick={() => props.onClear()}
                        class="p-2 rounded-lg text-text-secondary hover:text-accent-danger hover:bg-accent-danger/10 transition-colors text-lg leading-none"
                        title="Clear key binding"
                    >
                        &times;
                    </button>
                </Show>
            </div>
            <Show when={error()}>
                <p class="text-xs text-accent-danger mt-1">{error()}</p>
            </Show>
        </div>
    );
};

const DelaySlider: Component<{
    label: string;
    value: number;
    onChange: (v: number) => void;
}> = (props) => (
    <div>
        <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-1">
            {props.label}
            <span class="text-xs text-text-secondary font-normal">{props.value}ms</span>
        </label>
        <input
            type="range"
            min="0"
            max="1000"
            step="50"
            value={props.value}
            onInput={(e) => props.onChange(parseInt(e.currentTarget.value, 10))}
            class="w-full h-2 rounded-full bg-surface-highlight appearance-none cursor-pointer accent-accent-primary"
        />
    </div>
);

export default VoiceSettings;
