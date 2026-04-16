package io.wolftown.kaiku.data.ws

import io.wolftown.kaiku.domain.model.UserStatus
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.test.assertIs

/**
 * Tests deserialization of every [ServerEvent] variant from realistic
 * server JSON (snake_case type discriminator, snake_case field names).
 */
class ServerEventParsingTest {

    // ========================================================================
    // Connection events
    // ========================================================================

    @Test
    fun `Ready deserializes from server JSON`() {
        val json = """{"type":"ready","user_id":"usr-001"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.Ready>(event)
        assertEquals("usr-001", event.userId)
    }

    @Test
    fun `Pong deserializes from server JSON`() {
        val json = """{"type":"pong"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.Pong>(event)
    }

    @Test
    fun `Subscribed deserializes from server JSON`() {
        val json = """{"type":"subscribed","channel_id":"chan-001"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.Subscribed>(event)
        assertEquals("chan-001", event.channelId)
    }

    @Test
    fun `Unsubscribed deserializes from server JSON`() {
        val json = """{"type":"unsubscribed","channel_id":"chan-002"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.Unsubscribed>(event)
        assertEquals("chan-002", event.channelId)
    }

    @Test
    fun `Error deserializes from server JSON`() {
        val json = """{"type":"error","code":"auth_expired","message":"Token has expired"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.Error>(event)
        assertEquals("auth_expired", event.code)
        assertEquals("Token has expired", event.message)
    }

    // ========================================================================
    // Message events
    // ========================================================================

    @Test
    fun `MessageNew deserializes from server JSON`() {
        val json = """{"type":"message_new","channel_id":"abc-123","message":{"id":"msg-1","content":"hello"}}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.MessageNew>(event)
        assertEquals("abc-123", event.channelId)
        assertEquals("msg-1", event.message["id"]?.jsonPrimitive?.content)
        assertEquals("hello", event.message["content"]?.jsonPrimitive?.content)
    }

    @Test
    fun `MessageNew with full message object`() {
        val json = """
        {
            "type": "message_new",
            "channel_id": "chan-001",
            "message": {
                "id": "msg-uuid-001",
                "channel_id": "chan-001",
                "author": {
                    "id": "usr-001",
                    "username": "alice",
                    "display_name": "Alice"
                },
                "content": "Hello world!",
                "encrypted": false,
                "attachments": [],
                "created_at": "2026-03-12T10:00:00Z"
            }
        }
        """.trimIndent()
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.MessageNew>(event)
        assertEquals("chan-001", event.channelId)
        assertEquals("msg-uuid-001", event.message["id"]?.jsonPrimitive?.content)
    }

    @Test
    fun `MessageEdit deserializes from server JSON`() {
        val json = """{"type":"message_edit","channel_id":"chan-001","message_id":"msg-001","content":"edited text","edited_at":"2026-03-12T10:05:00Z"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.MessageEdit>(event)
        assertEquals("chan-001", event.channelId)
        assertEquals("msg-001", event.messageId)
        assertEquals("edited text", event.content)
        assertEquals("2026-03-12T10:05:00Z", event.editedAt)
    }

    @Test
    fun `MessageDelete deserializes from server JSON`() {
        val json = """{"type":"message_delete","channel_id":"chan-001","message_id":"msg-002"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.MessageDelete>(event)
        assertEquals("chan-001", event.channelId)
        assertEquals("msg-002", event.messageId)
    }

    @Test
    fun `ReactionAdd deserializes from server JSON`() {
        val json = """{"type":"reaction_add","channel_id":"chan-001","message_id":"msg-001","user_id":"usr-001","emoji":"\ud83d\udc4d"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.ReactionAdd>(event)
        assertEquals("chan-001", event.channelId)
        assertEquals("msg-001", event.messageId)
        assertEquals("usr-001", event.userId)
        assertEquals("\ud83d\udc4d", event.emoji)
    }

    @Test
    fun `ReactionRemove deserializes from server JSON`() {
        val json = """{"type":"reaction_remove","channel_id":"chan-002","message_id":"msg-003","user_id":"usr-002","emoji":"\u2764\ufe0f"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.ReactionRemove>(event)
        assertEquals("chan-002", event.channelId)
        assertEquals("msg-003", event.messageId)
        assertEquals("usr-002", event.userId)
        assertEquals("\u2764\ufe0f", event.emoji)
    }

    // ========================================================================
    // Typing events
    // ========================================================================

    @Test
    fun `TypingStart deserializes from server JSON`() {
        val json = """{"type":"typing_start","channel_id":"chan-001","user_id":"usr-001"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.TypingStart>(event)
        assertEquals("chan-001", event.channelId)
        assertEquals("usr-001", event.userId)
    }

    @Test
    fun `TypingStop deserializes from server JSON`() {
        val json = """{"type":"typing_stop","channel_id":"chan-001","user_id":"usr-001"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.TypingStop>(event)
        assertEquals("chan-001", event.channelId)
        assertEquals("usr-001", event.userId)
    }

    // ========================================================================
    // Presence events
    // ========================================================================

    @Test
    fun `PresenceUpdate deserializes from server JSON`() {
        val json = """{"type":"presence_update","user_id":"usr-001","status":"online"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.PresenceUpdate>(event)
        assertEquals("usr-001", event.userId)
        assertEquals(UserStatus.ONLINE, event.status)
    }

    @Test
    fun `PresenceUpdate with away status`() {
        val json = """{"type":"presence_update","user_id":"usr-002","status":"away"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.PresenceUpdate>(event)
        assertEquals(UserStatus.IDLE, event.status)
    }

    // ========================================================================
    // Voice events
    // ========================================================================

    @Test
    fun `VoicePublisherAnswer deserializes from server JSON`() {
        val json = """{"type":"voice_publisher_answer","channel_id":"voice-001","sdp":"v=0\r\no=- 123 IN IP4 0.0.0.0\r\n"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoicePublisherAnswer>(event)
        assertEquals("voice-001", event.channelId)
        assertTrue(event.sdp.startsWith("v=0"))
    }

    @Test
    fun `VoiceSubscriberOffer deserializes from server JSON`() {
        val json = """{"type":"voice_subscriber_offer","channel_id":"voice-001","sdp":"v=0\r\no=- 456 IN IP4 0.0.0.0\r\n"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceSubscriberOffer>(event)
        assertEquals("voice-001", event.channelId)
        assertTrue(event.sdp.startsWith("v=0"))
    }

    @Test
    fun `VoiceIceCandidate deserializes from server JSON`() {
        val json = """{"type":"voice_ice_candidate","channel_id":"voice-001","candidate":"candidate:1 1 udp 2130706431 192.168.1.1 5000 typ host","pc_type":"subscriber"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceIceCandidate>(event)
        assertEquals("voice-001", event.channelId)
        assertTrue(event.candidate.startsWith("candidate:"))
        assertEquals("subscriber", event.pcType)
    }

    @Test
    fun `VoiceUserJoined deserializes from server JSON`() {
        val json = """{"type":"voice_user_joined","channel_id":"voice-001","user_id":"usr-001","username":"alice","display_name":"Alice"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceUserJoined>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals("usr-001", event.userId)
        assertEquals("alice", event.username)
        assertEquals("Alice", event.displayName)
    }

    @Test
    fun `VoiceUserLeft deserializes from server JSON`() {
        val json = """{"type":"voice_user_left","channel_id":"voice-001","user_id":"usr-001"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceUserLeft>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals("usr-001", event.userId)
    }

    @Test
    fun `VoiceUserMuted deserializes from server JSON`() {
        val json = """{"type":"voice_user_muted","channel_id":"voice-001","user_id":"usr-002"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceUserMuted>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals("usr-002", event.userId)
    }

    @Test
    fun `VoiceUserUnmuted deserializes from server JSON`() {
        val json = """{"type":"voice_user_unmuted","channel_id":"voice-001","user_id":"usr-002"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceUserUnmuted>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals("usr-002", event.userId)
    }

    @Test
    fun `VoiceRoomState deserializes with participants`() {
        val json = """
        {
            "type": "voice_room_state",
            "channel_id": "voice-001",
            "participants": [
                {
                    "user_id": "usr-001",
                    "username": "alice",
                    "display_name": "Alice",
                    "muted": false,
                    "screen_sharing": false,
                    "webcam_active": true
                },
                {
                    "user_id": "usr-002",
                    "username": "bob",
                    "display_name": "Bob",
                    "muted": true,
                    "screen_sharing": true,
                    "webcam_active": false
                }
            ],
            "screen_shares": []
        }
        """.trimIndent()
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceRoomState>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals(2, event.participants.size)

        val alice = event.participants[0]
        assertEquals("usr-001", alice.userId)
        assertEquals("alice", alice.username)
        assertEquals("Alice", alice.displayName)
        assertFalse(alice.muted)
        assertFalse(alice.screenSharing)
        assertTrue(alice.webcamActive)

        val bob = event.participants[1]
        assertEquals("usr-002", bob.userId)
        assertTrue(bob.muted)
        assertTrue(bob.screenSharing)
        assertFalse(bob.webcamActive)
    }

    @Test
    fun `VoiceRoomState deserializes with screen shares`() {
        val json = """
        {
            "type": "voice_room_state",
            "channel_id": "voice-001",
            "participants": [],
            "screen_shares": [
                {
                    "stream_id": "stream-001",
                    "user_id": "usr-001",
                    "username": "alice",
                    "source_label": "Screen 1",
                    "has_audio": true,
                    "quality": "high",
                    "started_at": "2026-03-12T10:00:00Z"
                }
            ]
        }
        """.trimIndent()
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceRoomState>(event)
        assertEquals(1, event.screenShares.size)
        val share = event.screenShares[0]
        assertEquals("stream-001", share.streamId)
        assertEquals("usr-001", share.userId)
        assertEquals("alice", share.username)
        assertEquals("Screen 1", share.sourceLabel)
        assertTrue(share.hasAudio)
        assertEquals("high", share.quality)
        assertEquals("2026-03-12T10:00:00Z", share.startedAt)
    }

    @Test
    fun `VoiceRoomState defaults screen_shares to empty list`() {
        val json = """{"type":"voice_room_state","channel_id":"voice-001","participants":[]}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceRoomState>(event)
        assertTrue(event.screenShares.isEmpty())
    }

    @Test
    fun `VoiceError deserializes from server JSON`() {
        val json = """{"type":"voice_error","code":"room_full","message":"Voice channel is full"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceError>(event)
        assertEquals("room_full", event.code)
        assertEquals("Voice channel is full", event.message)
    }

    // ========================================================================
    // Screen share events
    // ========================================================================

    @Test
    fun `ScreenShareStarted deserializes from server JSON`() {
        val json = """
        {
            "type": "screen_share_started",
            "channel_id": "voice-001",
            "user_id": "usr-001",
            "stream_id": "stream-001",
            "username": "alice",
            "source_label": "Desktop - Screen 1",
            "has_audio": true,
            "quality": "high",
            "started_at": "2026-03-12T10:00:00Z"
        }
        """.trimIndent()
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.ScreenShareStarted>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals("usr-001", event.userId)
        assertEquals("stream-001", event.streamId)
        assertEquals("alice", event.username)
        assertEquals("Desktop - Screen 1", event.sourceLabel)
        assertTrue(event.hasAudio)
        assertEquals("high", event.quality)
        assertEquals("2026-03-12T10:00:00Z", event.startedAt)
    }

    @Test
    fun `ScreenShareStopped deserializes from server JSON`() {
        val json = """{"type":"screen_share_stopped","channel_id":"voice-001","user_id":"usr-001","stream_id":"stream-001","reason":"user_stopped"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.ScreenShareStopped>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals("usr-001", event.userId)
        assertEquals("stream-001", event.streamId)
        assertEquals("user_stopped", event.reason)
    }

    @Test
    fun `VoiceLayerChanged deserializes from server JSON`() {
        val json = """{"type":"voice_layer_changed","channel_id":"voice-001","source_user_id":"usr-001","track_source":"camera","active_layer":"high"}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.VoiceLayerChanged>(event)
        assertEquals("voice-001", event.channelId)
        assertEquals("usr-001", event.sourceUserId)
        assertEquals("camera", event.trackSource)
        assertEquals("high", event.activeLayer)
    }

    // ========================================================================
    // VoiceParticipant defaults
    // ========================================================================

    @Test
    fun `VoiceParticipant deserializes with defaults`() {
        val json = """{"user_id":"usr-001"}"""
        val participant = WsJson.decodeFromString<VoiceParticipant>(json)
        assertEquals("usr-001", participant.userId)
        assertEquals(null, participant.username)
        assertEquals(null, participant.displayName)
        assertFalse(participant.muted)
        assertFalse(participant.screenSharing)
        assertFalse(participant.webcamActive)
    }

    @Test
    fun `ScreenShareInfo deserializes with defaults`() {
        val json = """{"stream_id":"s-001","user_id":"usr-001"}"""
        val info = WsJson.decodeFromString<ScreenShareInfo>(json)
        assertEquals("s-001", info.streamId)
        assertEquals("usr-001", info.userId)
        assertEquals("", info.username)
        assertEquals("", info.sourceLabel)
        assertFalse(info.hasAudio)
        assertEquals("medium", info.quality)
        assertEquals("", info.startedAt)
    }

    // ========================================================================
    // Unknown fields are ignored
    // ========================================================================

    @Test
    fun `ServerEvent ignores unknown fields`() {
        val json = """{"type":"ready","user_id":"usr-001","server_version":"1.2.3","unknown_field":42}"""
        val event = WsJson.decodeFromString<ServerEvent>(json)
        assertIs<ServerEvent.Ready>(event)
        assertEquals("usr-001", event.userId)
    }

    // ========================================================================
    // ClientEvent serialization roundtrip
    // ========================================================================

    @Test
    fun `ClientEvent Ping serializes with type discriminator`() {
        val encoded = WsJson.encodeToString<ClientEvent>(ClientEvent.Ping)
        assertTrue(encoded.contains(""""type":"ping""""))
    }

    @Test
    fun `ClientEvent Subscribe serializes with snake_case fields`() {
        val encoded = WsJson.encodeToString<ClientEvent>(ClientEvent.Subscribe("chan-001"))
        assertTrue(encoded.contains(""""type":"subscribe""""))
        assertTrue(encoded.contains(""""channel_id":"chan-001""""))
    }

    @Test
    fun `ClientEvent VoiceSetLayerPreference serializes correctly`() {
        val event = ClientEvent.VoiceSetLayerPreference(
            channelId = "voice-001",
            targetUserId = "usr-002",
            trackSource = "screen",
            preferredLayer = "medium"
        )
        val encoded = WsJson.encodeToString<ClientEvent>(event)
        assertTrue(encoded.contains(""""type":"voice_set_layer_preference""""))
        assertTrue(encoded.contains(""""channel_id":"voice-001""""))
        assertTrue(encoded.contains(""""target_user_id":"usr-002""""))
        assertTrue(encoded.contains(""""track_source":"screen""""))
        assertTrue(encoded.contains(""""preferred_layer":"medium""""))
    }
}
