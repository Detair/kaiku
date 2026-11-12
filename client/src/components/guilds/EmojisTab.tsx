/**
 * EmojisTab - Emoji management
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Trash2, Upload, X } from "lucide-solid";
import {
  emojiState,
  loadGuildEmojis,
  uploadEmoji,
  deleteEmoji,
} from "@/stores/emoji";
import { authState } from "@/stores/auth";
import { isGuildOwner } from "@/stores/guilds";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import { validateFileSize, getUploadLimitText } from "@/lib/tauri";
import { t } from "@/i18n";
import { actionErrorText } from "@/components/admin/actionErrors";
import ConfirmActionDialog from "@/components/admin/ConfirmActionDialog";
import type { GuildEmoji } from "@/lib/types";

interface EmojisTabProps {
  guildId: string;
}

const EmojisTab: Component<EmojisTabProps> = (props) => {
  const [isUploading, setIsUploading] = createSignal(false);
  const [uploadError, setUploadError] = createSignal<string | null>(null);
  const [deleting, setDeleting] = createSignal<GuildEmoji | null>(null);

  onMount(() => {
    loadGuildEmojis(props.guildId);
  });

  const emojis = () => emojiState.guildEmojis[props.guildId] || [];
  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");
  const canManageEmojis = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_EMOJIS_AND_STICKERS,
    );

  const handleFileSelect = async (e: Event) => {
    const input = e.target as HTMLInputElement;
    if (!input.files?.length) return;

    const file = input.files[0];

    setUploadError(null);

    // Frontend validation before upload
    const validationError = validateFileSize(file, "emoji");
    if (validationError) {
      setUploadError(validationError);
      input.value = ""; // Clear selection
      return;
    }

    const name = file.name.split(".")[0].replace(/[^a-zA-Z0-9_]/g, "_");
    setIsUploading(true);

    try {
      await uploadEmoji(props.guildId, name, file);
      input.value = ""; // Reset input
    } catch (err) {
      console.error("Failed to upload emoji:", err);
      setUploadError(`${t("guildSettings.emojis.uploadFailed")} ${actionErrorText(err)}`);
    } finally {
      setIsUploading(false);
    }
  };


  return (
    <div class="p-6">
      {/* Header */}
      <div class="flex items-center justify-between mb-6">
        <div>
          <h3 class="text-lg font-semibold text-text-primary">{t("guildSettings.emojis.title")}</h3>
          <p class="text-sm text-text-secondary">
            {t("guildSettings.emojis.hint", { size: getUploadLimitText("emoji") })}
          </p>
        </div>

        <Show when={canManageEmojis()}>
          <label class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90 transition-colors cursor-pointer">
            <Show when={isUploading()} fallback={<Upload class="w-4 h-4" />}>
              <div class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
            </Show>
            {t("guildSettings.emojis.upload")}
            <input
              type="file"
              accept="image/png,image/jpeg,image/gif,image/webp"
              class="hidden"
              onChange={handleFileSelect}
              disabled={isUploading()}
            />
          </label>
        </Show>
      </div>

      <Show when={uploadError()}>
        <div role="alert" class="mb-4 p-3 bg-status-error/15 border border-status-error/50 rounded-lg text-sm text-text-primary flex items-center justify-between">
          <span>{uploadError()}</span>
          <button onClick={() => setUploadError(null)} aria-label={t("common.close")}>
            <X class="w-4 h-4" />
          </button>
        </div>
      </Show>

      {/* Emoji Grid */}
      <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
        <For
          each={emojis()}
          fallback={
            <div class="col-span-full py-8 text-center text-text-secondary border border-dashed border-white/10 rounded-xl">
              {t("guildSettings.emojis.empty")}
            </div>
          }
        >
          {(emoji) => (
            <div class="group relative p-3 rounded-xl bg-surface-layer1 border border-white/5 hover:border-white/10 transition-colors flex items-center gap-3">
              <div class="w-10 h-10 flex-shrink-0">
                <img
                  src={emoji.image_url}
                  alt={emoji.name}
                  class="w-full h-full object-contain"
                />
              </div>
              <div class="flex-1 min-w-0">
                <div
                  class="font-medium text-text-primary truncate"
                  title={emoji.name}
                >
                  :{emoji.name}:
                </div>
                <div class="text-xs text-text-secondary">
                  {emoji.animated ? t("guildSettings.emojis.animated") : t("guildSettings.emojis.still")}
                </div>
              </div>

              <Show when={canManageEmojis()}>
                <button
                  onClick={() => setDeleting(emoji)}
                  class="absolute top-2 right-2 p-1.5 rounded-lg opacity-0 group-hover:opacity-100 focus-visible:opacity-100 touch:opacity-100 transition-opacity bg-black/50 text-white hover:bg-accent-danger-fill"
                  title={t("guildSettings.emojis.deleteNamed", { name: emoji.name })}
                  aria-label={t("guildSettings.emojis.deleteNamed", { name: emoji.name })}
                >
                  <Trash2 class="w-3.5 h-3.5" />
                </button>
              </Show>
            </div>
          )}
        </For>
      </div>

      <Show when={deleting()}>
        {(emoji) => (
          <ConfirmActionDialog
            title={t("guildSettings.emojis.deleteTitle", { name: emoji().name })}
            message={t("guildSettings.emojis.deleteBody", { name: emoji().name })}
            confirmLabel={t("guildSettings.emojis.delete")}
            danger
            onConfirm={() => deleteEmoji(props.guildId, emoji().id)}
            onDone={() => setDeleting(null)}
            onCancel={() => setDeleting(null)}
          />
        )}
      </Show>
    </div>
  );
};

export default EmojisTab;
