package io.wolftown.kaiku.domain

import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import org.junit.Assert.assertEquals
import org.junit.Test

class UnreadTrackingTest {

    private fun channel(id: String, unread: Int = 0) = Channel(
        id = id,
        name = "c$id",
        channelType = ChannelType.TEXT,
        unreadCount = unread,
    )

    @Test
    fun incrementUnread_bumpsOnlyTheTargetChannel() {
        val before = listOf(channel("a", 0), channel("b", 2))
        val after = GuildRepository.incrementUnread(before, "b")
        assertEquals(0, after.first { it.id == "a" }.unreadCount)
        assertEquals(3, after.first { it.id == "b" }.unreadCount)
    }

    @Test
    fun incrementUnread_unknownChannel_isNoOp() {
        val before = listOf(channel("a", 1))
        assertEquals(before, GuildRepository.incrementUnread(before, "missing"))
    }

    @Test
    fun clearUnread_resetsOnlyTheTargetChannel() {
        val before = listOf(channel("a", 5), channel("b", 3))
        val after = GuildRepository.clearUnread(before, "a")
        assertEquals(0, after.first { it.id == "a" }.unreadCount)
        assertEquals(3, after.first { it.id == "b" }.unreadCount)
    }

    @Test
    fun channel_defaultsUnreadToZero() {
        // Channel responses that omit unread fields must still parse/construct.
        assertEquals(0, channel("a").unreadCount)
    }
}
