package io.wolftown.kaiku.data.api

import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WsJson
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.Category
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.UnreadAggregate
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
    fun `password change fixture decodes with flag true`() {
        val auth = KaikuJson.decodeFromString<AuthResponse>(Golden.load("auth_response_password_change.json"))

        assertTrue(auth.passwordChangeRequired)
        assertTrue(auth.accessToken.isNotEmpty())
        assertNotNull(auth.refreshToken)
        assertEquals(900, auth.expiresIn)
        // The regular login fixture carries the flag as false.
        assertFalse(KaikuJson.decodeFromString<AuthResponse>(Golden.load("auth_response.json")).passwordChangeRequired)
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

    @Test
    fun `categories fixture decodes`() {
        val categories = KaikuJson.decodeFromString<List<Category>>(Golden.load("guild_categories.json"))

        assertEquals(listOf("Text Channels", "Voice"), categories.map { it.name })
        assertEquals(listOf(0, 1), categories.map { it.position })
        assertEquals(null, categories[0].parentId)
        assertEquals(categories[0].id, categories[1].parentId)
    }

    @Test
    fun `ready fixture decodes with server_time`() {
        val event = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_ready.json"))
        assertIs<ServerEvent.Ready>(event)

        assertEquals("00000000-0000-4000-8000-00000000a001", event.userId)
        assertEquals("2026-03-12T10:03:00.250Z", event.serverTime)
    }

    @Test
    fun `ready without server_time still decodes`() {
        val event = WsJson.decodeFromString<ServerEvent>(
            """{"type":"ready","user_id":"00000000-0000-4000-8000-00000000a001"}"""
        )
        assertIs<ServerEvent.Ready>(event)

        assertEquals("00000000-0000-4000-8000-00000000a001", event.userId)
        assertEquals(null, event.serverTime)
    }

    @Test
    fun `subscribed fixture decodes`() {
        val event = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_subscribed.json"))

        assertEquals(ServerEvent.Subscribed("00000000-0000-4000-8000-00000000c001"), event)
    }

    @Test
    fun `dm_created fixture decodes and its channel decodes as DmConversation`() {
        val event = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_dm_created.json"))
        assertIs<ServerEvent.DmCreated>(event)

        val dm = KaikuJson.decodeFromJsonElement<DmConversation>(event.channel)
        assertEquals("00000000-0000-4000-8000-00000000d101", dm.id)
        assertEquals(ChannelType.DM, dm.channelType)
        assertEquals(listOf("alice", "bob"), dm.participants.map { it.username })
        assertEquals(null, dm.lastMessage)
        assertEquals(0, dm.unreadCount)
    }

    @Test
    fun `dm list fixture decodes`() {
        val dms = KaikuJson.decodeFromString<List<DmConversation>>(Golden.load("dm_list.json"))

        assertEquals(2, dms.size)
        val (group, direct) = dms
        assertEquals("Weekend plans", group.name)
        assertEquals(ChannelType.DM, group.channelType)
        assertNotNull(group.iconUrl)
        assertEquals(3, group.participants.size)
        assertEquals("/api/users/carol/avatar", group.participants[2].avatarUrl)
        assertEquals(2, group.unreadCount)
        val last = group.lastMessage
        assertNotNull(last)
        assertEquals("See you Saturday", last!!.content)
        assertEquals("carol", last.username)
        assertEquals("2026-03-12T11:00:00Z", last.createdAt)

        assertEquals("00000000-0000-4000-8000-00000000d101", direct.id)
        assertEquals(null, direct.lastMessage)
        assertEquals(0, direct.unreadCount)
    }

    @Test
    fun `channel_read and dm_read fixtures decode`() {
        val channelRead = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_channel_read.json"))
        assertIs<ServerEvent.ChannelRead>(channelRead)
        assertEquals("00000000-0000-4000-8000-00000000c001", channelRead.channelId)
        assertEquals("00000000-0000-4000-8000-00000000e002", channelRead.lastReadMessageId)

        val dmRead = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_dm_read.json"))
        assertIs<ServerEvent.DmRead>(dmRead)
        assertEquals("00000000-0000-4000-8000-00000000d101", dmRead.channelId)
        assertEquals(null, dmRead.lastReadMessageId)
    }

    @Test
    fun `unread aggregate fixture decodes`() {
        val aggregate = KaikuJson.decodeFromString<UnreadAggregate>(Golden.load("unread_aggregate.json"))

        val dm = aggregate.dms.single()
        assertEquals("00000000-0000-4000-8000-00000000d101", dm.channelId)
        assertEquals(2L, dm.unreadCount)
        assertEquals(null, dm.lastReadMessageId)
        val channel = aggregate.guilds.single().channels.single()
        assertEquals("00000000-0000-4000-8000-00000000c001", channel.channelId)
        assertEquals(3L, channel.unreadCount)
        assertEquals("00000000-0000-4000-8000-00000000e001", channel.lastReadMessageId)
        assertEquals(5L, aggregate.total)
    }
}
