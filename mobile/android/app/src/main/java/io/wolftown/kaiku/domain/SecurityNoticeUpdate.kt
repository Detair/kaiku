package io.wolftown.kaiku.domain

import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.model.Message

/** C28 coalescing: `security_notice_updated` replaces the notice of that message (count, events, last_at). */
fun applySecurityNoticeUpdate(messages: List<Message>, e: ServerEvent.SecurityNoticeUpdated): List<Message> {
    if (messages.none { it.id == e.messageId }) return messages
    return messages.map { m -> if (m.id == e.messageId) m.copy(securityNotice = e.securityNotice) else m }
}
