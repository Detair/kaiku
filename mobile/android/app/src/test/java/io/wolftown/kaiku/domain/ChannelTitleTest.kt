package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ChannelTitleTest {

    private val channels = listOf(
        Channel(id = "ch-1", name = "general", channelType = ChannelType.TEXT),
    )
    private val dms = listOf(
        DmConversation(
            id = "dm-1",
            participants = listOf(
                DmParticipant(userId = "me", username = "me", displayName = "Me"),
                DmParticipant(userId = "u2", username = "alice", displayName = "Alice"),
            ),
        ),
    )

    @Test
    fun `guild channel title uses channel name`() {
        val title = resolveChannelTitle("ch-1", channels, dms, "me")

        assertEquals("general", title?.text)
        assertFalse(title!!.isDm)
    }

    @Test
    fun `dm title uses other participant display name`() {
        val title = resolveChannelTitle("dm-1", channels, dms, "me")

        assertEquals("Alice", title?.text)
        assertTrue(title!!.isDm)
    }

    @Test
    fun `unknown channel yields null title`() {
        assertNull(resolveChannelTitle("nope", channels, dms, "me"))
    }
}
