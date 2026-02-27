/**
 * ChannelList - Guild channel sidebar with collapsible categories and drag-and-drop
 *
 * Displays channels organized into:
 * - Top-level categories (ALL CAPS headers)
 * - Subcategories (indented with border)
 * - Channels within categories (text and voice)
 * - Uncategorized channels at the bottom
 *
 * Supports drag-and-drop for:
 * - Reordering channels within a category
 * - Moving channels between categories
 * - Reordering categories
 * - Nesting categories (2-level max)
 */

import {
  Component,
  For,
  Show,
  createSignal,
  createEffect,
  createMemo,
  onCleanup,
} from "solid-js";
import { Plus, Mic, GripVertical } from "lucide-solid";
import {
  channelsState,
  selectChannel,
  moveChannel,
  moveChannelToCategory,
  markChannelAsRead,
  getUnreadCount,
} from "@/stores/channels";
import {
  categoriesState,
  loadGuildCategories,
  getTopLevelCategories,
  getSubcategories,
  isCategoryCollapsed,
  toggleCategoryCollapse,
  reorderCategories,
  isSubcategory as checkIsSubcategory,
} from "@/stores/categories";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import { joinVoice, leaveVoice, isInChannel } from "@/stores/voice";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import type { ChannelWithUnread, ChannelCategory } from "@/lib/types";
import { showToast } from "@/components/ui/Toast";
import CategoryHeader from "./CategoryHeader";
import ChannelItem from "./ChannelItem";
import CreateChannelModal from "./CreateChannelModal";
import ChannelSettingsModal from "./ChannelSettingsModal";
import MicrophoneTest from "../voice/MicrophoneTest";
import VoiceParticipants from "../voice/VoiceParticipants";
import {
  dragState,
  startDrag,
  setDropTarget,
  endDrag,
  getDragResult,
  calculateDropPosition,
  type DraggableType,
} from "./ChannelDragContext";

const ChannelList: Component = () => {
  const [showMicTest, setShowMicTest] = createSignal(false);
  const [showCreateModal, setShowCreateModal] = createSignal(false);
  const [createModalType, setCreateModalType] = createSignal<"text" | "voice">(
    "text",
  );
  const [createModalCategoryId, setCreateModalCategoryId] = createSignal<
    string | null
  >(null);
  const [settingsChannelId, setSettingsChannelId] = createSignal<string | null>(
    null,
  );

  // Load categories when guild changes
  createEffect(() => {
    const guildId = guildsState.activeGuildId;
    if (guildId) {
      loadGuildCategories(guildId);
    }
  });

  // Auto-mark selected channel as read after a short delay
  createEffect(() => {
    const channelId = channelsState.selectedChannelId;
    if (channelId && getUnreadCount(channelId) > 0) {
      const timer = setTimeout(() => {
        markChannelAsRead(channelId);
      }, 1000);
      onCleanup(() => clearTimeout(timer));
    }
  });

  // Get active guild for favorites
  const activeGuild = () => {
    const guildId = guildsState.activeGuildId;
    if (!guildId) return null;
    return guildsState.guilds.find((g) => g.id === guildId) ?? null;
  };

  // Check if current user can manage channels
  const canManageChannels = () => {
    const guildId = guildsState.activeGuildId;
    const userId = authState.user?.id;
    if (!guildId || !userId) return false;

    const isOwner = isGuildOwner(guildId, userId);
    return (
      isOwner ||
      memberHasPermission(
        guildId,
        userId,
        isOwner,
        PermissionBits.MANAGE_CHANNELS,
      )
    );
  };

  // Get top-level categories for active guild
  const topLevelCategories = createMemo(() => {
    const guildId = guildsState.activeGuildId;
    if (!guildId) return [];
    return getTopLevelCategories(guildId);
  });

  // Get channels grouped by category
  const channelsByCategory = createMemo(() => {
    const result: Record<string, ChannelWithUnread[]> = {};
    const uncategorized: ChannelWithUnread[] = [];

    for (const channel of channelsState.channels) {
      if (channel.category_id) {
        if (!result[channel.category_id]) {
          result[channel.category_id] = [];
        }
        result[channel.category_id].push(channel);
      } else {
        uncategorized.push(channel);
      }
    }

    // Sort channels within each category by position
    for (const categoryId of Object.keys(result)) {
      result[categoryId].sort((a, b) => a.position - b.position);
    }
    uncategorized.sort((a, b) => a.position - b.position);

    return { byCategory: result, uncategorized };
  });

  // Get channels for a specific category
  const getChannelsForCategory = (categoryId: string): ChannelWithUnread[] => {
    return channelsByCategory().byCategory[categoryId] ?? [];
  };

  // Get uncategorized channels
  const uncategorizedChannels = () => channelsByCategory().uncategorized;

  // Check if a category has any unread channels
  const categoryHasUnread = (categoryId: string): boolean => {
    const channels = getChannelsForCategory(categoryId);
    return channels.some(
      (c) => c.channel_type === "text" && c.unread_count > 0,
    );
  };

  const handleVoiceChannelClick = async (channelId: string) => {
    if (isInChannel(channelId)) {
      await leaveVoice();
    } else {
      try {
        await joinVoice(channelId);
      } catch (err) {
        console.error("Failed to join voice:", err);
        showToast({
          type: "error",
          title: "Could not join voice channel. Please try again.",
          duration: 8000,
        });
      }
    }
  };

  const openCreateModal = (
    type: "text" | "voice",
    categoryId: string | null = null,
  ) => {
    setCreateModalType(type);
    setCreateModalCategoryId(categoryId);
    setShowCreateModal(true);
  };

  const handleChannelCreated = (channelId: string) => {
    if (createModalType() === "text") {
      selectChannel(channelId);
    }
  };

  // ============================================================================
  // Drag and Drop Handlers
  // ============================================================================

  const handleDragStart = (e: DragEvent, id: string, type: DraggableType) => {
    if (!canManageChannels()) {
      e.preventDefault();
      return;
    }

    e.dataTransfer?.setData("text/plain", JSON.stringify({ id, type }));
    e.dataTransfer!.effectAllowed = "move";
    startDrag(id, type);

    // Add drag image styling
    const target = e.currentTarget as HTMLElement;
    target.style.opacity = "0.5";
  };

  const handleDragEnd = (e: DragEvent) => {
    const target = e.currentTarget as HTMLElement;
    target.style.opacity = "1";
    endDrag();
  };

  const handleChannelDragOver = (
    e: DragEvent,
    channelId: string,
    _categoryId: string | null,
  ) => {
    e.preventDefault();
    if (!canManageChannels()) return;
    if (!dragState.isDragging) return;

    const position = calculateDropPosition(
      e,
      e.currentTarget as HTMLElement,
      "channel",
    );
    setDropTarget(channelId, "channel", position);
  };

  const handleCategoryDragOver = (
    e: DragEvent,
    categoryId: string,
    isSubcat: boolean,
  ) => {
    e.preventDefault();
    if (!canManageChannels()) return;
    if (!dragState.isDragging) return;

    // For categories, allow "inside" only for non-subcategories
    let position = calculateDropPosition(
      e,
      e.currentTarget as HTMLElement,
      "category",
    );

    // Can't drop inside a subcategory (2-level max)
    if (position === "inside" && isSubcat) {
      position = "after";
    }

    // Can't drop a subcategory inside another category if it would exceed 2 levels
    if (
      dragState.draggingType === "category" &&
      position === "inside" &&
      checkIsSubcategory(dragState.draggingId!)
    ) {
      position = "after";
    }

    setDropTarget(categoryId, "category", position);
  };

  const handleDragLeave = (e: DragEvent) => {
    // Only clear if leaving to outside (not to a child element)
    const relatedTarget = e.relatedTarget as HTMLElement | null;
    const currentTarget = e.currentTarget as HTMLElement;
    if (!relatedTarget || !currentTarget.contains(relatedTarget)) {
      // Don't clear immediately - let dragover on the next element set the new target
    }
  };

  const handleDrop = async (e: DragEvent) => {
    e.preventDefault();
    const result = getDragResult();

    if (!result.sourceId || !result.targetId) {
      endDrag();
      return;
    }

    const guildId = guildsState.activeGuildId;
    if (!guildId) {
      endDrag();
      return;
    }

    // Handle the drop based on source and target types
    if (result.sourceType === "channel") {
      if (result.targetType === "channel") {
        // Channel dropped on channel - reorder
        moveChannel(
          result.sourceId,
          result.targetId,
          result.position as "before" | "after",
        );
      } else if (result.targetType === "category") {
        // Channel dropped on category - move to that category
        if (result.position === "inside") {
          moveChannelToCategory(result.sourceId, result.targetId);
        }
      }
    } else if (result.sourceType === "category") {
      if (result.targetType === "category" && result.position) {
        // Category dropped on category - reorder or nest
        await reorderCategories(
          guildId,
          result.sourceId,
          result.targetId,
          result.position,
        );
      }
    }

    endDrag();
  };

  // Handle drop on uncategorized section
  const handleUncategorizedDrop = (e: DragEvent) => {
    e.preventDefault();
    const result = getDragResult();

    if (result.sourceType === "channel" && result.sourceId) {
      moveChannelToCategory(result.sourceId, null);
    }

    endDrag();
  };

  // Get drop indicator classes
  const getDropIndicatorClasses = (id: string, type: DraggableType): string => {
    if (dragState.dropTargetId !== id || dragState.dropTargetType !== type) {
      return "";
    }

    switch (dragState.dropPosition) {
      case "before":
        return "border-t-2 border-accent-primary";
      case "after":
        return "border-b-2 border-accent-primary";
      case "inside":
        return "bg-accent-primary/10 ring-2 ring-accent-primary/30";
      default:
        return "";
    }
  };

  // Check if item is being dragged
  const isDragging = (id: string): boolean => {
    return dragState.draggingId === id;
  };

  // ============================================================================
  // Render Functions
  // ============================================================================

  // Render a single channel (text or voice) with drag support
  const renderChannel = (
    channel: ChannelWithUnread,
    categoryId: string | null,
  ) => {
    const isVoice = channel.channel_type === "voice";
    const draggable = canManageChannels();

    return (
      <div
        class={`transition-all duration-150 ${getDropIndicatorClasses(channel.id, "channel")} ${
          isDragging(channel.id) ? "opacity-50" : ""
        }`}
        draggable={draggable}
        onDragStart={(e) => handleDragStart(e, channel.id, "channel")}
        onDragEnd={handleDragEnd}
        onDragOver={(e) => handleChannelDragOver(e, channel.id, categoryId)}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <div class="flex items-center group">
          <Show when={draggable}>
            <div class="cursor-grab text-text-secondary hover:text-text-primary opacity-0 group-hover:opacity-100 transition-opacity mr-1">
              <GripVertical class="w-3 h-3" />
            </div>
          </Show>
          <div class="flex-1">
            <ChannelItem
              channel={channel}
              isSelected={
                !isVoice && channelsState.selectedChannelId === channel.id
              }
              onClick={
                isVoice
                  ? () => handleVoiceChannelClick(channel.id)
                  : () => selectChannel(channel.id)
              }
              onSettings={
                canManageChannels()
                  ? () => setSettingsChannelId(channel.id)
                  : undefined
              }
              guildId={activeGuild()?.id}
              guildName={activeGuild()?.name}
              guildIcon={activeGuild()?.icon_url}
            />
          </div>
        </div>
        <Show when={isVoice}>
          <VoiceParticipants channelId={channel.id} />
        </Show>
      </div>
    );
  };

  // Render channels list for a category
  const renderCategoryChannels = (categoryId: string) => {
    const channels = getChannelsForCategory(categoryId);
    return (
      <Show when={channels.length > 0}>
        <div class="space-y-0.5">
          <For each={channels}>
            {(channel) => renderChannel(channel, categoryId)}
          </For>
        </div>
      </Show>
    );
  };

  // Render a subcategory with its channels and drag support
  const renderSubcategory = (subcategory: ChannelCategory) => {
    const isCollapsed = () => isCategoryCollapsed(subcategory.id);
    const draggable = canManageChannels();

    return (
      <div
        class={`mt-1 transition-all duration-150 ${getDropIndicatorClasses(subcategory.id, "category")} ${
          isDragging(subcategory.id) ? "opacity-50" : ""
        }`}
        draggable={draggable}
        onDragStart={(e) => handleDragStart(e, subcategory.id, "category")}
        onDragEnd={handleDragEnd}
        onDragOver={(e) => handleCategoryDragOver(e, subcategory.id, true)}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <div class="flex items-center group">
          <Show when={draggable}>
            <div class="cursor-grab text-text-secondary hover:text-text-primary opacity-0 group-hover:opacity-100 transition-opacity">
              <GripVertical class="w-3 h-3" />
            </div>
          </Show>
          <div class="flex-1">
            <CategoryHeader
              id={subcategory.id}
              name={subcategory.name}
              collapsed={isCollapsed()}
              hasUnread={categoryHasUnread(subcategory.id)}
              isSubcategory={true}
              onToggle={() => toggleCategoryCollapse(subcategory.id)}
              onCreateChannel={
                canManageChannels()
                  ? () => openCreateModal("text", subcategory.id)
                  : undefined
              }
            />
          </div>
        </div>
        <Show when={!isCollapsed()}>
          <div class="ml-3 border-l-2 border-white/10 pl-1">
            {renderCategoryChannels(subcategory.id)}
          </div>
        </Show>
      </div>
    );
  };

  // Render a top-level category with subcategories and channels
  const renderCategory = (category: ChannelCategory) => {
    const guildId = guildsState.activeGuildId;
    const subcategories = guildId ? getSubcategories(guildId, category.id) : [];
    const isCollapsed = () => isCategoryCollapsed(category.id);
    const draggable = canManageChannels();

    return (
      <div
        class={`mb-2 transition-all duration-150 ${getDropIndicatorClasses(category.id, "category")} ${
          isDragging(category.id) ? "opacity-50" : ""
        }`}
        draggable={draggable}
        onDragStart={(e) => handleDragStart(e, category.id, "category")}
        onDragEnd={handleDragEnd}
        onDragOver={(e) => handleCategoryDragOver(e, category.id, false)}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <div class="flex items-center group">
          <Show when={draggable}>
            <div class="cursor-grab text-text-secondary hover:text-text-primary opacity-0 group-hover:opacity-100 transition-opacity">
              <GripVertical class="w-3 h-3" />
            </div>
          </Show>
          <div class="flex-1">
            <CategoryHeader
              id={category.id}
              name={category.name}
              collapsed={isCollapsed()}
              hasUnread={categoryHasUnread(category.id)}
              isSubcategory={false}
              onToggle={() => toggleCategoryCollapse(category.id)}
              onCreateChannel={
                canManageChannels()
                  ? () => openCreateModal("text", category.id)
                  : undefined
              }
            />
          </div>
        </div>
        <Show when={!isCollapsed()}>
          <div class="space-y-0.5 mt-0.5">
            {/* Direct channels in this category */}
            {renderCategoryChannels(category.id)}

            {/* Subcategories */}
            <For each={subcategories}>
              {(subcategory) => renderSubcategory(subcategory)}
            </For>
          </div>
        </Show>
      </div>
    );
  };

  return (
    <nav class="flex-1 overflow-y-auto px-2 py-2">
      {/* Categorized channels */}
      <For each={topLevelCategories()}>
        {(category) => renderCategory(category)}
      </For>

      {/* Uncategorized channels */}
      <Show when={uncategorizedChannels().length > 0}>
        <Show when={topLevelCategories().length > 0}>
          <div class="mx-3 my-2 border-t border-white/10" />
        </Show>

        {/* Uncategorized section header with mic test and create buttons */}
        <div
          class={`mb-2 transition-all duration-150 ${
            dragState.isDragging && dragState.draggingType === "channel"
              ? "ring-2 ring-accent-primary/20 rounded-lg"
              : ""
          }`}
          onDragOver={(e) => {
            e.preventDefault();
            if (dragState.draggingType === "channel") {
              setDropTarget("uncategorized", "category", "inside");
            }
          }}
          onDrop={handleUncategorizedDrop}
        >
          <div class="flex items-center justify-between px-2 py-1 mb-1 rounded-lg hover:bg-white/5 transition-colors group">
            <span class="text-xs font-bold text-text-secondary uppercase tracking-wider group-hover:text-text-primary transition-colors">
              Uncategorized
            </span>
            <div class="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                class="p-1 text-text-secondary hover:text-accent-primary rounded-lg hover:bg-white/10 transition-all duration-200"
                title="Test Microphone"
                onClick={() => setShowMicTest(true)}
              >
                <Mic class="w-4 h-4" />
              </button>
              <button
                class="p-1 text-text-secondary hover:text-text-primary rounded-lg hover:bg-white/10 transition-all duration-200"
                title="Create Channel"
                onClick={() => openCreateModal("text", null)}
              >
                <Plus class="w-4 h-4" />
              </button>
            </div>
          </div>
          <div class="space-y-0.5">
            <For each={uncategorizedChannels()}>
              {(channel) => renderChannel(channel, null)}
            </For>
          </div>
        </div>
      </Show>

      {/* Show mic test and create buttons when there are no categories */}
      <Show
        when={
          topLevelCategories().length === 0 &&
          uncategorizedChannels().length === 0
        }
      >
        <div class="flex items-center justify-center gap-2 py-4">
          <button
            class="p-2 text-text-secondary hover:text-accent-primary rounded-lg hover:bg-white/10 transition-all duration-200"
            title="Test Microphone"
            onClick={() => setShowMicTest(true)}
          >
            <Mic class="w-5 h-5" />
          </button>
          <Show when={canManageChannels()}>
            <button
              class="p-2 text-text-secondary hover:text-text-primary rounded-lg hover:bg-white/10 transition-all duration-200"
              title="Create Channel"
              onClick={() => openCreateModal("text", null)}
            >
              <Plus class="w-5 h-5" />
            </button>
          </Show>
        </div>
      </Show>

      {/* Empty state */}
      <Show
        when={
          !channelsState.isLoading &&
          !categoriesState.isLoading &&
          channelsState.channels.length === 0 &&
          topLevelCategories().length === 0 &&
          !channelsState.error
        }
      >
        <div class="px-2 py-4 text-center text-text-secondary text-sm">
          No channels yet
        </div>
      </Show>

      {/* Error state */}
      <Show when={channelsState.error}>
        <div
          class="px-2 py-4 text-center text-sm"
          style="color: var(--color-error-text)"
        >
          {channelsState.error}
        </div>
      </Show>

      {/* Microphone Test Modal */}
      <Show when={showMicTest()}>
        <MicrophoneTest onClose={() => setShowMicTest(false)} />
      </Show>

      {/* Create Channel Modal */}
      <Show when={showCreateModal() && guildsState.activeGuildId}>
        <CreateChannelModal
          guildId={guildsState.activeGuildId!}
          initialType={createModalType()}
          categoryId={createModalCategoryId()}
          onClose={() => setShowCreateModal(false)}
          onCreated={handleChannelCreated}
        />
      </Show>

      {/* Channel Settings Modal */}
      <Show when={settingsChannelId() && guildsState.activeGuildId}>
        <ChannelSettingsModal
          channelId={settingsChannelId()!}
          guildId={guildsState.activeGuildId!}
          onClose={() => setSettingsChannelId(null)}
        />
      </Show>
    </nav>
  );
};

export default ChannelList;
