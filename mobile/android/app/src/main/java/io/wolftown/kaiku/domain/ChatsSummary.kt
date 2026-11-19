package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.DmConversation

/** The start page's "Chats" entry: all unread direct messages and the newest conversation. */
data class ChatsSummary(val unread: Int = 0, val newest: DmConversation? = null)

/** [conversations] must be ordered newest first (as `DmRepository` keeps them). */
fun chatsSummary(conversations: List<DmConversation>): ChatsSummary = ChatsSummary(
    unread = conversations.sumOf { it.unreadCount.coerceAtLeast(0) },
    newest = conversations.firstOrNull { it.lastMessage != null },
)
