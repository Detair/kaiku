package io.wolftown.kaiku.data.voice

/** What the call layer says about rejoining a DM call's voice session. */
enum class RejoinVerdict {
    /** The call is still active and this user is in it: rejoin. */
    Allowed,

    /** The call is over or this user is no longer in it: do not rejoin, end the session. */
    Refused,

    /** The server could not be asked: this attempt counts as failed. */
    Unknown,
}

/**
 * Asked by the voice owner before each rejoin attempt of a DM call's session
 * sends its `voice_join`. Guild voice channels have none.
 */
fun interface RejoinGate {
    suspend fun mayRejoin(channelId: String): RejoinVerdict
}
