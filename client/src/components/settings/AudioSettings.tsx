import { Component, createSignal, onMount, For, Show } from "solid-js";
import { appSettings, updateAudioSetting, isSettingsLoading } from "@/stores/settings";
import { createVoiceAdapter, getVoiceAdapter } from "@/lib/webrtc";
import type { AudioDevice } from "@/lib/webrtc";
import { Mic, Volume2 } from "lucide-solid";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";
import { canSelectOutputDevice } from "@/lib/browserCapabilities";
import { showAllAlsaDevices, systemDefaultDevice, visibleDevices } from "@/lib/audioDevices";
import Button from "@/components/ui/Button";
import ShowAllAlsaDevicesToggle from "@/components/voice/ShowAllAlsaDevicesToggle";

const AudioSettings: Component = () => {
    const [inputDevices, setInputDevices] = createSignal<AudioDevice[]>([]);
    const [outputDevices, setOutputDevices] = createSignal<AudioDevice[]>([]);
    const [labelsAvailable, setLabelsAvailable] = createSignal(true);
    const [requestingLabels, setRequestingLabels] = createSignal(false);

    const applyDeviceList = (list: { inputs: AudioDevice[]; outputs: AudioDevice[]; labelsAvailable?: boolean }) => {
        setInputDevices(list.inputs);
        setOutputDevices(list.outputs);
        setLabelsAvailable(list.labelsAvailable !== false);
    };

    // Explicit user action: browsers only reveal device names after the
    // microphone permission has been granted.
    const showDeviceNames = async () => {
        setRequestingLabels(true);
        try {
            const adapter = await createVoiceAdapter();
            const result = await adapter.requestDeviceLabels?.();
            if (result?.ok) {
                applyDeviceList(result.value);
            } else {
                showToast({
                    type: "error",
                    title: t("settings.audio.deviceNamesFailed"),
                    duration: 8000,
                });
            }
        } catch (err) {
            console.error("Failed to request device names:", err);
        } finally {
            setRequestingLabels(false);
        }
    };

    onMount(async () => {
        try {
            // Enumerate through the voice adapter so device ids match what
            // the voice stack expects: CPAL device names on desktop,
            // MediaDevices ids in the browser. (navigator.mediaDevices ids
            // would be meaningless to the native audio backend.)
            const adapter = await createVoiceAdapter();
            const result = await adapter.getAudioDevices();
            if (result.ok) {
                applyDeviceList(result.value);
            } else {
                console.error("Failed to enumerate audio devices:", result.error);
            }
        } catch (err) {
            console.error("Failed to enumerate audio devices:", err);
        }
    });

    // The value shown in a picker: the saved device (matched by id), else
    // the list's own system default entry (desktop list, Chrome's
    // "default"), else the synthetic "default" option.
    const pickerValue = (devices: AudioDevice[], saved: string | null | undefined) => {
        if (saved && devices.some((d) => d.deviceId === saved)) return saved;
        return systemDefaultDevice(devices)?.deviceId ?? "default";
    };

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
                showToast({
                    type: "error",
                    title: t(
                        kind === "input"
                            ? "voice.audio.inputSwitchFailed"
                            : "voice.audio.outputSwitchFailed",
                    ),
                    duration: 8000,
                });
            }
        }
    };

    return (
        <div class="space-y-6">
            <div>
                <h3 class="text-lg font-semibold text-text-primary mb-1">{t("settings.audio.title")}</h3>
                <p class="text-sm text-text-secondary">
                    {t("settings.audio.subtitle")}
                </p>
            </div>

            <Show when={!isSettingsLoading() && appSettings()} fallback={<p class="text-text-secondary">{t("common.loading")}</p>}>
                {(settings) => (
                    <>
                        <div class="space-y-4">
                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2">
                                    <Mic class="w-4 h-4 text-text-secondary" />
                                    {t("settings.audio.inputDevice")}
                                </label>
                                <select
                                    class="w-full px-4 py-2 bg-surface-base border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors appearance-none cursor-pointer"
                                    value={pickerValue(inputDevices(), settings().audio.input_device)}
                                    onChange={(e) => changeDevice("input", e.currentTarget.value)}
                                >
                                    {/* Only when the list has no system default entry of its own. */}
                                    <Show when={!systemDefaultDevice(inputDevices())}>
                                        <option value="default">{t("settings.audio.defaultDevice")}</option>
                                    </Show>
                                    <For each={visibleDevices(inputDevices(), showAllAlsaDevices(), settings().audio.input_device)}>
                                        {(device, i) => (
                                            <option value={device.deviceId}>
                                                {device.label || t("settings.audio.microphoneFallback", { id: i() + 1 })}
                                            </option>
                                        )}
                                    </For>
                                </select>
                                <Show when={!labelsAvailable()}>
                                    <div class="mt-2 flex flex-wrap items-center gap-3">
                                        <p class="text-xs text-text-secondary">{t("settings.audio.deviceNamesHidden")}</p>
                                        <Button
                                            variant="secondary"
                                            size="sm"
                                            disabled={requestingLabels()}
                                            onClick={showDeviceNames}
                                        >
                                            {t("settings.audio.showDeviceNames")}
                                        </Button>
                                    </div>
                                </Show>
                            </div>

                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2 mt-4">
                                    {t("settings.audio.inputVolume")}
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
                            <Show when={canSelectOutputDevice()}>
                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2">
                                    <Volume2 class="w-4 h-4 text-text-secondary" />
                                    {t("settings.audio.outputDevice")}
                                </label>
                                <select
                                    class="w-full px-4 py-2 bg-surface-base border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors appearance-none cursor-pointer"
                                    value={pickerValue(outputDevices(), settings().audio.output_device)}
                                    onChange={(e) => changeDevice("output", e.currentTarget.value)}
                                >
                                    {/* Only when the list has no system default entry of its own. */}
                                    <Show when={!systemDefaultDevice(outputDevices())}>
                                        <option value="default">{t("settings.audio.defaultDevice")}</option>
                                    </Show>
                                    <For each={visibleDevices(outputDevices(), showAllAlsaDevices(), settings().audio.output_device)}>
                                        {(device, i) => (
                                            <option value={device.deviceId}>
                                                {device.label || t("settings.audio.speakerFallback", { id: i() + 1 })}
                                            </option>
                                        )}
                                    </For>
                                </select>
                            </div>
                            </Show>

                            <div>
                                <label class="text-sm font-medium text-text-primary flex items-center gap-2 mb-2 mt-4">
                                    {t("settings.audio.outputVolume")}
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

                        <ShowAllAlsaDevicesToggle devices={[...inputDevices(), ...outputDevices()]} />
                    </>
                )}
            </Show>
        </div>
    );
};

export default AudioSettings;
