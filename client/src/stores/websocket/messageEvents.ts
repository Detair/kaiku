/**
 * Message event handlers: edit application, OS notifications, and thread events.
 */

import type { Message, ThreadInfo } from "@/lib/types";
import { playNotification } from "@/lib/sound";
import type { MentionType, SoundEventType } from "@/lib/sound/types";
import type { NotificationContext } from "@/lib/notifications";

import { currentUser } from "../auth";
import { getChannel, channelsState } from "../channels";
import { dmsState } from "../dms";
import { guildsState } from "../guilds";
import {
  decryptMessageIfNeeded,
  messagesState,
  setMessagesState,
} from "../messages";
import {
  addThreadReply,
  clearThreadUnread,
  markThreadUnread,
  removeThreadReply,
  setThreadReadState,
  threadsState,
  updateParentThreadIndicator,
  updateThreadInfo,
} from "../threads";

/**
 * Handle notification sound for incoming message.
 */
export function handleMessageNotification(message: Message): void {
  const user = currentUser();

  // Don't notify for own messages
  if (user && message.author.id === user.id) {
    return;
  }

  // Don't notify for currently focused channel when the window is visible
  if (channelsState.selectedChannelId === message.channel_id && !document.hidden) {
    return;
  }

  if (dmsState.selectedDMId === message.channel_id && !dmsState.isShowingFriends && !document.hidden) {
    return;
  }

  // Determine if this is a DM
  const channel = getChannel(message.channel_id);
  const isDm =
    dmsState.dms.some((dm) => dm.id === message.channel_id) ||
    channel?.channel_type === "dm" ||
    channel?.guild_id === null;

  // Determine event type based on channel and mention
  let eventType: SoundEventType;
  if (isDm) {
    eventType = "message_dm";
  } else if (
    message.mention_type === "direct" ||
    message.mention_type === "everyone" ||
    message.mention_type === "here"
  ) {
    eventType = "message_mention";
  } else {
    eventType = "message_channel";
  }

  // Build notification context for OS notifications
  const guild = isDm ? null : guildsState.guilds.find((g) => g.id === channel?.guild_id);
  const notifCtx: NotificationContext = {
    username: message.author.display_name || message.author.username,
    content: message.encrypted ? null : message.content,
    guildName: guild?.name ?? null,
    channelName: channel?.name ?? null,
  };

  // Play notification with OS notification context
  playNotification(
    {
      type: eventType,
      channelId: message.channel_id,
      isDm,
      mentionType: message.mention_type as MentionType,
      authorId: message.author.id,
      content: message.content,
    },
    notifCtx,
  );
}

/**
 * Apply an edited message's content and timestamp, decrypting if needed
 * for E2EE channels.
 */
export async function applyMessageEdit(
  channelId: string,
  messageId: string,
  content: string,
  editedAt: string,
): Promise<void> {
  const messages = messagesState.byChannel[channelId];
  if (!messages) return;
  const index = messages.findIndex((m) => m.id === messageId);
  if (index === -1) return;

  const existingMsg = messages[index];
  let newContent = content;
  if (existingMsg.encrypted) {
    const decrypted = await decryptMessageIfNeeded({ ...existingMsg, content });
    newContent = decrypted.content;
  }

  // Re-find index after await — concurrent loadMessages can prepend and shift indices
  const current = messagesState.byChannel[channelId];
  if (!current) return;
  const freshIndex = current.findIndex((m) => m.id === messageId);
  if (freshIndex === -1) return;

  setMessagesState("byChannel", channelId, freshIndex, "content", newContent);
  setMessagesState("byChannel", channelId, freshIndex, "edited_at", editedAt);
}

// ============================================================================
// Thread event handlers
// ============================================================================

export function handleThreadReplyNew(
  channelId: string,
  parentId: string,
  message: Message,
  threadInfo: ThreadInfo,
): void {
  // Add reply to thread store
  addThreadReply(parentId, message);

  // Update thread info cache
  updateThreadInfo(parentId, threadInfo);

  // Mark thread as unread if not currently open and reply is from another user
  const user = currentUser();
  if (
    threadsState.activeThreadId !== parentId &&
    (!user || message.author.id !== user.id)
  ) {
    markThreadUnread(parentId);
  }

  // Update parent message's thread indicator in main messages store
  updateParentThreadIndicator(channelId, parentId, threadInfo);

  // Play notification for thread reply
  handleThreadNotification(message);
}

export function handleThreadReplyDelete(
  channelId: string,
  parentId: string,
  messageId: string,
  threadInfo: ThreadInfo,
): void {
  // Remove reply from thread store
  removeThreadReply(parentId, messageId);

  // Update thread info cache (updateThreadInfo preserves existing has_unread)
  updateThreadInfo(parentId, threadInfo);

  // Update parent message's thread indicator in main messages store
  updateParentThreadIndicator(channelId, parentId, threadInfo);

  // If the deleted message was the one that made the thread "read",
  // and there are still newer unread replies, the unread state is preserved
  // by updateThreadInfo's has_unread preservation logic.
}

export function handleThreadRead(
  parentId: string,
  lastReadMessageId: string | null,
): void {
  setThreadReadState(parentId, lastReadMessageId);
  clearThreadUnread(parentId);
}

function handleThreadNotification(message: Message): void {
  const user = currentUser();
  if (user && message.author.id === user.id) return;

  const channel = getChannel(message.channel_id);
  const isDm = channel?.channel_type === "dm" || channel?.guild_id === null;

  // Build notification context for OS notifications
  const guild = isDm ? null : guildsState.guilds.find((g) => g.id === channel?.guild_id);
  const notifCtx: NotificationContext = {
    username: message.author.display_name || message.author.username,
    content: message.encrypted ? null : message.content,
    guildName: guild?.name ?? null,
    channelName: channel?.name ?? null,
  };

  playNotification(
    {
      type: "message_thread",
      channelId: message.channel_id,
      isDm,
      mentionType: message.mention_type as MentionType,
      authorId: message.author.id,
      content: message.content,
    },
    notifCtx,
  );
}
