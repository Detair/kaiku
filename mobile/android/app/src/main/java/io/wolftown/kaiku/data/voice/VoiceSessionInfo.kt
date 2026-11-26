package io.wolftown.kaiku.data.voice

/**
 * The identity of one voice session: [number] is unique per process,
 * [callTag] is the call layer's name for it when it is a DM call's session
 * (null for a voice channel). [hasAudio]: the session has taken the phone's sound (its
 * first join is set up and it holds the audio focus).
 */
data class VoiceSessionInfo(val number: Long, val channelId: String, val callTag: Long?, val hasAudio: Boolean = false)
