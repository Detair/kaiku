/**
 * GeneralTab - General guild settings (threads, discovery, tags, banner)
 */

import {
  Component,
  createSignal,
  createMemo,
  For,
  Show,
  onMount,
} from "solid-js";
import { X, Compass, Info } from "lucide-solid";
import {
  getGuildSettings,
  updateGuildSettings,
  dismissDiscoveryPrompt,
} from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";
import { isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import { isGuildDiscoveryOff } from "@/stores/serverFeatures";
import { t } from "@/i18n";
import Button from "@/components/ui/Button";
import Toggle from "@/components/ui/Toggle";
import ServerNameSection from "./ServerNameSection";

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
  const [promptDismissed, setPromptDismissed] = createSignal(true);

  const trimmedBannerUrl = createMemo(() => bannerUrl().trim());
  const isValidBannerUrl = createMemo(() => {
    const url = trimmedBannerUrl();
    if (!url) return false;
    try {
      return new URL(url).protocol === "https:";
    } catch {
      return false;
    }
  });

  const canManageGuild = () => {
    const userId = authState.user?.id || "";
    return (
      isGuildOwner(props.guildId, userId) ||
      memberHasPermission(
        props.guildId,
        userId,
        false,
        PermissionBits.MANAGE_GUILD,
      )
    );
  };

  const showDiscoveryBanner = createMemo(
    () =>
      !loading() &&
      !isGuildDiscoveryOff() &&
      canManageGuild() &&
      !discoverable() &&
      tags().length === 0 &&
      !promptDismissed(),
  );

  onMount(async () => {
    try {
      const settings = await getGuildSettings(props.guildId);
      setThreadsEnabled(settings.threads_enabled);
      setDiscoverable(settings.discoverable);
      setTags(settings.tags ?? []);
      setBannerUrl(settings.banner_url ?? "");
      setPromptDismissed(settings.discovery_prompt_dismissed);
    } catch (err) {
      console.error("Failed to load guild settings:", err);
      showToast({
        type: "error",
        title: "Settings Error",
        message: "Could not load server settings.",
        duration: 8000,
      });
    } finally {
      setLoading(false);
    }
  });

  const saveSetting = async (
    patch: Parameters<typeof updateGuildSettings>[1],
  ) => {
    setSavingCount((c) => c + 1);
    try {
      await updateGuildSettings(props.guildId, patch);
    } catch (err) {
      console.error("Failed to update guild settings:", err);
      showToast({
        type: "error",
        title: "Update Failed",
        message: "Could not update settings.",
        duration: 8000,
      });
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
      showToast({
        type: "error",
        title: "Invalid Tag",
        message: "Tags must be 2-32 characters.",
      });
      return;
    }
    if (!TAG_REGEX.test(raw)) {
      showToast({
        type: "error",
        title: "Invalid Tag",
        message: "Tags may only contain letters, numbers, and hyphens.",
      });
      return;
    }
    if (tags().includes(raw)) {
      showToast({
        type: "info",
        title: "Duplicate Tag",
        message: "This tag already exists.",
      });
      return;
    }
    if (tags().length >= MAX_TAGS) {
      showToast({
        type: "error",
        title: "Tag Limit",
        message: `Maximum ${MAX_TAGS} tags allowed.`,
      });
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
      showToast({
        type: "error",
        title: "Invalid URL",
        message: "Banner URL must use HTTPS.",
      });
      return;
    }
    try {
      await saveSetting({ banner_url: url });
      showToast({
        type: "success",
        title: "Saved",
        message: "Banner URL updated.",
      });
    } catch (_: unknown) {
      // error already shown
    }
  };

  const handleDismissPrompt = async () => {
    setPromptDismissed(true);
    try {
      await dismissDiscoveryPrompt(props.guildId);
    } catch (err) {
      console.error("Failed to dismiss discovery prompt:", err);
      // Banner stays hidden even if the API call fails — UX choice to
      // avoid the banner popping back. It will reappear on next load.
    }
  };

  const handleSetUpDiscovery = () => {
    const el = document.getElementById("discoverable-toggle");
    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  };

  return (
    <div class="p-6 space-y-6">
      <ServerNameSection guildId={props.guildId} />

      {/* Discovery Setup Banner */}
      <Show when={showDiscoveryBanner()}>
        <div class="relative p-4 rounded-xl border border-accent-primary/20 bg-accent-primary/5">
          <div class="flex items-start gap-3">
            <div class="w-9 h-9 rounded-lg bg-accent-primary/15 flex items-center justify-center flex-shrink-0 mt-0.5">
              <Compass class="w-5 h-5 text-accent-primary" />
            </div>
            <div class="flex-1 min-w-0">
              <div class="text-sm font-semibold text-text-primary">
                Make your server easier to find
              </div>
              <div class="text-xs text-text-secondary mt-1">
                Set up server discovery so others can find and join your server.
              </div>
              <div class="flex items-center gap-2 mt-3">
                <button
                  onClick={handleSetUpDiscovery}
                  class="px-3 py-1.5 text-xs font-medium rounded-lg bg-accent-primary text-on-accent hover:bg-primary-hover transition-colors"
                >
                  Set up
                </button>
                <button
                  onClick={handleDismissPrompt}
                  class="px-3 py-1.5 text-xs font-medium rounded-lg bg-white/5 text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                >
                  Dismiss
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>

      <div>
        <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wide mb-4">
          General
        </h3>

        {/* Threads Toggle */}
        <div class="flex items-center justify-between p-4 bg-surface-layer2 rounded-xl border border-white/5">
          <div class="flex-1 mr-4">
            <div id="guild-threads-label" class="text-sm font-medium text-text-primary">
              Enable Message Threads
            </div>
            <div id="guild-threads-desc" class="text-xs text-text-secondary mt-1">
              Allow members to create threaded replies on messages. Disabling
              this hides the "Reply in Thread" option but keeps existing threads
              readable.
            </div>
          </div>
          <Toggle
            checked={threadsEnabled()}
            onChange={() => handleToggleThreads()}
            disabled={loading() || saving()}
            labelledBy="guild-threads-label"
            describedBy="guild-threads-desc"
          />
        </div>
      </div>

      {/* Discovery Section */}
      <div>
        <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wide mb-4">
          Discovery
        </h3>

        <Show
          when={!isGuildDiscoveryOff()}
          fallback={
            <p
              class="flex items-start gap-2 p-4 text-sm text-text-secondary bg-surface-layer2 rounded-xl border border-border-solid border-dashed"
              data-testid="guild-discovery-off"
            >
              <Info size={16} class="flex-shrink-0 mt-0.5" aria-hidden="true" />
              {t("guilds.settings.discoveryOff")}
            </p>
          }
        >
        {/* Discoverable Toggle */}
        <div
          id="discoverable-toggle"
          class="flex items-center justify-between p-4 bg-surface-layer2 rounded-xl border border-white/5"
        >
          <div class="flex-1 mr-4">
            <div id="guild-discoverable-label" class="text-sm font-medium text-text-primary">
              Make Server Discoverable
            </div>
            <div id="guild-discoverable-desc" class="text-xs text-text-secondary mt-1">
              Allow this server to appear in the public server browser. Anyone
              can find and join without an invite code.
            </div>
          </div>
          <Toggle
            checked={discoverable()}
            onChange={() => handleToggleDiscoverable()}
            disabled={loading() || saving()}
            labelledBy="guild-discoverable-label"
            describedBy="guild-discoverable-desc"
            data-testid="guild-discoverable-toggle"
          />
        </div>

        {/* Tags Editor (only shown when discoverable) */}
        <Show when={discoverable()}>
          <div class="mt-4 p-4 bg-surface-layer2 rounded-xl border border-white/5">
            <div class="text-sm font-medium text-text-primary mb-1">Tags</div>
            <div class="text-xs text-text-secondary mb-3">
              Add up to {MAX_TAGS} tags to help people find your server.
              Letters, numbers, and hyphens only.
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
                        class="hover:text-text-primary transition-colors state-disabled-quiet"
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
                  placeholder={t("guilds.create.tagPlaceholder")}
                  value={tagInput()}
                  onInput={(e) => setTagInput(e.currentTarget.value)}
                  onKeyDown={handleTagKeyDown}
                  maxLength={32}
                  class="flex-1 px-3 py-1.5 text-sm rounded-lg bg-surface-layer1 border border-white/5 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary/50"
                />
                <Button
                  size="sm"
                  onClick={handleAddTag}
                  disabled={saving() || !tagInput().trim()}
                  disabledReason={!tagInput().trim() && t("guilds.create.tagRequired")}
                  showReason={false}
                >
                  Add
                </Button>
              </div>
            </Show>
          </div>

          {/* Banner URL */}
          <div class="mt-4 p-4 bg-surface-layer2 rounded-xl border border-white/5">
            <div class="text-sm font-medium text-text-primary mb-1">
              Banner Image
            </div>
            <div class="text-xs text-text-secondary mb-3">
              URL to a banner image displayed on your server's discovery card.
            </div>
            <div class="flex items-center gap-2">
              <input
                type="url"
                placeholder={t("guilds.create.bannerUrlPlaceholder")}
                value={bannerUrl()}
                onInput={(e) => {
                  setBannerUrl(e.currentTarget.value);
                  setBannerLoadError(false);
                }}
                class="flex-1 px-3 py-1.5 text-sm rounded-lg bg-surface-layer1 border border-white/5 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary/50"
              />
              <button
                onClick={handleBannerSave}
                disabled={saving()}
                class="px-3 py-1.5 text-xs font-medium rounded-lg bg-accent-primary text-on-accent hover:bg-primary-hover transition-colors state-disabled"
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
        </Show>
      </div>
    </div>
  );
};

export default GeneralTab;
