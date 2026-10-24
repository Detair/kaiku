package io.wolftown.kaiku.data.api

import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WsJson
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.UserStatus
import io.wolftown.kaiku.testutil.Golden
import kotlinx.serialization.json.decodeFromJsonElement
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.test.assertIs

/**
 * Decodes the golden JSON fixtures generated from the server's own types
 * (`server/tests/integration/android_contract_fixtures.rs`). A failure here
 * means the Android models drifted from the real wire format.
 */
class GoldenContractTest {

    @Test
    fun `messages page fixture decodes as CursorPage`() {
        val page = KaikuJson.decodeFromString<CursorPage<Message>>(Golden.load("messages_page.json"))

        assertEquals(2, page.items.size)
        assertTrue(page.hasMore)
        assertEquals(page.items[1].id, page.nextCursor)

        // Server order is newest first.
        val (newer, older) = page.items
        assertEquals("2026-03-12T10:01:00Z", newer.createdAt)
        assertEquals("2026-03-12T10:00:00.123456Z", older.createdAt)
        assertEquals(UserStatus.ONLINE, newer.author.status)

        val attachment = page.items.flatMap { it.attachments }.single()
        assertEquals("/api/messages/attachments/${attachment.id}/download", attachment.url)
        assertNotNull(attachment.thumbnailUrl)
    }

    @Test
    fun `ws message_new fixture decodes and its message decodes`() {
        val event = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_message_new.json"))
        assertIs<ServerEvent.MessageNew>(event)

        val message = KaikuJson.decodeFromJsonElement<Message>(event.message)
        assertEquals(event.channelId, message.channelId)
        assertEquals("Hello from WebSocket!", message.content)
        assertEquals(UserStatus.ONLINE, message.author.status)
    }

    @Test
    fun `auth response fixture decodes`() {
        val auth = KaikuJson.decodeFromString<AuthResponse>(Golden.load("auth_response.json"))

        assertTrue(auth.accessToken.isNotEmpty())
        assertNotNull(auth.refreshToken)
        assertEquals(900, auth.expiresIn)
        assertEquals("Bearer", auth.tokenType)
        assertFalse(auth.setupRequired)
    }

    @Test
    fun `guild channels fixture decodes with forum and announcement`() {
        val channels = KaikuJson.decodeFromString<List<Channel>>(Golden.load("guild_channels.json"))

        assertEquals(4, channels.size)
        assertEquals(
            listOf(ChannelType.TEXT, ChannelType.VOICE, ChannelType.UNKNOWN, ChannelType.UNKNOWN),
            channels.map { it.channelType }
        )
        assertEquals(2, channels.count { it.channelType == ChannelType.UNKNOWN })
        assertEquals(3, channels[0].unreadCount)
    }
}
