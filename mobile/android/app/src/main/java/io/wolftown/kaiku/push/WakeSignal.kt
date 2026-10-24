package io.wolftown.kaiku.push

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.util.UUID

/** Kind of a C20 wake signal (`t`). Unknown values map to [UNKNOWN] and are ignored. */
enum class WakeKind {
    DM,
    MENTION,
    READ,
    CALL,
    UNKNOWN;

    companion object {
        fun fromWire(value: String?): WakeKind = when (value) {
            "dm" -> DM
            "mention" -> MENTION
            "read" -> READ
            "call" -> CALL
            else -> UNKNOWN
        }
    }
}

/**
 * A content-free wake signal (C20): which channel has activity and how many
 * messages the push stands for ([count] is a fetch-size hint, not an unread count).
 */
data class WakeSignal(val kind: WakeKind, val channelId: String, val count: Int)

/** Parses C20 wake signals from FCM data messages and UnifiedPush bodies. */
object WakeSignalParser {
    const val MAX_COUNT = 100
    private const val DEFAULT_COUNT = 1

    private val json = Json { ignoreUnknownKeys = true }

    /** FCM data message: all values are strings (`"count": "3"`). */
    fun fromFcmData(data: Map<String, String>): WakeSignal? =
        build(data["t"], data["channel_id"], data["count"]?.trim()?.toLongOrNull())

    /** UnifiedPush: plaintext JSON body `{"t","channel_id","count"}` with a numeric count. */
    fun fromUnifiedPushBody(body: ByteArray): WakeSignal? {
        val obj = try {
            json.parseToJsonElement(body.decodeToString()) as? JsonObject
        } catch (_: Exception) {
            null
        } ?: return null
        val t = (obj["t"] as? JsonPrimitive)?.takeIf { it.isString }?.content
        val channelId = (obj["channel_id"] as? JsonPrimitive)?.takeIf { it.isString }?.content
        val count = (obj["count"] as? JsonPrimitive)?.let { it.longOrNull ?: it.content.trim().toLongOrNull() }
        return build(t, channelId, count)
    }

    private fun build(t: String?, channelId: String?, count: Long?): WakeSignal? {
        val id = canonicalUuid(channelId) ?: return null
        val clamped = (count ?: DEFAULT_COUNT.toLong()).coerceIn(0L, MAX_COUNT.toLong()).toInt()
        return WakeSignal(WakeKind.fromWire(t), id, clamped)
    }

    /** Lowercase canonical UUID, or null (UUID.fromString alone accepts short groups). */
    private fun canonicalUuid(value: String?): String? {
        if (value == null || value.length != 36) return null
        val uuid = try {
            UUID.fromString(value)
        } catch (_: IllegalArgumentException) {
            return null
        }
        val lower = value.lowercase()
        return if (uuid.toString() == lower) lower else null
    }
}
