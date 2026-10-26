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

/** `call_state` of a call wake (C20 addendum). Unknown values map to [UNKNOWN] and are ignored. */
enum class CallState {
    RINGING,
    ENDED,
    UNKNOWN;

    companion object {
        fun fromWire(value: String?): CallState = when (value) {
            "ringing" -> RINGING
            "ended" -> ENDED
            else -> UNKNOWN
        }
    }
}

/** `reason` of an ended call. Unknown values map to [UNKNOWN] and are ignored. */
enum class CallEndReason {
    NO_ANSWER,
    CANCELLED,
    ALL_DECLINED,
    LAST_LEFT,
    UNKNOWN;

    companion object {
        /** Null when the payload has no reason. */
        fun fromWire(value: String?): CallEndReason? = when (value) {
            null -> null
            "no_answer" -> NO_ANSWER
            "cancelled" -> CANCELLED
            "all_declined" -> ALL_DECLINED
            "last_left" -> LAST_LEFT
            else -> UNKNOWN
        }
    }
}

/** Ids and state of a DM call wake; no names (they are resolved after the wake). */
data class CallInfo(
    val callId: String,
    val callerId: String,
    val state: CallState,
    val reason: CallEndReason? = null,
)

/**
 * A content-free wake signal (C20): which channel has activity and how many
 * messages the push stands for ([count] is a fetch-size hint, not an unread count).
 * [call] is set for (and only for) [WakeKind.CALL] signals.
 */
data class WakeSignal(
    val kind: WakeKind,
    val channelId: String,
    val count: Int,
    val call: CallInfo? = null,
)

/** Raw call fields of a wake payload, before validation. */
private data class RawCall(
    val callId: String?,
    val callerId: String?,
    val state: String?,
    val reason: String?,
)

/** Parses C20 wake signals from FCM data messages and UnifiedPush bodies. */
object WakeSignalParser {
    const val MAX_COUNT = 100
    private const val DEFAULT_COUNT = 1

    private val json = Json { ignoreUnknownKeys = true }

    /** FCM data message: all values are strings (`"count": "3"`, call fields as for UnifiedPush). */
    fun fromFcmData(data: Map<String, String>): WakeSignal? =
        build(
            data["t"],
            data["channel_id"],
            data["count"]?.trim()?.toLongOrNull(),
            RawCall(data["call_id"], data["caller_id"], data["call_state"], data["reason"]),
        )

    /**
     * UnifiedPush: plaintext JSON body `{"t","channel_id","count"}` with a numeric
     * count; call wakes add string `call_id`, `caller_id`, `call_state`, `reason`.
     */
    fun fromUnifiedPushBody(body: ByteArray): WakeSignal? {
        val obj = try {
            json.parseToJsonElement(body.decodeToString()) as? JsonObject
        } catch (_: Exception) {
            null
        } ?: return null
        fun string(key: String) = (obj[key] as? JsonPrimitive)?.takeIf { it.isString }?.content
        val count = (obj["count"] as? JsonPrimitive)?.let { it.longOrNull ?: it.content.trim().toLongOrNull() }
        return build(
            string("t"),
            string("channel_id"),
            count,
            RawCall(string("call_id"), string("caller_id"), string("call_state"), string("reason")),
        )
    }

    private fun build(t: String?, channelId: String?, count: Long?, rawCall: RawCall): WakeSignal? {
        val id = canonicalUuid(channelId) ?: return null
        val kind = WakeKind.fromWire(t)
        // A call wake without valid ids cannot be matched to anything: drop it.
        val call = if (kind == WakeKind.CALL) {
            callInfo(rawCall) ?: return null
        } else {
            null
        }
        val clamped = (count ?: DEFAULT_COUNT.toLong()).coerceIn(0L, MAX_COUNT.toLong()).toInt()
        return WakeSignal(kind, id, clamped, call)
    }

    private fun callInfo(raw: RawCall): CallInfo? {
        val callId = canonicalUuid(raw.callId) ?: return null
        val callerId = canonicalUuid(raw.callerId) ?: return null
        return CallInfo(callId, callerId, CallState.fromWire(raw.state), CallEndReason.fromWire(raw.reason))
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
