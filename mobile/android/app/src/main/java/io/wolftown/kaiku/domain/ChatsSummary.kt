package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.DmConversation

/**
 * The start page's "Chats" entry: the number of all unread direct messages
 * and the conversation to preview. The preview is the conversation with the
 * latest activity, the same one the chat list shows first (also one without
 * messages yet: "No messages yet"); the number is counted over all
 * conversations, independently of which one is previewed.
 */
data class ChatsSummary(val unread: Int = 0, val newest: DmConversation? = null)

/** [conversations] must be ordered newest first (as `DmRepository` keeps them). */
fun chatsSummary(conversations: List<DmConversation>): ChatsSummary = ChatsSummary(
    unread = conversations.sumOf { it.unreadCount.coerceAtLeast(0) },
    newest = conversations.firstOrNull(),
)
