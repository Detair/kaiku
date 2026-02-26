/**
 * Focus Settings
 *
 * Configure focus modes for intelligent notification routing.
 * Supports built-in and custom modes with VIP overrides and emergency keywords.
 */

import { Component, For, Show, createSignal, createMemo } from "solid-js";
import {
  Crosshair,
  Plus,
  Trash2,
  ChevronDown,
  ChevronUp,
} from "lucide-solid";
import type {
  FocusMode,
  FocusSuppressionLevel,
  FocusTriggerCategory,
} from "@/lib/types";
import { preferences, updatePreference, DEFAULT_FOCUS_PREFERENCES } from "@/stores/preferences";
import {
  focusState,
  activateFocusMode,
  deactivateFocusMode,
} from "@/stores/focus";

const MAX_MODES = 10;
const MAX_VIP_USERS = 50;
const MAX_VIP_CHANNELS = 50;
const MAX_KEYWORDS = 5;

const SUPPRESSION_OPTIONS: { value: FocusSuppressionLevel; label: string; desc: string }[] = [
  { value: "all", label: "Suppress all", desc: "Block all notifications" },
  { value: "except_mentions", label: "Except mentions", desc: "Allow @mentions through" },
  { value: "except_dms", label: "Except DMs", desc: "Allow direct messages through" },
];

const TRIGGER_OPTIONS: { value: FocusTriggerCategory; label: string }[] = [
  { value: "game", label: "Games" },
  { value: "coding", label: "Coding" },
  { value: "listening", label: "Listening" },
  { value: "watching", label: "Watching" },
];

function generateId(): string {
  return crypto.randomUUID();
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const FocusSettings: Component = () => {
  const [keywordInput, setKeywordInput] = createSignal("");
  const [vipUserInput, setVipUserInput] = createSignal("");
  const [vipChannelInput, setVipChannelInput] = createSignal("");

  const [expandedModeId, _setExpandedModeId] = createSignal<string | null>(null);
  const setExpandedModeId = (id: string | null) => {
    _setExpandedModeId(id);
    setKeywordInput("");
    setVipUserInput("");
    setVipChannelInput("");
  };

  const focusPrefs = createMemo(() => preferences().focus ?? DEFAULT_FOCUS_PREFERENCES);
  const modes = createMemo(() => focusPrefs().modes);
  const canAddMode = createMemo(() => modes().length < MAX_MODES);

  const updateModes = (updatedModes: FocusMode[]) => {
    updatePreference("focus", { ...focusPrefs(), modes: updatedModes });
  };

  const updateMode = (modeId: string, changes: Partial<FocusMode>) => {
    const updatedModes = modes().map((m) =>
      m.id === modeId ? { ...m, ...changes } : m
    );
    updateModes(updatedModes);
  };

  const handleAddMode = () => {
    if (!canAddMode()) return;
    const newMode: FocusMode = {
      id: generateId(),
      name: "New Mode",
      icon: "crosshair",
      builtin: false,
      triggerCategories: null,
      autoActivateEnabled: false,
      suppressionLevel: "all",
      vipUserIds: [],
      vipChannelIds: [],
      emergencyKeywords: [],
    };
    updateModes([...modes(), newMode]);
    setExpandedModeId(newMode.id);
  };

  const handleDeleteMode = (modeId: string) => {
    const mode = modes().find((m) => m.id === modeId);
    if (!mode || mode.builtin) return;

    // Deactivate if this mode is active
    if (focusState().activeModeId === modeId) {
      deactivateFocusMode();
    }

    updateModes(modes().filter((m) => m.id !== modeId));
    if (expandedModeId() === modeId) {
      setExpandedModeId(null);
    }
  };

  const handleToggleAutoActivateGlobal = (enabled: boolean) => {
    updatePreference("focus", { ...focusPrefs(), autoActivateGlobal: enabled });
  };

  const handleToggleTriggerCategory = (modeId: string, category: FocusTriggerCategory) => {
    const mode = modes().find((m) => m.id === modeId);
    if (!mode) return;

    const current = mode.triggerCategories ?? [];
    const updated = current.includes(category)
      ? current.filter((c) => c !== category)
      : [...current, category];

    updateMode(modeId, { triggerCategories: updated.length > 0 ? updated : null });
  };

  const handleAddKeyword = (modeId: string) => {
    const keyword = keywordInput().trim();
    if (!keyword || keyword.length < 3 || keyword.length > 30) return;

    const mode = modes().find((m) => m.id === modeId);
    if (!mode || mode.emergencyKeywords.length >= MAX_KEYWORDS) return;
    if (mode.emergencyKeywords.includes(keyword)) return;

    updateMode(modeId, { emergencyKeywords: [...mode.emergencyKeywords, keyword] });
    setKeywordInput("");
  };

  const handleRemoveKeyword = (modeId: string, keyword: string) => {
    const mode = modes().find((m) => m.id === modeId);
    if (!mode) return;
    updateMode(modeId, { emergencyKeywords: mode.emergencyKeywords.filter((k) => k !== keyword) });
  };

  const handleAddVipUser = (modeId: string) => {
    const userId = vipUserInput().trim();
    if (!userId || !UUID_RE.test(userId)) return;

    const mode = modes().find((m) => m.id === modeId);
    if (!mode || mode.vipUserIds.length >= MAX_VIP_USERS) return;
    if (mode.vipUserIds.includes(userId)) return;

    updateMode(modeId, { vipUserIds: [...mode.vipUserIds, userId] });
    setVipUserInput("");
  };

  const handleRemoveVipUser = (modeId: string, userId: string) => {
    const mode = modes().find((m) => m.id === modeId);
    if (!mode) return;
    updateMode(modeId, { vipUserIds: mode.vipUserIds.filter((id) => id !== userId) });
  };

  const handleAddVipChannel = (modeId: string) => {
    const channelId = vipChannelInput().trim();
    if (!channelId || !UUID_RE.test(channelId)) return;

    const mode = modes().find((m) => m.id === modeId);
    if (!mode || mode.vipChannelIds.length >= MAX_VIP_CHANNELS) return;
    if (mode.vipChannelIds.includes(channelId)) return;

    updateMode(modeId, { vipChannelIds: [...mode.vipChannelIds, channelId] });
    setVipChannelInput("");
  };

  const handleRemoveVipChannel = (modeId: string, channelId: string) => {
    const mode = modes().find((m) => m.id === modeId);
    if (!mode) return;
    updateMode(modeId, { vipChannelIds: mode.vipChannelIds.filter((id) => id !== channelId) });
  };

  return (
    <div class="space-y-6">
      {/* Header */}
      <div>
        <h3 class="text-lg font-semibold mb-2 text-text-primary flex items-center gap-2">
          <Crosshair class="w-5 h-5" />
          Focus Modes
        </h3>
        <p class="text-sm text-text-secondary">
          Suppress notifications during focused sessions. VIP contacts and emergency keywords can bypass suppression.
        </p>
      </div>

      {/* Global auto-activate toggle */}
      <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
        <label class="flex items-center gap-3 cursor-pointer">
          <input
            type="checkbox"
            checked={focusPrefs().autoActivateGlobal}
            onChange={(e) => handleToggleAutoActivateGlobal(e.currentTarget.checked)}
            class="w-5 h-5 rounded border-2 border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary transition-colors cursor-pointer accent-accent-primary"
          />
          <div>
            <span class="text-text-primary font-medium">Auto-activate focus modes</span>
            <p class="text-xs text-text-secondary mt-0.5">
              Automatically enable focus mode when a matching app is detected
            </p>
          </div>
        </label>
      </div>

      {/* Active mode indicator */}
      <Show when={focusState().activeModeId}>
        {(modeId) => {
          const activeMode = createMemo(() =>
            modes().find((m) => m.id === modeId())
          );
          return (
            <Show when={activeMode()}>
              {(mode) => (
                <div class="flex items-center justify-between p-3 rounded-xl bg-accent-primary/10 border border-accent-primary/30">
                  <div class="flex items-center gap-2">
                    <div class="w-2 h-2 rounded-full bg-accent-primary animate-pulse" />
                    <span class="text-sm text-accent-primary font-medium">
                      {mode().name} active
                      {focusState().autoActivated ? " (auto)" : ""}
                    </span>
                  </div>
                  <button
                    onClick={() => deactivateFocusMode()}
                    class="text-xs px-2 py-1 rounded-lg bg-white/10 hover:bg-white/20 text-text-primary transition-colors"
                  >
                    End
                  </button>
                </div>
              )}
            </Show>
          );
        }}
      </Show>

      {/* Modes list */}
      <div class="space-y-2">
        <For each={modes()}>
          {(mode) => {
            const isExpanded = createMemo(() => expandedModeId() === mode.id);
            const isActive = createMemo(() => focusState().activeModeId === mode.id);

            return (
              <div class="rounded-xl border border-white/10 overflow-hidden">
                {/* Mode header */}
                <button
                  onClick={() => setExpandedModeId(isExpanded() ? null : mode.id)}
                  class="w-full flex items-center gap-3 px-4 py-3 hover:bg-white/5 transition-colors text-left"
                >
                  <div class="flex-1 flex items-center gap-3">
                    <span class="text-text-primary font-medium">{mode.name}</span>
                    <Show when={mode.builtin}>
                      <span class="text-[10px] px-1.5 py-0.5 rounded bg-white/10 text-text-muted uppercase tracking-wide">
                        built-in
                      </span>
                    </Show>
                    <Show when={isActive()}>
                      <span class="text-[10px] px-1.5 py-0.5 rounded bg-accent-primary/20 text-accent-primary uppercase tracking-wide">
                        active
                      </span>
                    </Show>
                  </div>
                  <div class="flex items-center gap-2">
                    <Show when={!isActive()}>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          activateFocusMode(mode.id);
                        }}
                        class="text-xs px-2 py-1 rounded-lg bg-white/10 hover:bg-accent-primary/20 text-text-secondary hover:text-accent-primary transition-colors"
                      >
                        Activate
                      </button>
                    </Show>
                    {isExpanded() ? (
                      <ChevronUp class="w-4 h-4 text-text-muted" />
                    ) : (
                      <ChevronDown class="w-4 h-4 text-text-muted" />
                    )}
                  </div>
                </button>

                {/* Expanded editor */}
                <Show when={isExpanded()}>
                  <div class="px-4 pb-4 space-y-4 border-t border-white/5">
                    {/* Name */}
                    <Show when={!mode.builtin}>
                      <div class="pt-4">
                        <label class="text-sm text-text-secondary mb-1 block">Name</label>
                        <input
                          type="text"
                          value={mode.name}
                          maxLength={30}
                          onInput={(e) => {
                            const name = e.currentTarget.value;
                            if (name.trim().length > 0) {
                              updateMode(mode.id, { name });
                            }
                          }}
                          class="w-full px-3 py-2 rounded-lg bg-surface-highlight border border-white/10 text-text-primary text-sm focus:outline-none focus:border-accent-primary transition-colors"
                        />
                      </div>
                    </Show>

                    {/* Suppression level */}
                    <div class={mode.builtin ? "pt-4" : ""}>
                      <label class="text-sm text-text-secondary mb-2 block">Suppression level</label>
                      <div class="space-y-1">
                        <For each={SUPPRESSION_OPTIONS}>
                          {(option) => (
                            <label class="flex items-center gap-3 cursor-pointer p-2 rounded-lg hover:bg-white/5">
                              <input
                                type="radio"
                                name={`suppression-${mode.id}`}
                                checked={mode.suppressionLevel === option.value}
                                onChange={() => updateMode(mode.id, { suppressionLevel: option.value })}
                                class="accent-accent-primary"
                              />
                              <div>
                                <span class="text-sm text-text-primary">{option.label}</span>
                                <p class="text-xs text-text-muted">{option.desc}</p>
                              </div>
                            </label>
                          )}
                        </For>
                      </div>
                    </div>

                    {/* Trigger categories */}
                    <div>
                      <label class="text-sm text-text-secondary mb-2 block">Auto-activate triggers</label>
                      <div class="flex flex-wrap gap-2">
                        <For each={TRIGGER_OPTIONS}>
                          {(option) => {
                            const isSelected = createMemo(() =>
                              mode.triggerCategories?.includes(option.value) ?? false
                            );
                            return (
                              <button
                                onClick={() => handleToggleTriggerCategory(mode.id, option.value)}
                                class="text-xs px-3 py-1.5 rounded-lg border transition-colors"
                                classList={{
                                  "border-accent-primary bg-accent-primary/10 text-accent-primary": isSelected(),
                                  "border-white/10 text-text-secondary hover:border-white/20": !isSelected(),
                                }}
                              >
                                {option.label}
                              </button>
                            );
                          }}
                        </For>
                      </div>
                      <label class="flex items-center gap-2 mt-2 cursor-pointer">
                        <input
                          type="checkbox"
                          checked={mode.autoActivateEnabled}
                          onChange={(e) => updateMode(mode.id, { autoActivateEnabled: e.currentTarget.checked })}
                          class="w-4 h-4 rounded border border-white/30 bg-transparent checked:bg-accent-primary checked:border-accent-primary accent-accent-primary"
                        />
                        <span class="text-xs text-text-secondary">Enable auto-activation for this mode</span>
                      </label>
                    </div>

                    {/* Emergency keywords */}
                    <div>
                      <label class="text-sm text-text-secondary mb-2 block">
                        Emergency keywords ({mode.emergencyKeywords.length}/{MAX_KEYWORDS})
                      </label>
                      <p class="text-xs text-text-muted mb-2">
                        Messages containing these words bypass suppression
                      </p>
                      <div class="flex flex-wrap gap-1 mb-2">
                        <For each={mode.emergencyKeywords}>
                          {(keyword) => (
                            <span class="inline-flex items-center gap-1 text-xs px-2 py-1 rounded-lg bg-white/10 text-text-primary">
                              {keyword}
                              <button
                                onClick={() => handleRemoveKeyword(mode.id, keyword)}
                                class="text-text-muted hover:text-red-400 transition-colors"
                              >
                                &times;
                              </button>
                            </span>
                          )}
                        </For>
                      </div>
                      <Show when={mode.emergencyKeywords.length < MAX_KEYWORDS}>
                        <div class="flex gap-2">
                          <input
                            type="text"
                            value={keywordInput()}
                            maxLength={30}
                            onInput={(e) => setKeywordInput(e.currentTarget.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") handleAddKeyword(mode.id);
                            }}
                            placeholder="Add keyword (min 3 chars)..."
                            class="flex-1 px-3 py-1.5 rounded-lg bg-surface-highlight border border-white/10 text-text-primary text-xs focus:outline-none focus:border-accent-primary transition-colors"
                          />
                          <button
                            onClick={() => handleAddKeyword(mode.id)}
                            class="px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/20 text-text-primary text-xs transition-colors"
                          >
                            Add
                          </button>
                        </div>
                      </Show>
                    </div>

                    {/* VIP Users */}
                    <div>
                      <label class="text-sm text-text-secondary mb-2 block">
                        VIP Users ({mode.vipUserIds.length}/{MAX_VIP_USERS})
                      </label>
                      <p class="text-xs text-text-muted mb-2">
                        Messages from these users bypass focus suppression
                      </p>
                      <Show when={mode.vipUserIds.length > 0}>
                        <div class="flex flex-wrap gap-1 mb-2">
                          <For each={mode.vipUserIds}>
                            {(userId) => (
                              <span class="inline-flex items-center gap-1 text-xs px-2 py-1 rounded-lg bg-white/10 text-text-primary">
                                {userId.substring(0, 8)}...
                                <button
                                  onClick={() => handleRemoveVipUser(mode.id, userId)}
                                  class="text-text-muted hover:text-red-400 transition-colors"
                                >
                                  &times;
                                </button>
                              </span>
                            )}
                          </For>
                        </div>
                      </Show>
                      <Show when={mode.vipUserIds.length < MAX_VIP_USERS}>
                        <div class="flex gap-2">
                          <input
                            type="text"
                            value={vipUserInput()}
                            onInput={(e) => setVipUserInput(e.currentTarget.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") handleAddVipUser(mode.id);
                            }}
                            placeholder="User ID..."
                            class="flex-1 px-3 py-1.5 rounded-lg bg-surface-highlight border border-white/10 text-text-primary text-xs focus:outline-none focus:border-accent-primary transition-colors"
                          />
                          <button
                            onClick={() => handleAddVipUser(mode.id)}
                            class="px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/20 text-text-primary text-xs transition-colors"
                          >
                            Add
                          </button>
                        </div>
                      </Show>
                    </div>

                    {/* VIP Channels */}
                    <div>
                      <label class="text-sm text-text-secondary mb-2 block">
                        VIP Channels ({mode.vipChannelIds.length}/{MAX_VIP_CHANNELS})
                      </label>
                      <p class="text-xs text-text-muted mb-2">
                        Messages from these channels bypass focus suppression
                      </p>
                      <Show when={mode.vipChannelIds.length > 0}>
                        <div class="flex flex-wrap gap-1 mb-2">
                          <For each={mode.vipChannelIds}>
                            {(channelId) => (
                              <span class="inline-flex items-center gap-1 text-xs px-2 py-1 rounded-lg bg-white/10 text-text-primary">
                                {channelId.substring(0, 8)}...
                                <button
                                  onClick={() => handleRemoveVipChannel(mode.id, channelId)}
                                  class="text-text-muted hover:text-red-400 transition-colors"
                                >
                                  &times;
                                </button>
                              </span>
                            )}
                          </For>
                        </div>
                      </Show>
                      <Show when={mode.vipChannelIds.length < MAX_VIP_CHANNELS}>
                        <div class="flex gap-2">
                          <input
                            type="text"
                            value={vipChannelInput()}
                            onInput={(e) => setVipChannelInput(e.currentTarget.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") handleAddVipChannel(mode.id);
                            }}
                            placeholder="Channel ID..."
                            class="flex-1 px-3 py-1.5 rounded-lg bg-surface-highlight border border-white/10 text-text-primary text-xs focus:outline-none focus:border-accent-primary transition-colors"
                          />
                          <button
                            onClick={() => handleAddVipChannel(mode.id)}
                            class="px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/20 text-text-primary text-xs transition-colors"
                          >
                            Add
                          </button>
                        </div>
                      </Show>
                    </div>

                    {/* Delete button for custom modes */}
                    <Show when={!mode.builtin}>
                      <div class="pt-2 border-t border-white/5">
                        <button
                          onClick={() => handleDeleteMode(mode.id)}
                          class="flex items-center gap-2 text-xs text-red-400 hover:text-red-300 transition-colors"
                        >
                          <Trash2 class="w-3.5 h-3.5" />
                          Delete this mode
                        </button>
                      </div>
                    </Show>
                  </div>
                </Show>
              </div>
            );
          }}
        </For>
      </div>

      {/* Add mode button */}
      <Show when={canAddMode()}>
        <button
          onClick={handleAddMode}
          class="w-full flex items-center justify-center gap-2 py-3 rounded-xl border border-dashed border-white/20 hover:border-accent-primary/50 text-text-secondary hover:text-accent-primary transition-colors"
        >
          <Plus class="w-4 h-4" />
          <span class="text-sm">Add custom mode ({modes().length}/{MAX_MODES})</span>
        </button>
      </Show>

      {/* Info text */}
      <p class="text-xs text-text-muted">
        DND status and quiet hours always suppress all notifications regardless of focus mode.
        Focus modes add intelligent filtering on top of existing notification settings.
      </p>
    </div>
  );
};

export default FocusSettings;
