package io.wolftown.kaiku.domain.model

import io.wolftown.kaiku.data.KaikuJson
import kotlinx.serialization.SerializationException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class LenientEnumTest {

    private fun channel(type: String) = KaikuJson.decodeFromString<Channel>(
        """{"id":"c1","name":"n","channel_type":"$type"}"""
    )

    @Test
    fun `unknown channel_type decodes to UNKNOWN`() {
        assertEquals(ChannelType.UNKNOWN, channel("forum").channelType)
        assertEquals(ChannelType.UNKNOWN, channel("announcement").channelType)
    }

    @Test
    fun `known channel types still decode`() {
        assertEquals(ChannelType.TEXT, channel("text").channelType)
        assertEquals(ChannelType.VOICE, channel("voice").channelType)
        assertEquals(ChannelType.DM, channel("dm").channelType)
    }

    @Test
    fun `unknown user status decodes to UNKNOWN`() {
        val user = KaikuJson.decodeFromString<User>(
            """{"id":"u1","username":"a","display_name":"A","status":"invisible"}"""
        )
        assertEquals(UserStatus.UNKNOWN, user.status)
    }

    @Test
    fun `known user statuses keep their wire names`() {
        fun status(wire: String) = KaikuJson.decodeFromString<User>(
            """{"id":"u1","username":"a","display_name":"A","status":"$wire"}"""
        ).status
        assertEquals(UserStatus.ONLINE, status("online"))
        assertEquals(UserStatus.IDLE, status("away"))
        assertEquals(UserStatus.DND, status("busy"))
        assertEquals(UserStatus.OFFLINE, status("offline"))
    }

    @Test
    fun `known values encode to their wire name`() {
        assertEquals(""""voice"""", KaikuJson.encodeToString(ChannelType.serializer(), ChannelType.VOICE))
        assertEquals(""""away"""", KaikuJson.encodeToString(UserStatus.serializer(), UserStatus.IDLE))
    }

    @Test
    fun `UNKNOWN is never encoded`() {
        assertThrows(SerializationException::class.java) {
            KaikuJson.encodeToString(ChannelType.serializer(), ChannelType.UNKNOWN)
        }
    }
}
