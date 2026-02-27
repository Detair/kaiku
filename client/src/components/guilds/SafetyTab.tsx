/**
 * SafetyTab - Content filter configuration for guild settings
 *
 * Provides:
 * 1. Built-in category toggles (Slurs, Hate Speech, Spam, Abusive Language)
 * 2. Custom pattern management (keywords + regex)
 * 3. Filter test panel (dry-run)
 * 4. Moderation action log
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import {
  ShieldAlert,
  Plus,
  Trash2,
  FlaskConical,
  ToggleLeft,
  ToggleRight,
} from "lucide-solid";
import {
  listFilterConfigs,
  updateFilterConfigs,
  listCustomPatterns,
  createCustomPattern,
  deleteCustomPattern,
  listModerationLog,
  testFilter,
} from "@/lib/api/filters";
import type {
  GuildFilterConfig,
  GuildFilterPattern,
  FilterCategory,
  FilterAction,
  ModerationAction,
  TestFilterResponse,
} from "@/lib/api/filters";

interface SafetyTabProps {
  guildId: string;
}

const CATEGORY_INFO: Record<string, { label: string; description: string }> = {
  slurs: {
    label: "Slurs",
    description: "Blocks known slurs and derogatory terms",
  },
  hate_speech: {
    label: "Hate Speech",
    description: "Filters hate speech and discriminatory language",
  },
  spam: {
    label: "Spam",
    description: "Detects spam patterns, phishing links, and scam messages",
  },
  abusive_language: {
    label: "Abusive Language",
    description: "Filters abusive and threatening language",
  },
};

const ALL_CATEGORIES: FilterCategory[] = [
  "slurs",
  "hate_speech",
  "spam",
  "abusive_language",
];

const SafetyTab: Component<SafetyTabProps> = (props) => {
  // Category configs
  const [configs, setConfigs] = createSignal<GuildFilterConfig[]>([]);
  const [configLoading, setConfigLoading] = createSignal(true);
  const [savingConfig, setSavingConfig] = createSignal(false);

  // Local state for toggles (before save)
  const [localConfigs, setLocalConfigs] = createSignal<
    Record<string, { enabled: boolean; action: FilterAction }>
  >({});

  // Custom patterns
  const [patterns, setPatterns] = createSignal<GuildFilterPattern[]>([]);
  const [patternsLoading, setPatternsLoading] = createSignal(true);
  const [showAddPattern, setShowAddPattern] = createSignal(false);
  const [newPattern, setNewPattern] = createSignal("");
  const [newIsRegex, setNewIsRegex] = createSignal(false);
  const [newDescription, setNewDescription] = createSignal("");
  const [addingPattern, setAddingPattern] = createSignal(false);
  const [deleteConfirm, setDeleteConfirm] = createSignal<string | null>(null);

  // Test panel
  const [testInput, setTestInput] = createSignal("");
  const [testResult, setTestResult] = createSignal<TestFilterResponse | null>(
    null,
  );
  const [testing, setTesting] = createSignal(false);

  // Moderation log
  const [logEntries, setLogEntries] = createSignal<ModerationAction[]>([]);
  const [logTotal, setLogTotal] = createSignal(0);
  const [logLoading, setLogLoading] = createSignal(false);
  const [logOffset, setLogOffset] = createSignal(0);

  // Active section
  const [activeSection, setActiveSection] = createSignal<
    "categories" | "patterns" | "test" | "log"
  >("categories");

  onMount(async () => {
    await Promise.all([loadConfigs(), loadPatterns()]);
  });

  const loadConfigs = async () => {
    setConfigLoading(true);
    try {
      const data = await listFilterConfigs(props.guildId);
      setConfigs(data);

      // Initialize local state from server data
      const local: Record<string, { enabled: boolean; action: FilterAction }> =
        {};
      for (const cat of ALL_CATEGORIES) {
        const existing = data.find((c) => c.category === cat);
        local[cat] = {
          enabled: existing?.enabled ?? false,
          action: existing?.action ?? "block",
        };
      }
      setLocalConfigs(local);
    } catch (err) {
      console.error("Failed to load filter configs:", err);
    } finally {
      setConfigLoading(false);
    }
  };

  const loadPatterns = async () => {
    setPatternsLoading(true);
    try {
      const data = await listCustomPatterns(props.guildId);
      setPatterns(data);
    } catch (err) {
      console.error("Failed to load custom patterns:", err);
    } finally {
      setPatternsLoading(false);
    }
  };

  const saveConfigs = async () => {
    setSavingConfig(true);
    try {
      const entries = ALL_CATEGORIES.map((cat) => ({
        category: cat,
        enabled: localConfigs()[cat]?.enabled ?? false,
        action: localConfigs()[cat]?.action ?? ("block" as FilterAction),
      }));
      const updated = await updateFilterConfigs(props.guildId, entries);
      setConfigs(updated);
    } catch (err) {
      console.error("Failed to save filter configs:", err);
    } finally {
      setSavingConfig(false);
    }
  };

  const toggleCategory = (cat: string) => {
    setLocalConfigs((prev) => ({
      ...prev,
      [cat]: { ...prev[cat], enabled: !prev[cat]?.enabled },
    }));
  };

  const setCategoryAction = (cat: string, action: FilterAction) => {
    setLocalConfigs((prev) => ({
      ...prev,
      [cat]: { ...prev[cat], action },
    }));
  };

  const handleAddPattern = async () => {
    if (!newPattern().trim()) return;
    setAddingPattern(true);
    try {
      const pattern = await createCustomPattern(props.guildId, {
        pattern: newPattern().trim(),
        is_regex: newIsRegex(),
        description: newDescription().trim() || undefined,
      });
      setPatterns((prev) => [pattern, ...prev]);
      setNewPattern("");
      setNewIsRegex(false);
      setNewDescription("");
      setShowAddPattern(false);
    } catch (err) {
      console.error("Failed to add pattern:", err);
    } finally {
      setAddingPattern(false);
    }
  };

  const handleDeletePattern = async (patternId: string) => {
    if (deleteConfirm() === patternId) {
      try {
        await deleteCustomPattern(props.guildId, patternId);
        setPatterns((prev) => prev.filter((p) => p.id !== patternId));
      } catch (err) {
        console.error("Failed to delete pattern:", err);
      }
      setDeleteConfirm(null);
    } else {
      setDeleteConfirm(patternId);
      setTimeout(() => setDeleteConfirm(null), 3000);
    }
  };

  const handleTest = async () => {
    if (!testInput().trim()) return;
    setTesting(true);
    try {
      const result = await testFilter(props.guildId, testInput());
      setTestResult(result);
    } catch (err) {
      console.error("Failed to test filter:", err);
    } finally {
      setTesting(false);
    }
  };

  const loadLog = async (offset = 0) => {
    setLogLoading(true);
    try {
      const result = await listModerationLog(props.guildId, 20, offset);
      setLogEntries(result.items);
      setLogTotal(result.total);
      setLogOffset(offset);
    } catch (err) {
      console.error("Failed to load moderation log:", err);
    } finally {
      setLogLoading(false);
    }
  };

  const formatDate = (iso: string) => {
    const d = new Date(iso);
    return d.toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const hasConfigChanges = () => {
    for (const cat of ALL_CATEGORIES) {
      const local = localConfigs()[cat];
      const server = configs().find((c) => c.category === cat);
      if (!local) continue;
      if (!server && local.enabled) return true;
      if (
        server &&
        (server.enabled !== local.enabled || server.action !== local.action)
      )
        return true;
    }
    return false;
  };

  return (
    <div class="p-6 space-y-6">
      {/* Header */}
      <div>
        <h3 class="text-lg font-semibold text-text-primary flex items-center gap-2">
          <ShieldAlert class="w-5 h-5" />
          Content Safety
        </h3>
        <p class="text-sm text-text-secondary mt-1">
          Configure automatic content filtering to protect your community.
        </p>
      </div>

      {/* Section Tabs */}
      <div
        class="flex gap-1 p-1 rounded-lg"
        style="background-color: var(--color-surface-raised)"
      >
        <button
          onClick={() => setActiveSection("categories")}
          class="px-3 py-1.5 text-sm rounded-md transition-colors"
          classList={{
            "bg-accent-primary/20 text-accent-primary font-medium":
              activeSection() === "categories",
            "text-text-secondary hover:text-text-primary":
              activeSection() !== "categories",
          }}
        >
          Categories
        </button>
        <button
          onClick={() => setActiveSection("patterns")}
          class="px-3 py-1.5 text-sm rounded-md transition-colors"
          classList={{
            "bg-accent-primary/20 text-accent-primary font-medium":
              activeSection() === "patterns",
            "text-text-secondary hover:text-text-primary":
              activeSection() !== "patterns",
          }}
        >
          Custom Patterns
        </button>
        <button
          onClick={() => setActiveSection("test")}
          class="px-3 py-1.5 text-sm rounded-md transition-colors"
          classList={{
            "bg-accent-primary/20 text-accent-primary font-medium":
              activeSection() === "test",
            "text-text-secondary hover:text-text-primary":
              activeSection() !== "test",
          }}
        >
          Test
        </button>
        <button
          onClick={() => {
            setActiveSection("log");
            if (logEntries().length === 0) loadLog();
          }}
          class="px-3 py-1.5 text-sm rounded-md transition-colors"
          classList={{
            "bg-accent-primary/20 text-accent-primary font-medium":
              activeSection() === "log",
            "text-text-secondary hover:text-text-primary":
              activeSection() !== "log",
          }}
        >
          Log
        </button>
      </div>

      {/* Categories Section */}
      <Show when={activeSection() === "categories"}>
        <Show
          when={!configLoading()}
          fallback={<p class="text-text-secondary text-sm">Loading...</p>}
        >
          <div class="space-y-3">
            <For each={ALL_CATEGORIES}>
              {(cat) => {
                const info = CATEGORY_INFO[cat];
                const local = () =>
                  localConfigs()[cat] ?? {
                    enabled: false,
                    action: "block" as FilterAction,
                  };
                return (
                  <div
                    class="flex items-center justify-between p-4 rounded-xl border border-white/10"
                    style="background-color: var(--color-surface-raised)"
                  >
                    <div class="flex-1">
                      <div class="flex items-center gap-2">
                        <span class="font-medium text-text-primary">
                          {info?.label ?? cat}
                        </span>
                        <Show when={local().enabled}>
                          <span class="text-xs px-2 py-0.5 rounded-full bg-green-500/20 text-green-400">
                            Active
                          </span>
                        </Show>
                      </div>
                      <p class="text-sm text-text-secondary mt-0.5">
                        {info?.description}
                      </p>
                    </div>
                    <div class="flex items-center gap-3">
                      <select
                        value={local().action}
                        onChange={(e) =>
                          setCategoryAction(
                            cat,
                            e.currentTarget.value as FilterAction,
                          )
                        }
                        class="text-sm px-2 py-1 rounded-lg border border-white/10 bg-transparent text-text-primary"
                      >
                        <option value="block">Block</option>
                        <option value="log">Log Only</option>
                        <option value="warn">Warn</option>
                      </select>
                      <button
                        onClick={() => toggleCategory(cat)}
                        class="text-2xl transition-colors"
                        classList={{
                          "text-green-400": local().enabled,
                          "text-text-secondary": !local().enabled,
                        }}
                      >
                        <Show
                          when={local().enabled}
                          fallback={<ToggleLeft class="w-8 h-8" />}
                        >
                          <ToggleRight class="w-8 h-8" />
                        </Show>
                      </button>
                    </div>
                  </div>
                );
              }}
            </For>
          </div>

          <Show when={hasConfigChanges()}>
            <div class="flex justify-end pt-2">
              <button
                onClick={saveConfigs}
                disabled={savingConfig()}
                class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium text-sm hover:bg-accent-primary/90 disabled:opacity-50 transition-colors"
              >
                {savingConfig() ? "Saving..." : "Save Changes"}
              </button>
            </div>
          </Show>
        </Show>
      </Show>

      {/* Custom Patterns Section */}
      <Show when={activeSection() === "patterns"}>
        <Show
          when={!patternsLoading()}
          fallback={<p class="text-text-secondary text-sm">Loading...</p>}
        >
          <div class="space-y-3">
            <div class="flex items-center justify-between">
              <p class="text-sm text-text-secondary">
                {patterns().length} custom pattern
                {patterns().length !== 1 ? "s" : ""}
              </p>
              <button
                onClick={() => setShowAddPattern(true)}
                class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent-primary/20 text-accent-primary text-sm font-medium hover:bg-accent-primary/30 transition-colors"
              >
                <Plus class="w-4 h-4" />
                Add Pattern
              </button>
            </div>

            {/* Add Pattern Form */}
            <Show when={showAddPattern()}>
              <div
                class="p-4 rounded-xl border border-accent-primary/30 space-y-3"
                style="background-color: var(--color-surface-raised)"
              >
                <div>
                  <label class="text-sm text-text-secondary block mb-1">
                    Pattern
                  </label>
                  <input
                    type="text"
                    value={newPattern()}
                    onInput={(e) => setNewPattern(e.currentTarget.value)}
                    placeholder={
                      newIsRegex() ? "(?i)regex\\s+pattern" : "keyword"
                    }
                    class="w-full px-3 py-2 rounded-lg border border-white/10 bg-transparent text-text-primary text-sm placeholder-text-secondary/50"
                    maxLength={500}
                  />
                </div>
                <div class="flex items-center gap-4">
                  <label class="flex items-center gap-2 text-sm text-text-secondary cursor-pointer">
                    <input
                      type="checkbox"
                      checked={newIsRegex()}
                      onChange={(e) => setNewIsRegex(e.currentTarget.checked)}
                      class="rounded"
                    />
                    Regex pattern
                  </label>
                  <input
                    type="text"
                    value={newDescription()}
                    onInput={(e) => setNewDescription(e.currentTarget.value)}
                    placeholder="Description (optional)"
                    class="flex-1 px-3 py-1.5 rounded-lg border border-white/10 bg-transparent text-text-primary text-sm placeholder-text-secondary/50"
                  />
                </div>
                <div class="flex justify-end gap-2">
                  <button
                    onClick={() => setShowAddPattern(false)}
                    class="px-3 py-1.5 text-sm text-text-secondary hover:text-text-primary transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={handleAddPattern}
                    disabled={addingPattern() || !newPattern().trim()}
                    class="px-4 py-1.5 rounded-lg bg-accent-primary text-white text-sm font-medium hover:bg-accent-primary/90 disabled:opacity-50 transition-colors"
                  >
                    {addingPattern() ? "Adding..." : "Add"}
                  </button>
                </div>
              </div>
            </Show>

            {/* Pattern List */}
            <For
              each={patterns()}
              fallback={
                <p class="text-sm text-text-secondary/60 text-center py-8">
                  No custom patterns configured.
                </p>
              }
            >
              {(pattern) => (
                <div
                  class="flex items-center justify-between p-3 rounded-lg border border-white/5"
                  style="background-color: var(--color-surface-raised)"
                >
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <code class="text-sm text-text-primary truncate">
                        {pattern.pattern}
                      </code>
                      <Show when={pattern.is_regex}>
                        <span class="text-xs px-1.5 py-0.5 rounded bg-purple-500/20 text-purple-400">
                          regex
                        </span>
                      </Show>
                      <Show when={!pattern.enabled}>
                        <span class="text-xs px-1.5 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
                          disabled
                        </span>
                      </Show>
                    </div>
                    <Show when={pattern.description}>
                      <p class="text-xs text-text-secondary mt-0.5 truncate">
                        {pattern.description}
                      </p>
                    </Show>
                  </div>
                  <button
                    onClick={() => handleDeletePattern(pattern.id)}
                    class="p-1.5 rounded-lg transition-colors"
                    classList={{
                      "text-red-400 bg-red-500/20":
                        deleteConfirm() === pattern.id,
                      "text-text-secondary hover:text-red-400 hover:bg-red-500/10":
                        deleteConfirm() !== pattern.id,
                    }}
                    title={
                      deleteConfirm() === pattern.id
                        ? "Click again to confirm"
                        : "Delete"
                    }
                  >
                    <Trash2 class="w-4 h-4" />
                  </button>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>

      {/* Test Section */}
      <Show when={activeSection() === "test"}>
        <div class="space-y-3">
          <p class="text-sm text-text-secondary">
            Test content against your active filters without sending a message.
          </p>
          <div class="flex gap-2">
            <input
              type="text"
              value={testInput()}
              onInput={(e) => setTestInput(e.currentTarget.value)}
              onKeyDown={(e) => e.key === "Enter" && handleTest()}
              placeholder="Type a message to test..."
              class="flex-1 px-3 py-2 rounded-lg border border-white/10 bg-transparent text-text-primary text-sm placeholder-text-secondary/50"
              maxLength={4000}
            />
            <button
              onClick={handleTest}
              disabled={testing() || !testInput().trim()}
              class="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent-primary text-white text-sm font-medium hover:bg-accent-primary/90 disabled:opacity-50 transition-colors"
            >
              <FlaskConical class="w-4 h-4" />
              {testing() ? "Testing..." : "Test"}
            </button>
          </div>

          <Show when={testResult()}>
            {(result) => (
              <div
                class="p-4 rounded-xl border"
                classList={{
                  "border-red-500/30 bg-red-500/5": result().blocked,
                  "border-green-500/30 bg-green-500/5": !result().blocked,
                }}
              >
                <p
                  class="font-medium text-sm"
                  classList={{
                    "text-red-400": result().blocked,
                    "text-green-400": !result().blocked,
                  }}
                >
                  {result().blocked ? "Blocked" : "Allowed"} —{" "}
                  {result().matches.length} match
                  {result().matches.length !== 1 ? "es" : ""}
                </p>
                <Show when={result().matches.length > 0}>
                  <div class="mt-2 space-y-1">
                    <For each={result().matches}>
                      {(match) => (
                        <div class="text-xs text-text-secondary flex gap-2">
                          <span class="px-1.5 py-0.5 rounded bg-white/5">
                            {match.category}
                          </span>
                          <span class="px-1.5 py-0.5 rounded bg-white/5">
                            {match.action}
                          </span>
                          <code class="truncate">{match.matched_pattern}</code>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </div>
            )}
          </Show>
        </div>
      </Show>

      {/* Moderation Log Section */}
      <Show when={activeSection() === "log"}>
        <div class="space-y-3">
          <div class="flex items-center justify-between">
            <p class="text-sm text-text-secondary">
              {logTotal()} total action{logTotal() !== 1 ? "s" : ""}
            </p>
          </div>

          <Show when={logLoading()}>
            <p class="text-sm text-text-secondary text-center py-4">
              Loading...
            </p>
          </Show>

          <Show when={!logLoading()}>
            <For
              each={logEntries()}
              fallback={
                <p class="text-sm text-text-secondary/60 text-center py-8">
                  No moderation actions recorded.
                </p>
              }
            >
              {(entry) => (
                <div
                  class="p-3 rounded-lg border border-white/5 space-y-1"
                  style="background-color: var(--color-surface-raised)"
                >
                  <div class="flex items-center justify-between">
                    <div class="flex items-center gap-2">
                      <span
                        class="text-xs px-1.5 py-0.5 rounded"
                        classList={{
                          "bg-red-500/20 text-red-400":
                            entry.action === "block",
                          "bg-yellow-500/20 text-yellow-400":
                            entry.action === "warn",
                          "bg-blue-500/20 text-blue-400":
                            entry.action === "log",
                        }}
                      >
                        {entry.action}
                      </span>
                      <Show when={entry.category}>
                        <span class="text-xs px-1.5 py-0.5 rounded bg-white/5 text-text-secondary">
                          {entry.category}
                        </span>
                      </Show>
                    </div>
                    <span class="text-xs text-text-secondary">
                      {formatDate(entry.created_at)}
                    </span>
                  </div>
                  <p class="text-xs text-text-secondary">
                    Pattern:{" "}
                    <code class="text-text-primary">
                      {entry.matched_pattern}
                    </code>
                  </p>
                  <p class="text-xs text-text-secondary truncate">
                    Content:{" "}
                    <span class="text-text-primary/60">
                      {entry.original_content}
                    </span>
                  </p>
                </div>
              )}
            </For>

            {/* Pagination */}
            <Show when={logTotal() > 20}>
              <div class="flex justify-center gap-2 pt-2">
                <button
                  onClick={() => loadLog(Math.max(0, logOffset() - 20))}
                  disabled={logOffset() === 0}
                  class="px-3 py-1 text-sm rounded-lg border border-white/10 text-text-secondary hover:text-text-primary disabled:opacity-30 transition-colors"
                >
                  Previous
                </button>
                <span class="text-sm text-text-secondary py-1">
                  {logOffset() + 1}-{Math.min(logOffset() + 20, logTotal())} of{" "}
                  {logTotal()}
                </span>
                <button
                  onClick={() => loadLog(logOffset() + 20)}
                  disabled={logOffset() + 20 >= logTotal()}
                  class="px-3 py-1 text-sm rounded-lg border border-white/10 text-text-secondary hover:text-text-primary disabled:opacity-30 transition-colors"
                >
                  Next
                </button>
              </div>
            </Show>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default SafetyTab;
