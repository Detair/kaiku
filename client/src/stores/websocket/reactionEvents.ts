/**
 * Reaction and channel pin message-level event handlers.
 */

import { currentUser } from "../auth";
import { messagesState, setMessagesState } from "../messages";

export function handleReactionAdd(
  channelId: string,
  messageId: string,
  userId: string,
  emoji: string,
): void {
  const messages = messagesState.byChannel[channelId];
  if (!messages) return;

  const messageIndex = messages.findIndex((m) => m.id === messageId);
  if (messageIndex === -1) return;

  const message = messages[messageIndex];
  const reactions = message.reactions ? [...message.reactions] : [];

  // Find existing reaction for this emoji
  const reactionIndex = reactions.findIndex((r) => r.emoji === emoji);

  if (reactionIndex !== -1) {
    // Update existing reaction — increment count rather than deriving
    // from users.length to stay consistent with the server's count
    const reaction = { ...reactions[reactionIndex] };
    const users = reaction.users ?? [];
    if (!users.includes(userId)) {
      reaction.users = [...users, userId];
      reaction.count = (reaction.count ?? 0) + 1;
      const user = currentUser();
      if (user && userId === user.id) {
        reaction.me = true;
      }
      reactions[reactionIndex] = reaction;
    }
  } else {
    // Add new reaction
    const user = currentUser();
    reactions.push({
      emoji,
      count: 1,
      users: [userId],
      me: user ? userId === user.id : false,
    });
  }

  // Update the message in the store
  setMessagesState(
    "byChannel",
    channelId,
    messageIndex,
    "reactions",
    reactions,
  );
}

export function handleReactionRemove(
  channelId: string,
  messageId: string,
  userId: string,
  emoji: string,
): void {
  const messages = messagesState.byChannel[channelId];
  if (!messages) return;

  const messageIndex = messages.findIndex((m) => m.id === messageId);
  if (messageIndex === -1) return;

  const message = messages[messageIndex];
  if (!message.reactions) return;

  const reactions = [...message.reactions];
  const reactionIndex = reactions.findIndex((r) => r.emoji === emoji);

  if (reactionIndex === -1) return;

  // Decrement count rather than deriving from users.length to stay
  // consistent with the server's count
  const reaction = { ...reactions[reactionIndex] };
  const users = reaction.users ?? [];
  const wasTracked = users.includes(userId);
  reaction.users = users.filter((id) => id !== userId);

  // Only decrement if user was tracked in the array OR the array was
  // never populated (API-loaded reactions). Skip if the user is absent
  // from a populated array — that's a duplicate remove event.
  if (wasTracked || users.length === 0) {
    reaction.count = Math.max(0, (reaction.count ?? 1) - 1);
  }

  const user = currentUser();
  if (user && userId === user.id) {
    reaction.me = false;
  }

  if (reaction.count === 0) {
    reactions.splice(reactionIndex, 1);
  } else {
    reactions[reactionIndex] = reaction;
  }

  setMessagesState(
    "byChannel",
    channelId,
    messageIndex,
    "reactions",
    reactions.length > 0 ? reactions : undefined,
  );
}

// Channel pin event handler

export function updateMessagePinStatus(
  channelId: string,
  messageId: string,
  pinned: boolean,
): void {
  const messages = messagesState.byChannel[channelId];
  if (!messages) return;

  const messageIndex = messages.findIndex((m) => m.id === messageId);
  if (messageIndex === -1) return;

  setMessagesState(
    "byChannel",
    channelId,
    messageIndex,
    "pinned",
    pinned,
  );
}
