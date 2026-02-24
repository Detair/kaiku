/**
 * GeneralTab - General guild settings (threads, discovery, tags, banner)
 */

import { Component, createSignal, createMemo, For, Show, onMount } from "solid-js";
import { X } from "lucide-solid";
import { getGuildSettings, updateGuildSettings } from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";

interface GeneralTabProps {
  guildId: string;
}

const TAG_REGEX = /^[a-zA-Z0-9-]+$/;
const MAX_TAGS = 5;

const GeneralTab: Component<GeneralTabProps> = (props) => {
  const [threadsEnabled, setThreadsEnabled] = createSignal(true);
  const [discoverable, setDiscoverable] = createSignal(false);
  const [tags, setTags] = createSignal<string[]>([]);
  const [tagInput, setTagInput] = createSignal("");
  const [bannerUrl, setBannerUrl] = createSignal("");
  const [loading, setLoading] = createSignal(true);
  const [savingCount, setSavingCount] = createSignal(0);
  const saving = () => savingCount() > 0;
  const [bannerLoadError, setBannerLoadError] = createSignal(false);

  const trimmedBannerUrl = createMemo(() => bannerUrl().trim());
  const isValidBannerUrl = createMemo(() => {
    const url = trimmedBannerUrl();
    if (!url) return false;
    try { return new URL(url).protocol === "https:"; }
    catch { return false; }
  });

  onMount(async () => {
    try {
      const settings = await getGuildSettings(props.guildId);
      setThreadsEnabled(settings.threads_enabled);
      setDiscoverable(settings.discoverable);
      setTags(settings.tags ?? []);
      setBannerUrl(settings.banner_url ?? "");
    } catch (err) {
      console.error("Failed to load guild settings:", err);
      showToast({ type: "error", title: "Settings Error", message: "Could not load guild settings.", duration: 8000 });
    } finally {
      setLoading(false);
    }
  });

  const saveSetting = async (patch: Parameters<typeof updateGuildSettings>[1]) => {
    setSavingCount((c) => c + 1);
    try {
      await updateGuildSettings(props.guildId, patch);
    } catch (err) {
      console.error("Failed to update guild settings:", err);
      showToast({ type: "error", title: "Update Failed", message: "Could not update settings.", duration: 8000 });
      throw err;
    } finally {
      setSavingCount((c) => c - 1);
    }
  };

  const handleToggleThreads = async () => {
    const newValue = !threadsEnabled();
    try {
      await saveSetting({ threads_enabled: newValue });
      setThreadsEnabled(newValue);
    } catch (_: unknown) {
      // error already shown by saveSetting
    }
  };

  const handleToggleDiscoverable = async () => {
    const newValue = !discoverable();
    try {
      await saveSetting({ discoverable: newValue });
      setDiscoverable(newValue);
    } catch (_: unknown) {
      // error already shown by saveSetting
    }
  };

  const handleAddTag = async () => {
    const raw = tagInput().trim().toLowerCase();
    if (!raw) return;
    if (raw.length < 2 || raw.length > 32) {
      showToast({ type: "error", title: "Invalid Tag", message: "Tags must be 2-32 characters." });
      return;
    }
    if (!TAG_REGEX.test(raw)) {
      showToast({ type: "error", title: "Invalid Tag", message: "Tags may only contain letters, numbers, and hyphens." });
      return;
    }
    if (tags().includes(raw)) {
      showToast({ type: "info", title: "Duplicate Tag", message: "This tag already exists." });
      return;
    }
    if (tags().length >= MAX_TAGS) {
      showToast({ type: "error", title: "Tag Limit", message: `Maximum ${MAX_TAGS} tags allowed.` });
      return;
    }

    const newTags = [...tags(), raw];
    try {
      await saveSetting({ tags: newTags });
      setTags(newTags);
      setTagInput("");
    } catch (_: unknown) {
      // error already shown
    }
  };

  const handleRemoveTag = async (tag: string) => {
    const newTags = tags().filter((t) => t !== tag);
    try {
      await saveSetting({ tags: newTags });
      setTags(newTags);
    } catch (_: unknown) {
      // error already shown
    }
  };

  const handleTagKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void handleAddTag();
    }
  };

  const handleBannerSave = async () => {
    const url = bannerUrl().trim() || null;
    if (url && !isValidBannerUrl()) {
      showToast({ type: "error", title: "Invalid URL", message: "Banner URL must use HTTPS." });
      return;
    }
    try {
      await saveSetting({ banner_url: url });
      showToast({ type: "success", title: "Saved", message: "Banner URL updated." });
    } catch (_: unknown) {
      // error already shown
    }
  };

  return (
    <div class="p-6 space-y-6">
      <div>
        <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wide mb-4">
          General
        </h3>

        {/* Threads Toggle */}
        <div class="flex items-center justify-between p-4 bg-surface-layer2 rounded-xl border border-white/5">
          <div class="flex-1 mr-4">
            <div class="text-sm font-medium text-text-primary">
              Enable Message Threads
            </div>
            <div class="text-xs text-text-secondary mt-1">
              Allow members to create threaded replies on messages. Disabling this hides the "Reply in Thread" option but keeps existing threads readable.
            </div>
          </div>
          <button
            onClick={handleToggleThreads}
            disabled={loading() || saving()}
            class="relative w-11 h-6 rounded-full transition-colors duration-200 focus:outline-none focus:ring-2 focus:ring-accent-primary/50 disabled:opacity-50"
            classList={{
              "bg-accent-primary": threadsEnabled(),
              "bg-white/20": !threadsEnabled(),
            }}
            role="switch"
            aria-checked={threadsEnabled()}
            aria-label="Enable Message Threads"
          >
            <span
              class="absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform duration-200"
              classList={{
                "translate-x-5": threadsEnabled(),
                "translate-x-0": !threadsEnabled(),
              }}
            />
          </button>
        </div>
      </div>

      {/* Discovery Section */}
      <div>
        <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wide mb-4">
          Discovery
        </h3>

        {/* Discoverable Toggle */}
        <div class="flex items-center justify-between p-4 bg-surface-layer2 rounded-xl border border-white/5">
          <div class="flex-1 mr-4">
            <div class="text-sm font-medium text-text-primary">
              Make Server Discoverable
            </div>
            <div class="text-xs text-text-secondary mt-1">
              Allow this server to appear in the public server browser. Anyone can find and join without an invite code.
            </div>
          </div>
          <button
            onClick={handleToggleDiscoverable}
            disabled={loading() || saving()}
            class="relative w-11 h-6 rounded-full transition-colors duration-200 focus:outline-none focus:ring-2 focus:ring-accent-primary/50 disabled:opacity-50"
            classList={{
              "bg-accent-primary": discoverable(),
              "bg-white/20": !discoverable(),
            }}
            role="switch"
            aria-checked={discoverable()}
            aria-label="Make Server Discoverable"
          >
            <span
              class="absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform duration-200"
              classList={{
                "translate-x-5": discoverable(),
                "translate-x-0": !discoverable(),
              }}
            />
          </button>
        </div>

        {/* Tags Editor (only shown when discoverable) */}
        <Show when={discoverable()}>
          <div class="mt-4 p-4 bg-surface-layer2 rounded-xl border border-white/5">
            <div class="text-sm font-medium text-text-primary mb-1">Tags</div>
            <div class="text-xs text-text-secondary mb-3">
              Add up to {MAX_TAGS} tags to help people find your server. Letters, numbers, and hyphens only.
            </div>

            {/* Existing tags */}
            <Show when={tags().length > 0}>
              <div class="flex flex-wrap gap-1.5 mb-3">
                <For each={tags()}>
                  {(tag) => (
                    <span class="inline-flex items-center gap-1 px-2 py-0.5 text-xs rounded-lg bg-white/5 text-text-secondary">
                      {tag}
                      <button
                        onClick={() => handleRemoveTag(tag)}
                        disabled={saving()}
                        class="hover:text-text-primary transition-colors disabled:opacity-50"
                        aria-label={`Remove tag ${tag}`}
                      >
                        <X class="w-3 h-3" />
                      </button>
                    </span>
                  )}
                </For>
              </div>
            </Show>

            {/* Add tag input */}
            <Show when={tags().length < MAX_TAGS}>
              <div class="flex items-center gap-2">
                <input
                  type="text"
                  placeholder="Add a tag..."
                  value={tagInput()}
                  onInput={(e) => setTagInput(e.currentTarget.value)}
                  onKeyDown={handleTagKeyDown}
                  maxLength={32}
                  class="flex-1 px-3 py-1.5 text-sm rounded-lg bg-surface-layer1 border border-white/5 text-text-primary placeholder-text-secondary focus:outline-none focus:border-accent-primary/50"
                />
                <button
                  onClick={handleAddTag}
                  disabled={saving() || !tagInput().trim()}
                  class="px-3 py-1.5 text-xs font-medium rounded-lg bg-accent-primary text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
                >
                  Add
                </button>
              </div>
            </Show>
          </div>

          {/* Banner URL */}
          <div class="mt-4 p-4 bg-surface-layer2 rounded-xl border border-white/5">
            <div class="text-sm font-medium text-text-primary mb-1">Banner Image</div>
            <div class="text-xs text-text-secondary mb-3">
              URL to a banner image displayed on your server's discovery card.
            </div>
            <div class="flex items-center gap-2">
              <input
                type="url"
                placeholder="https://example.com/banner.png"
                value={bannerUrl()}
                onInput={(e) => { setBannerUrl(e.currentTarget.value); setBannerLoadError(false); }}
                class="flex-1 px-3 py-1.5 text-sm rounded-lg bg-surface-layer1 border border-white/5 text-text-primary placeholder-text-secondary focus:outline-none focus:border-accent-primary/50"
              />
              <button
                onClick={handleBannerSave}
                disabled={saving()}
                class="px-3 py-1.5 text-xs font-medium rounded-lg bg-accent-primary text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
              >
                Save
              </button>
            </div>
            <Show when={isValidBannerUrl()}>
              <div class="mt-3 h-20 rounded-lg overflow-hidden border border-white/5">
                <Show
                  when={!bannerLoadError()}
                  fallback={
                    <div class="flex items-center justify-center h-full text-xs text-text-secondary">
                      Image failed to load
                    </div>
                  }
                >
                  <img
                    src={trimmedBannerUrl()}
                    alt="Banner preview"
                    class="w-full h-full object-cover"
                    onError={() => setBannerLoadError(true)}
                    onLoad={() => setBannerLoadError(false)}
                  />
                </Show>
              </div>
            </Show>
          </div>
        </Show>
      </div>
    </div>
  );
};

export default GeneralTab;
