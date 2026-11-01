/**
 * CreateGuildModal - Two-step modal for creating new guilds
 *
 * Step 1: Name + Description
 * Step 2: Discovery setup (toggle, tags, banner URL, preview)
 *
 * When the instance has the server browser turned off (`guild_discovery:
 * false` in /api/settings) step 2 offers nothing, so the modal is a single
 * step that creates the server directly and says why.
 */

import { Component, createSignal, createMemo, Show, For, onMount, onCleanup } from "solid-js";
import { Info, X } from "lucide-solid";
import { createGuild } from "@/stores/guilds";
import { isGuildDiscoveryOff } from "@/stores/serverFeatures";
import * as tauri from "@/lib/tauri";
import { Portal } from "solid-js/web";
import { t } from "@/i18n";
import Button from "@/components/ui/Button";
import FileInput from "@/components/ui/FileInput";
import Toggle from "@/components/ui/Toggle";

interface CreateGuildModalProps {
  onClose: () => void;
}

const TAG_REGEX = /^[a-zA-Z0-9-]+$/;
const MAX_TAGS = 5;

const CreateGuildModal: Component<CreateGuildModalProps> = (props) => {
  onMount(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    onCleanup(() => document.removeEventListener("keydown", handleKeyDown));
  });

  // Step state: 1 = name/description, 2 = discovery setup
  const [step, setStep] = createSignal<1 | 2>(1);

  // Step 1 fields
  const [name, setName] = createSignal("");
  const [description, setDescription] = createSignal("");

  // Step 2 fields
  const [discoverable, setDiscoverable] = createSignal(false);
  const [tags, setTags] = createSignal<string[]>([]);
  const [tagInput, setTagInput] = createSignal("");
  const [tagError, setTagError] = createSignal<string | null>(null);
  const [bannerUrl, setBannerUrl] = createSignal("");
  const [bannerFile, setBannerFile] = createSignal<File | null>(null);
  const [bannerPreviewUrl, setBannerPreviewUrl] = createSignal("");
  const [bannerLoadError, setBannerLoadError] = createSignal(false);

  // Shared state
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

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

  // Initials for the preview card
  const initials = createMemo(() =>
    name()
      .trim()
      .split(" ")
      .filter((w) => w.length > 0)
      .map((w) => w[0])
      .join("")
      .toUpperCase()
      .slice(0, 2),
  );

  const validateName = (): boolean => {
    const guildName = name().trim();
    if (!guildName) {
      setError("Server name is required");
      return false;
    }

    if (guildName.length < 2 || guildName.length > 100) {
      setError("Server name must be between 2 and 100 characters");
      return false;
    }
    return true;
  };

  const handleNext = (e: Event) => {
    e.preventDefault();
    if (!validateName()) return;
    setError(null);
    setStep(2);
  };

  const handleBack = () => {
    setError(null);
    setStep(1);
  };

  const handleAddTag = () => {
    const raw = tagInput().trim().toLowerCase();
    if (!raw) return;

    if (raw.length < 2 || raw.length > 32) {
      setTagError("Tags must be 2-32 characters.");
      return;
    }
    if (!TAG_REGEX.test(raw)) {
      setTagError("Tags may only contain letters, numbers, and hyphens.");
      return;
    }
    if (tags().includes(raw)) {
      setTagError("This tag already exists.");
      return;
    }
    if (tags().length >= MAX_TAGS) {
      setTagError(`Maximum ${MAX_TAGS} tags allowed.`);
      return;
    }

    setTags((prev) => [...prev, raw]);
    setTagInput("");
    setTagError(null);
  };

  const handleRemoveTag = (tag: string) => {
    setTags((prev) => prev.filter((t) => t !== tag));
  };

  const handleTagKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleAddTag();
    }
  };

  const handleCreate = async (e: Event) => {
    e.preventDefault();
    if (!validateName()) return;

    // Validate banner URL if provided
    const banner = trimmedBannerUrl();
    if (banner && !isValidBannerUrl() && !bannerFile()) {
      setError("Banner URL must use HTTPS.");
      return;
    }

    setIsCreating(true);
    setError(null);

    try {
      const discoveryParams = discoverable() && !isGuildDiscoveryOff()
        ? {
            discoverable: true as const,
            tags: tags().length > 0 ? tags() : undefined,
            banner_url: !bannerFile() && banner ? banner : undefined,
          }
        : undefined;

      const newGuild = await createGuild(
        name().trim(),
        description().trim() || undefined,
        discoveryParams,
      );

      if (discoverable() && !isGuildDiscoveryOff() && bannerFile()) {
        try {
          await tauri.uploadGuildBanner(newGuild.id, bannerFile()!);
        } catch (uploadErr) {
          setError(
            `Server created, but banner upload failed: ${
              uploadErr instanceof Error ? uploadErr.message : "Unknown error"
            }. You can try again in settings.`,
          );
          setIsCreating(false);
          return;
        }
      }

      props.onClose();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to create server",
      );
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <Portal>
      <div class="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
        <div
          class="border border-white/10 rounded-2xl w-[480px] max-w-[calc(100vw-2rem)] max-h-[min(640px,calc(100vh-2rem))] flex flex-col shadow-2xl"
          style={{"background-color":"var(--color-surface-base)"}}
        >
          {/* Header */}
          <div class="flex items-center justify-between p-6 border-b border-white/10">
            <h2 class="text-xl font-bold text-text-primary">
              Create a Server
            </h2>
            <button
              onClick={props.onClose}
              class="text-text-secondary hover:text-text-primary transition-colors"
              aria-label="Close"
            >
              <X size={24} />
            </button>
          </div>

          {/* Step indicator (two steps only while discovery is available) */}
          <Show when={!isGuildDiscoveryOff()}>
          <div class="flex items-center gap-2 px-6 pt-4" aria-hidden="true">
            <div
              class="h-1 flex-1 rounded-full transition-colors"
              classList={{
                "bg-accent-primary": true,
              }}
            />
            <div
              class="h-1 flex-1 rounded-full transition-colors"
              classList={{
                "bg-accent-primary": step() === 2,
                "bg-surface-highlight": step() === 1,
              }}
            />
          </div>
          </Show>

          {/* Content */}
          <Show when={step() === 1}>
            <form
              onSubmit={(e) => (isGuildDiscoveryOff() ? handleCreate(e) : handleNext(e))}
              class="flex-1 overflow-y-auto p-6"
            >
              <div class="space-y-4">
                {/* Server Name */}
                <div>
                  <label class="block text-sm font-semibold text-text-primary mb-2">
                    Server Name <span class="text-accent-danger">*</span>
                  </label>
                  <input
                    type="text"
                    data-testid="create-guild-name"
                    value={name()}
                    onInput={(e) => setName(e.currentTarget.value)}
                    placeholder={t("guilds.create.namePlaceholder")}
                    class="w-full px-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-placeholder focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
                    style={{"background-color":"var(--color-surface-layer2)"}}
                    maxLength={100}
                    disabled={isCreating()}
                    autofocus
                  />
                  <p class="text-xs text-text-secondary mt-1">
                    {name().length}/100 characters
                  </p>
                </div>

                {/* Description */}
                <div>
                  <label class="block text-sm font-semibold text-text-primary mb-2">
                    Description{" "}
                    <span class="text-text-secondary text-xs">(optional)</span>
                  </label>
                  <textarea
                    value={description()}
                    onInput={(e) => setDescription(e.currentTarget.value)}
                    placeholder={t("guilds.create.descriptionPlaceholder")}
                    class="w-full px-4 py-3 border border-white/10 rounded-lg text-text-input placeholder:text-text-placeholder focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent resize-none"
                    style={{"background-color":"var(--color-surface-layer2)"}}
                    rows={3}
                    maxLength={1000}
                    disabled={isCreating()}
                  />
                  <p class="text-xs text-text-secondary mt-1">
                    {description().length}/1000 characters
                  </p>
                </div>

                <Show when={isGuildDiscoveryOff()}>
                  <p
                    class="flex items-start gap-2 text-xs text-text-secondary"
                    data-testid="create-guild-discovery-off"
                  >
                    <Info size={14} class="flex-shrink-0 mt-0.5" aria-hidden="true" />
                    {t("guilds.create.discoveryOff")}
                  </p>
                </Show>

                {/* Error Message */}
                <Show when={error()}>
                  <div
                    class="p-3 rounded-lg"
                    style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)"}}
                  >
                    <p class="text-sm" style={{"color":"var(--color-error-text)"}}>
                      {error()}
                    </p>
                  </div>
                </Show>
              </div>
            </form>

            {/* Footer - Step 1 */}
            <div class="flex items-center justify-end gap-3 p-6 border-t border-white/10">
              <button
                type="button"
                onClick={props.onClose}
                class="px-4 py-2 text-text-primary hover:bg-surface-layer2 rounded-lg transition-colors"
              >
                Cancel
              </button>
              <Show
                when={!isGuildDiscoveryOff()}
                fallback={
                  <Button
                    type="submit"
                    onClick={handleCreate}
                    data-testid="create-guild-submit"
                    disabled={isCreating() || !name().trim()}
                    disabledReason={!name().trim() && t("guilds.create.nameRequired")}
                  >
                    {isCreating() ? "Creating..." : "Create Server"}
                  </Button>
                }
              >
                <Button
                  type="submit"
                  onClick={handleNext}
                  data-testid="create-guild-next"
                  disabled={!name().trim()}
                  disabledReason={t("guilds.create.nameRequired")}
                >
                  Next
                </Button>
              </Show>
            </div>
          </Show>

          <Show when={step() === 2}>
            <div class="flex-1 overflow-y-auto p-6">
              <div class="space-y-4">
                {/* Discoverable Toggle */}
                <div class="flex items-center justify-between p-4 bg-surface-layer2 rounded-xl border border-white/5">
                  <div class="flex-1 mr-4">
                    <div class="text-sm font-medium text-text-primary">
                      Make this server visible in the server browser
                    </div>
                    <div class="text-xs text-text-secondary mt-1">
                      Anyone can find and join your server without an invite
                      code.
                    </div>
                  </div>
                  <Toggle
                    checked={discoverable()}
                    onChange={setDiscoverable}
                    disabled={isCreating()}
                    label={t("guilds.create.discoverableLabel")}
                    data-testid="create-guild-discoverable-toggle"
                  />
                </div>

                {/* Discovery fields - only when toggle is on */}
                <Show when={discoverable()}>
                  {/* Tags Input */}
                  <div class="p-4 bg-surface-layer2 rounded-xl border border-white/5">
                    <div class="text-sm font-medium text-text-primary mb-1">
                      Tags
                    </div>
                    <div class="text-xs text-text-secondary mb-3">
                      Add up to {MAX_TAGS} tags to help people find your
                      server. Letters, numbers, and hyphens only.
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
                                disabled={isCreating()}
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
                          onInput={(e) => {
                            setTagInput(e.currentTarget.value);
                            setTagError(null);
                          }}
                          onKeyDown={handleTagKeyDown}
                          maxLength={32}
                          disabled={isCreating()}
                          class="flex-1 px-3 py-1.5 text-sm rounded-lg bg-surface-layer1 border border-white/5 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary/50"
                        />
                        <Button
                          size="sm"
                          onClick={handleAddTag}
                          disabled={isCreating() || !tagInput().trim()}
                          disabledReason={!tagInput().trim() && t("guilds.create.tagRequired")}
                          showReason={false}
                        >
                          Add
                        </Button>
                      </div>
                    </Show>

                    <Show when={tagError()}>
                      <p class="text-xs mt-2" style={{"color":"var(--color-error-text)"}}>
                        {tagError()}
                      </p>
                    </Show>
                  </div>

                  {/* Banner URL */}
                  <div class="p-4 bg-surface-layer2 rounded-xl border border-white/5">
                    <div class="text-sm font-medium text-text-primary mb-1">
                      Banner Image
                    </div>
                    <div class="text-xs text-text-secondary mb-3">
                      URL to a banner image displayed on your server's
                      discovery card.
                    </div>
                    <div class="flex flex-col gap-3">
                      <div class="flex items-center gap-2">
                        <FileInput
                          accept="image/*"
                          buttonLabel={t("guilds.create.bannerChoose")}
                          fileName={bannerFile()?.name}
                          onFile={(file) => {
                            setBannerFile(file);
                            setBannerPreviewUrl(URL.createObjectURL(file));
                            setBannerUrl("");
                            setBannerLoadError(false);
                          }}
                          disabled={isCreating()}
                          data-testid="create-guild-banner-file"
                        />
                        <Show when={bannerFile()}>
                          <button
                            type="button"
                            onClick={() => {
                              setBannerFile(null);
                              setBannerPreviewUrl("");
                            }}
                            class="text-xs text-text-secondary hover:text-text-primary transition-colors"
                          >
                            Clear File
                          </button>
                        </Show>
                      </div>
                      <div class="flex items-center gap-2">
                        <span class="text-xs text-text-secondary">OR</span>
                      </div>
                      <input
                        type="url"
                        placeholder={t("guilds.create.bannerUrlPlaceholder")}
                        value={bannerUrl()}
                        onInput={(e) => {
                          setBannerUrl(e.currentTarget.value);
                          setBannerFile(null);
                          setBannerPreviewUrl("");
                          setBannerLoadError(false);
                        }}
                        disabled={isCreating() || !!bannerFile()}
                        class="w-full px-3 py-1.5 text-sm rounded-lg bg-surface-layer1 border border-white/5 text-text-primary placeholder-text-placeholder focus:outline-none focus:border-accent-primary/50 state-disabled"
                      />
                    </div>
                    <Show when={isValidBannerUrl() || bannerPreviewUrl()}>
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
                            src={bannerPreviewUrl() || trimmedBannerUrl()}
                            alt="Banner preview"
                            class="w-full h-full object-cover"
                            onError={() => setBannerLoadError(true)}
                            onLoad={() => setBannerLoadError(false)}
                          />
                        </Show>
                      </div>
                    </Show>
                  </div>

                  {/* Preview Card */}
                  <div>
                    <div class="text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2">
                      Preview
                    </div>
                    <div class="rounded-xl border border-white/5 overflow-hidden bg-surface-layer2">
                      {/* Banner area */}
                      <div class="h-16 relative">
                        <Show
                          when={(isValidBannerUrl() || bannerPreviewUrl()) && !bannerLoadError()}
                          fallback={
                            <div
                              class="w-full h-full"
                              style={{
                                background:
                                  "linear-gradient(135deg, var(--color-accent-primary) 0%, var(--color-surface-layer1) 100%)",
                                opacity: "0.6",
                              }}
                            />
                          }
                        >
                          <img
                            src={bannerPreviewUrl() || trimmedBannerUrl()}
                            alt=""
                            class="w-full h-full object-cover"
                          />
                        </Show>
                        {/* Guild icon */}
                        <div class="absolute -bottom-4 left-3">
                          <div class="w-8 h-8 rounded-lg bg-surface-layer1 border-2 border-surface-layer2 flex items-center justify-center">
                            <span class="text-[10px] font-bold text-text-primary">
                              {initials()}
                            </span>
                          </div>
                        </div>
                      </div>
                      {/* Content */}
                      <div class="pt-5 px-3 pb-3">
                        <div class="text-xs font-semibold text-text-primary truncate">
                          {name().trim() || "Server Name"}
                        </div>
                        <Show when={tags().length > 0}>
                          <div class="flex flex-wrap gap-1 mt-1.5">
                            <For each={tags()}>
                              {(tag) => (
                                <span class="px-1.5 py-0.5 text-[10px] rounded bg-white/5 text-text-secondary">
                                  {tag}
                                </span>
                              )}
                            </For>
                          </div>
                        </Show>
                      </div>
                    </div>
                  </div>
                </Show>

                {/* Skip note */}
                <p class="text-xs text-text-secondary text-center">
                  You can always set this up later in Server Settings.
                </p>

                {/* Error Message */}
                <Show when={error()}>
                  <div
                    class="p-3 rounded-lg"
                    style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)"}}
                  >
                    <p class="text-sm" style={{"color":"var(--color-error-text)"}}>
                      {error()}
                    </p>
                  </div>
                </Show>
              </div>
            </div>

            {/* Footer - Step 2 */}
            <div class="flex items-center justify-between p-6 border-t border-white/10">
              <button
                type="button"
                onClick={handleBack}
                class="px-4 py-2 text-text-primary hover:bg-surface-layer2 rounded-lg transition-colors state-disabled-quiet"
                disabled={isCreating()}
              >
                Back
              </button>
              <Button
                type="submit"
                onClick={handleCreate}
                data-testid="create-guild-submit"
                disabled={isCreating()}
              >
                {isCreating() ? "Creating..." : "Create Server"}
              </Button>
            </div>
          </Show>
        </div>
      </div>
    </Portal>
  );
};

export default CreateGuildModal;
