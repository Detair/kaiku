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

interface EmojisTabProps {
  guildId: string;
}

const EmojisTab: Component<EmojisTabProps> = (props) => {
  const [isUploading, setIsUploading] = createSignal(false);
  const [uploadError, setUploadError] = createSignal<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = createSignal<string | null>(null);

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
      setUploadError(
        err instanceof Error ? err.message : "Failed to upload emoji",
      );
    } finally {
      setIsUploading(false);
    }
  };

  const handleDelete = async (emojiId: string) => {
    if (deleteConfirm() === emojiId) {
      try {
        await deleteEmoji(props.guildId, emojiId);
      } catch (err) {
        console.error("Failed to delete emoji:", err);
      }
      setDeleteConfirm(null);
    } else {
      setDeleteConfirm(emojiId);
      setTimeout(() => setDeleteConfirm(null), 3000);
    }
  };

  return (
    <div class="p-6">
      {/* Header */}
      <div class="flex items-center justify-between mb-6">
        <div>
          <h3 class="text-lg font-semibold text-text-primary">Emojis</h3>
          <p class="text-sm text-text-secondary">
            Upload custom emojis for your server. Maximum size:{" "}
            {getUploadLimitText("emoji")}
          </p>
        </div>

        <Show when={canManageEmojis()}>
          <label class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-primary text-white text-sm font-medium hover:bg-accent-primary/90 transition-colors cursor-pointer">
            <Show when={isUploading()} fallback={<Upload class="w-4 h-4" />}>
              <div class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
            </Show>
            Upload Emoji
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
        <div class="mb-4 p-3 bg-accent-danger/10 border border-accent-danger/20 rounded-lg text-sm text-accent-danger flex items-center justify-between">
          <span>{uploadError()}</span>
          <button onClick={() => setUploadError(null)}>
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
              No custom emojis yet
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
                  {emoji.animated ? "Animated" : "Static"}
                </div>
              </div>

              <Show when={canManageEmojis()}>
                <button
                  onClick={() => handleDelete(emoji.id)}
                  class="absolute top-2 right-2 p-1.5 rounded-lg opacity-0 group-hover:opacity-100 transition-opacity"
                  classList={{
                    "bg-accent-danger text-white opacity-100":
                      deleteConfirm() === emoji.id,
                    "bg-black/50 text-white hover:bg-accent-danger":
                      deleteConfirm() !== emoji.id,
                  }}
                  title="Delete Emoji"
                >
                  <Trash2 class="w-3.5 h-3.5" />
                </button>
              </Show>
            </div>
          )}
        </For>
      </div>
    </div>
  );
};

export default EmojisTab;
