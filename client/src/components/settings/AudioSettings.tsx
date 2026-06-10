import { Component, createSignal, onMount, For, Show } from "solid-js";
import { appSettings, updateAudioSetting, isSettingsLoading } from "@/stores/settings";
import { createVoiceAdapter, getVoiceAdapter } from "@/lib/webrtc";
import type { AudioDevice } from "@/lib/webrtc";
import { Mic, Volume2 } from "lucide-solid";

const AudioSettings: Component = () => {
    const [inputDevices, setInputDevices] = createSignal<AudioDevice[]>([]);
    const [outputDevices, setOutputDevices] = createSignal<AudioDevice[]>([]);

    onMount(async () => {
        try {
            // Enumerate through the voice adapter so device ids match what
            // the voice stack expects: CPAL device names on desktop,
            // MediaDevices ids in the browser. (navigator.mediaDevices ids
            // would be meaningless to the native audio backend.)
            const adapter = await createVoiceAdapter();
            const result = await adapter.getAudioDevices();
            if (result.ok) {
                setInputDevices(result.value.inputs);
                setOutputDevices(result.value.outputs);
            } else {
                console.error("Failed to enumerate audio devices:", result.error);
            }
        } catch (err) {
            console.error("Failed to enumerate audio devices:", err);
        }
    });

    // Persist the selection and live-apply it to an active voice session.
    const changeDevice = async (kind: "input" | "output", value: string) => {
        const deviceId = value === "default" ? null : value;
        await updateAudioSetting(
            kind === "input" ? "input_device" : "output_device",
            deviceId,
        );
        // Live-apply only for concrete devices; "default" takes effect on
        // the next voice join (the adapter API has no "reset to default").
        const adapter = getVoiceAdapter();
        if (adapter && deviceId) {
            const result =
                kind === "input"
                    ? await adapter.setInputDevice(deviceId)
                    : await adapter.setOutputDevice(deviceId);
            if (!result.ok) {
                console.warn(`Failed to apply ${kind} device:`, result.error);
            }
        }
    };

    return (
        <div class="space-y-6">
            <div>
                <h3 class="text-lg font-semibold text-text-primary mb-1">Audio Settings</h3>
                <p class="text-sm text-text-secondary">
                    Configure your input and output devices and volume.
                </p>
            </div>

            <Show when={!isSettingsLoading() && appSettings()} fallback={<p class="text-text-secondary">Loading...</p>}>
                {(settings) => (
                    <>
                        <div class="space-y-4">
                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2">
                                    <Mic class="w-4 h-4 text-text-secondary" />
                                    Input Device
                                </label>
                                <select
                                    class="w-full px-4 py-2 bg-surface-base border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors appearance-none cursor-pointer"
                                    value={settings().audio.input_device || "default"}
                                    onChange={(e) => changeDevice("input", e.currentTarget.value)}
                                >
                                    <option value="default">Default Device</option>
                                    <For each={inputDevices()}>
                                        {(device) => (
                                            <option value={device.deviceId}>
                                                {device.label || `Microphone (${device.deviceId.slice(0, 5)}...)`}
                                            </option>
                                        )}
                                    </For>
                                </select>
                            </div>

                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2 mt-4">
                                    Input Volume
                                </label>
                                <input
                                    type="range"
                                    min="0"
                                    max="100"
                                    value={settings().audio.input_volume}
                                    onInput={(e) => updateAudioSetting("input_volume", parseInt(e.currentTarget.value, 10))}
                                    class="w-full h-2 rounded-full bg-surface-highlight appearance-none cursor-pointer accent-accent-primary flex-1"
                                />
                            </div>
                        </div>

                        <div class="space-y-4 pt-4 border-t border-white/10">
                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2">
                                    <Volume2 class="w-4 h-4 text-text-secondary" />
                                    Output Device
                                </label>
                                <select
                                    class="w-full px-4 py-2 bg-surface-base border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors appearance-none cursor-pointer"
                                    value={settings().audio.output_device || "default"}
                                    onChange={(e) => changeDevice("output", e.currentTarget.value)}
                                >
                                    <option value="default">Default Device</option>
                                    <For each={outputDevices()}>
                                        {(device) => (
                                            <option value={device.deviceId}>
                                                {device.label || `Speaker (${device.deviceId.slice(0, 5)}...)`}
                                            </option>
                                        )}
                                    </For>
                                </select>
                            </div>

                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2 mt-4">
                                    Output Volume
                                </label>
                                <input
                                    type="range"
                                    min="0"
                                    max="100"
                                    value={settings().audio.output_volume}
                                    onInput={(e) => updateAudioSetting("output_volume", parseInt(e.currentTarget.value, 10))}
                                    class="w-full h-2 rounded-full bg-surface-highlight appearance-none cursor-pointer accent-accent-primary flex-1"
                                />
                            </div>
                        </div>
                    </>
                )}
            </Show>
        </div>
    );
};

export default AudioSettings;
