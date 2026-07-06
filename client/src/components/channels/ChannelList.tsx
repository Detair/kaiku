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
} from "solid-js";
import { Plus, Mic, GripVertical } from "lucide-solid";
import {
  channelsState,
  selectChannel,
  moveChannel,
  moveChannelToCategory,
} from "@/stores/channels";
import {
  categoriesState,
  loadGuildCategories,
  getTopLevelCategories,
  getSubcategories,
  getCategory,
  isCategoryCollapsed,
  toggleCategoryCollapse,
  reorderCategories,
  isSubcategory as checkIsSubcategory,
} from "@/stores/categories";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
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
import flokiHappy from "@/assets/emotes/floki_emote_1.webp";
import {
  dragState,
  startDrag,
  setDropTarget,
  endDrag,
  getDragResult,
  calculateDropPosition,
  type DraggableType,
} from "./ChannelDragContext";

interface ChannelListProps {
  /** Called after a navigation action (e.g. channel selected). Used to close the mobile drawer. */
  onNavigate?: () => void;
}

const ChannelList: Component<ChannelListProps> = (props) => {
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

  const handleVoiceChannelClick = (channelId: string) => {
    // Just select the channel — VoiceChannelView handles join/leave
    selectChannel(channelId);
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
    e.stopPropagation(); // Prevent bubbling to category handler which would overwrite the drop target
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

    // Helper: check if a channel type is allowed in a category
    const isCategoryTypeAllowed = (channelId: string, targetCategoryId: string): boolean => {
      const channel = channelsState.channels.find((c) => c.id === channelId);
      const category = getCategory(targetCategoryId);
      if (!channel || !category || category.category_type === "mixed") return true;
      const allowed = category.category_type === channel.channel_type;
      if (!allowed) {
        showToast({
          type: "warning",
          title: "Cannot move channel",
          message: `${channel.channel_type === "voice" ? "Voice" : "Text"} channels can't be moved to a ${category.category_type}-only category.`,
          duration: 5000,
        });
      }
      return allowed;
    };

    // Handle the drop based on source and target types
    if (result.sourceType === "channel") {
      if (result.targetType === "channel") {
        // Channel dropped on channel - reorder (possibly across categories)
        const targetChannel = channelsState.channels.find((c) => c.id === result.targetId);
        if (targetChannel?.category_id && !isCategoryTypeAllowed(result.sourceId, targetChannel.category_id)) {
          endDrag();
          return;
        }
        await moveChannel(
          result.sourceId,
          result.targetId,
          result.position as "before" | "after",
        );
      } else if (result.targetType === "category") {
        // Channel dropped on category header - validate type
        if (!isCategoryTypeAllowed(result.sourceId, result.targetId)) {
          endDrag();
          return;
        }
        await moveChannelToCategory(
          result.sourceId,
          result.targetId,
          result.position === "before" ? "start" : "end",
        );
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
  const handleUncategorizedDrop = async (e: DragEvent) => {
    e.preventDefault();
    const result = getDragResult();

    if (result.sourceType === "channel" && result.sourceId) {
      await moveChannelToCategory(result.sourceId, null);
    }

    endDrag();
  };

  // Check drop position for an item
  const getDropPosition = (id: string, type: DraggableType) => {
    if (dragState.dropTargetId !== id || dragState.dropTargetType !== type) {
      return null;
    }
    return dragState.dropPosition;
  };

  // Check if item is being dragged
  const isDragging = (id: string): boolean => {
    return dragState.draggingId === id;
  };

  // Render a drop indicator slot — always in DOM, animates open when active
  const DropSlot = (props: { active: boolean }) => (
    <div
      class="overflow-hidden transition-all duration-150"
      style={{ height: props.active ? "8px" : "0px" }}
    >
      <div class="flex items-center h-full px-1">
        <div class="w-2.5 h-2.5 rounded-full shrink-0" style={{"background-color":"var(--color-accent-primary)"}} />
        <div class="flex-1 rounded-full" style={{"height":"3px","background-color":"var(--color-accent-primary)"}} />
      </div>
    </div>
  );

  // ============================================================================
  // Render Functions
  // ============================================================================

  // Render a single channel (text or voice) with drag support
  // Single container handles ALL drag events + contains DropSlots
  const renderChannel = (
    channel: ChannelWithUnread,
    categoryId: string | null,
  ) => {
    const isVoice = channel.channel_type === "voice";
    const draggable = canManageChannels();
    const dropPos = () => getDropPosition(channel.id, "channel");
    const dragging = () => isDragging(channel.id);

    return (
      <div
        draggable={draggable}
        onDragStart={(e) => handleDragStart(e, channel.id, "channel")}
        onDragEnd={handleDragEnd}
        onDragOver={(e) => handleChannelDragOver(e, channel.id, categoryId)}
        onDragLeave={handleDragLeave}
        onDrop={(e) => { e.stopPropagation(); handleDrop(e); }}
      >
        <DropSlot active={dropPos() === "before"} />
        <div
          class="transition-all duration-150"
          classList={{
            "opacity-30 border border-dashed border-white/20 rounded-lg": dragging(),
          }}
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
                    : () => { selectChannel(channel.id); props.onNavigate?.(); }
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
        <DropSlot active={dropPos() === "after"} />
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
    const dropPos = () => getDropPosition(subcategory.id, "category");
    const dragging = () => isDragging(subcategory.id);

    return (
      <div
        draggable={draggable}
        onDragStart={(e) => handleDragStart(e, subcategory.id, "category")}
        onDragEnd={handleDragEnd}
        onDragOver={(e) => handleCategoryDragOver(e, subcategory.id, true)}
        onDragLeave={handleDragLeave}
        onDrop={(e) => { e.stopPropagation(); handleDrop(e); }}
      >
        <DropSlot active={dropPos() === "before"} />
        <div
          class="mt-1 transition-all duration-150"
          classList={{
            "opacity-30 border border-dashed border-white/20 rounded-lg": dragging(),
            "bg-accent-primary/10 ring-2 ring-accent-primary/30 rounded-lg": dropPos() === "inside",
          }}
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
                categoryType={subcategory.category_type}
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
        <DropSlot active={dropPos() === "after"} />
      </div>
    );
  };

  // Render a top-level category with subcategories and channels
  const renderCategory = (category: ChannelCategory) => {
    const guildId = guildsState.activeGuildId;
    const subcategories = guildId ? getSubcategories(guildId, category.id) : [];
    const isCollapsed = () => isCategoryCollapsed(category.id);
    const draggable = canManageChannels();
    const dropPos = () => getDropPosition(category.id, "category");
    const dragging = () => isDragging(category.id);

    return (
      <div
        draggable={draggable}
        onDragStart={(e) => handleDragStart(e, category.id, "category")}
        onDragEnd={handleDragEnd}
        onDragOver={(e) => handleCategoryDragOver(e, category.id, false)}
        onDragLeave={handleDragLeave}
        onDrop={(e) => { e.stopPropagation(); handleDrop(e); }}
      >
        <DropSlot active={dropPos() === "before"} />
        <div
          class="mb-2 transition-all duration-150"
          classList={{
            "opacity-30 border border-dashed border-white/20 rounded-lg": dragging(),
            "bg-accent-primary/10 ring-2 ring-accent-primary/30 rounded-lg": dropPos() === "inside",
          }}
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
                categoryType={category.category_type}
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
        <DropSlot active={dropPos() === "after"} />
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
                title="Test Microphone" aria-label="Test Microphone"
                onClick={() => setShowMicTest(true)}
              >
                <Mic class="w-4 h-4" />
              </button>
              <button
                class="p-1 text-text-secondary hover:text-text-primary rounded-lg hover:bg-white/10 transition-all duration-200"
                data-testid="create-channel-button"
                title="Create Channel" aria-label="Create Channel"
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
            title="Test Microphone" aria-label="Test Microphone"
            onClick={() => setShowMicTest(true)}
          >
            <Mic class="w-5 h-5" />
          </button>
          <Show when={canManageChannels()}>
            <button
              class="p-2 text-text-secondary hover:text-text-primary rounded-lg hover:bg-white/10 transition-all duration-200"
              data-testid="create-channel-button"
              title="Create Channel" aria-label="Create Channel"
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
          <img src={flokiHappy} alt="" class="w-10 h-10 mx-auto mb-1 object-contain" loading="lazy" />
          No channels yet
        </div>
      </Show>

      {/* Error state */}
      <Show when={channelsState.error}>
        <div
          class="px-2 py-4 text-center text-sm"
          style={{"color":"var(--color-error-text)"}}
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
