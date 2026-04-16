package io.wolftown.kaiku.data.ws

import io.wolftown.kaiku.domain.model.UserStatus
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/**
 * All server-to-client WebSocket events.
 *
 * Wire format: `{"type": "event_name", ...fields}` with snake_case
 * type discriminator and snake_case field names.
 */
@Serializable
sealed class ServerEvent {

    // -- Connection -----------------------------------------------------------

    @Serializable
    @SerialName("ready")
    data class Ready(val userId: String) : ServerEvent()

    @Serializable
    @SerialName("pong")
    data object Pong : ServerEvent()

    @Serializable
    @SerialName("subscribed")
    data class Subscribed(val channelId: String) : ServerEvent()

    @Serializable
    @SerialName("unsubscribed")
    data class Unsubscribed(val channelId: String) : ServerEvent()

    @Serializable
    @SerialName("error")
    data class Error(val code: String, val message: String) : ServerEvent()

    // -- Messages -------------------------------------------------------------

    @Serializable
    @SerialName("message_new")
    data class MessageNew(val channelId: String, val message: JsonObject) : ServerEvent()

    @Serializable
    @SerialName("message_edit")
    data class MessageEdit(
        val channelId: String,
        val messageId: String,
        val content: String,
        val editedAt: String
    ) : ServerEvent()

    @Serializable
    @SerialName("message_delete")
    data class MessageDelete(val channelId: String, val messageId: String) : ServerEvent()

    @Serializable
    @SerialName("reaction_add")
    data class ReactionAdd(
        val channelId: String,
        val messageId: String,
        val userId: String,
        val emoji: String
    ) : ServerEvent()

    @Serializable
    @SerialName("reaction_remove")
    data class ReactionRemove(
        val channelId: String,
        val messageId: String,
        val userId: String,
        val emoji: String
    ) : ServerEvent()

    // -- Typing ---------------------------------------------------------------

    @Serializable
    @SerialName("typing_start")
    data class TypingStart(val channelId: String, val userId: String) : ServerEvent()

    @Serializable
    @SerialName("typing_stop")
    data class TypingStop(val channelId: String, val userId: String) : ServerEvent()

    // -- Presence -------------------------------------------------------------

    @Serializable
    @SerialName("presence_update")
    data class PresenceUpdate(val userId: String, val status: UserStatus) : ServerEvent()

    // -- Voice ----------------------------------------------------------------

    @Serializable
    @SerialName("voice_publisher_answer")
    data class VoicePublisherAnswer(val channelId: String, val sdp: String) : ServerEvent()

    @Serializable
    @SerialName("voice_subscriber_offer")
    data class VoiceSubscriberOffer(val channelId: String, val sdp: String) : ServerEvent()

    @Serializable
    @SerialName("voice_ice_candidate")
    data class VoiceIceCandidate(
        val channelId: String,
        val candidate: String,
        val pcType: String  // "publisher" or "subscriber"
    ) : ServerEvent()

    @Serializable
    @SerialName("voice_user_joined")
    data class VoiceUserJoined(
        val channelId: String,
        val userId: String,
        val username: String,
        val displayName: String
    ) : ServerEvent()

    @Serializable
    @SerialName("voice_user_left")
    data class VoiceUserLeft(val channelId: String, val userId: String) : ServerEvent()

    @Serializable
    @SerialName("voice_user_muted")
    data class VoiceUserMuted(val channelId: String, val userId: String) : ServerEvent()

    @Serializable
    @SerialName("voice_user_unmuted")
    data class VoiceUserUnmuted(val channelId: String, val userId: String) : ServerEvent()

    @Serializable
    @SerialName("voice_room_state")
    data class VoiceRoomState(
        val channelId: String,
        val participants: List<VoiceParticipant>,
        val screenShares: List<ScreenShareInfo> = emptyList()
    ) : ServerEvent()

    @Serializable
    @SerialName("voice_error")
    data class VoiceError(val code: String, val message: String) : ServerEvent()

    // -- Screen Share ---------------------------------------------------------

    @Serializable
    @SerialName("screen_share_started")
    data class ScreenShareStarted(
        val channelId: String,
        val userId: String,
        val streamId: String,
        val username: String,
        val sourceLabel: String,
        val hasAudio: Boolean,
        val quality: String,
        val startedAt: String
    ) : ServerEvent()

    @Serializable
    @SerialName("screen_share_stopped")
    data class ScreenShareStopped(
        val channelId: String,
        val userId: String,
        val streamId: String,
        val reason: String
    ) : ServerEvent()

    @Serializable
    @SerialName("voice_layer_changed")
    data class VoiceLayerChanged(
        val channelId: String,
        val sourceUserId: String,
        val trackSource: String,
        val activeLayer: String
    ) : ServerEvent()
}

@Serializable
data class VoiceParticipant(
    val userId: String,
    val username: String? = null,
    val displayName: String? = null,
    val muted: Boolean = false,
    val screenSharing: Boolean = false,
    val webcamActive: Boolean = false
)

@Serializable
data class ScreenShareInfo(
    val streamId: String,
    val userId: String,
    val username: String = "",
    val sourceLabel: String = "",
    val hasAudio: Boolean = false,
    val quality: String = "medium",
    val startedAt: String = ""
)
