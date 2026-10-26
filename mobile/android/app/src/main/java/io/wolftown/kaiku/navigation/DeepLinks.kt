package io.wolftown.kaiku.navigation

import java.net.URI
import java.util.UUID

object DeepLinks {
    private const val SCHEME = "kaiku"
    private const val HOST = "channel"

    fun channelUri(channelId: String): String = "$SCHEME://$HOST/$channelId"

    /** Returns the channel id of a `kaiku://channel/{uuid}` link, or null for anything else. */
    fun parseChannelId(uri: String?): String? {
        if (uri == null) return null
        val parsed = try {
            URI(uri)
        } catch (_: Exception) {
            return null
        }
        if (parsed.scheme != SCHEME || parsed.host != HOST) return null
        if (parsed.rawQuery != null || parsed.rawFragment != null) return null
        val id = parsed.rawPath?.removePrefix("/") ?: return null
        val uuid = try {
            UUID.fromString(id)
        } catch (_: IllegalArgumentException) {
            return null
        }
        // UUID.fromString is lenient about short groups; require the canonical form.
        return if (uuid.toString() == id.lowercase()) id.lowercase() else null
    }
}
