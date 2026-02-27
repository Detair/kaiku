import { Component, Show } from "solid-js";
import { appSettings, updateAudioSetting, updateVoiceSetting, isSettingsLoading } from "@/stores/settings";

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
                        <div class="space-y-4">
                            <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                                <label class="flex items-center gap-3 cursor-pointer">
                                    <input
                                        type="checkbox"
                                        checked={settings().audio.noise_suppression}
                                        onChange={(e) => updateAudioSetting("noise_suppression", e.currentTarget.checked)}
                                        class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
                                    />
                                    <div>
                                        <span class="text-text-primary font-medium">Noise Suppression</span>
                                        <p class="text-xs text-text-secondary mt-0.5">
                                            Filters out background noise like keyboard typing and fans
                                        </p>
                                    </div>
                                </label>
                            </div>

                            <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                                <label class="flex items-center gap-3 cursor-pointer">
                                    <input
                                        type="checkbox"
                                        checked={settings().audio.echo_cancellation}
                                        onChange={(e) => updateAudioSetting("echo_cancellation", e.currentTarget.checked)}
                                        class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
                                    />
                                    <div>
                                        <span class="text-text-primary font-medium">Echo Cancellation</span>
                                        <p class="text-xs text-text-secondary mt-0.5">
                                            Prevents your microphone from picking up audio from your speakers
                                        </p>
                                    </div>
                                </label>
                            </div>
                        </div>

                        <div class="space-y-4 pt-4 border-t border-white/10">
                            <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                                <label class="flex items-center gap-3 cursor-pointer">
                                    <input
                                        type="checkbox"
                                        checked={settings().voice.voice_activity_detection}
                                        onChange={(e) => updateVoiceSetting("voice_activity_detection", e.currentTarget.checked)}
                                        class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
                                    />
                                    <div>
                                        <span class="text-text-primary font-medium">Voice Activity Detection</span>
                                        <p class="text-xs text-text-secondary mt-0.5">
                                            Automatically activate your microphone when you speak
                                        </p>
                                    </div>
                                </label>

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
                                            class="w-full h-2 rounded-full bg-surface-highlight appearance-none cursor-pointer accent-accent-primary flex-1"
                                        />
                                    </div>
                                </Show>
                            </div>

                            <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                                <label class="flex items-center gap-3 cursor-pointer">
                                    <input
                                        type="checkbox"
                                        checked={settings().voice.push_to_talk}
                                        onChange={(e) => updateVoiceSetting("push_to_talk", e.currentTarget.checked)}
                                        class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
                                    />
                                    <div>
                                        <span class="text-text-primary font-medium">Push to Talk</span>
                                        <p class="text-xs text-text-secondary mt-0.5">
                                            Only transmit voice when a specific key is pressed
                                        </p>
                                    </div>
                                </label>
                            </div>
                        </div>
                    </>
                )}
            </Show>
        </div>
    );
};

export default VoiceSettings;
