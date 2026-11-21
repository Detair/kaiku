package io.wolftown.kaiku.data.voice

import io.wolftown.kaiku.data.ws.VoiceRemovedReason

/** A join ended because the server removed the voice peer before the join was set up. */
class VoiceRemovedException(val reason: VoiceRemovedReason) : Exception("removed from voice: $reason")
