package io.wolftown.kaiku.data.ws

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * All client-to-server WebSocket events.
 *
 * Serialized with `{"type": "event_name", ...fields}` using [WsJson].
 */
@Serializable
sealed class ClientEvent {

    @Serializable
    @SerialName("authenticate")
    data class Authenticate(val token: String) : ClientEvent()

    @Serializable
    @SerialName("ping")
    data object Ping : ClientEvent()

    @Serializable
    @SerialName("subscribe")
    data class Subscribe(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("unsubscribe")
    data class Unsubscribe(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("typing")
    data class Typing(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("stop_typing")
    data class StopTyping(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("voice_join")
    data class VoiceJoin(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("voice_leave")
    data class VoiceLeave(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("voice_publisher_offer")
    data class VoicePublisherOffer(val channelId: String, val sdp: String) : ClientEvent()

    @Serializable
    @SerialName("voice_subscriber_answer")
    data class VoiceSubscriberAnswer(val channelId: String, val sdp: String) : ClientEvent()

    @Serializable
    @SerialName("voice_ice_candidate")
    data class VoiceIceCandidate(
        val channelId: String,
        val candidate: String,
        val pcType: String  // "publisher" or "subscriber"
    ) : ClientEvent()

    @Serializable
    @SerialName("voice_mute")
    data class VoiceMute(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("voice_unmute")
    data class VoiceUnmute(val channelId: String) : ClientEvent()

    @Serializable
    @SerialName("voice_set_layer_preference")
    data class VoiceSetLayerPreference(
        val channelId: String,
        val targetUserId: String,
        val trackSource: String,
        val preferredLayer: String
    ) : ClientEvent()
}
