package io.wolftown.kaiku.push

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class WakeSignalParserTest {

    private val uuid = "0f8fad5b-d9cb-469f-a165-70867728950e"

    @Test
    fun `fcm data map parses with string count`() {
        val signal = WakeSignalParser.fromFcmData(mapOf("t" to "dm", "channel_id" to uuid, "count" to "3"))
        assertEquals(WakeSignal(WakeKind.DM, uuid, 3), signal)
    }

    @Test
    fun `unified push json body parses with number count`() {
        val body = """{"t":"mention","channel_id":"$uuid","count":5}""".toByteArray()
        assertEquals(WakeSignal(WakeKind.MENTION, uuid, 5), WakeSignalParser.fromUnifiedPushBody(body))
    }

    @Test
    fun `read kind parses`() {
        val body = """{"t":"read","channel_id":"$uuid","count":0}""".toByteArray()
        assertEquals(WakeSignal(WakeKind.READ, uuid, 0), WakeSignalParser.fromUnifiedPushBody(body))
    }

    @Test
    fun `invalid channel id yields null`() {
        assertNull(WakeSignalParser.fromFcmData(mapOf("t" to "dm", "channel_id" to "not-a-uuid", "count" to "1")))
        assertNull(WakeSignalParser.fromFcmData(mapOf("t" to "dm", "channel_id" to "1-2-3-4-5", "count" to "1")))
        assertNull(WakeSignalParser.fromFcmData(mapOf("t" to "dm", "count" to "1")))
        assertNull(WakeSignalParser.fromUnifiedPushBody("""{"t":"dm","channel_id":42,"count":1}""".toByteArray()))
    }

    @Test
    fun `unknown kind maps to UNKNOWN`() {
        val signal = WakeSignalParser.fromFcmData(mapOf("t" to "future", "channel_id" to uuid, "count" to "1"))
        assertEquals(WakeKind.UNKNOWN, signal?.kind)
    }

    @Test
    fun `garbage body yields null`() {
        assertNull(WakeSignalParser.fromUnifiedPushBody("not json".toByteArray()))
        assertNull(WakeSignalParser.fromUnifiedPushBody("[1,2]".toByteArray()))
        assertNull(WakeSignalParser.fromUnifiedPushBody(byteArrayOf(0xC3.toByte(), 0x28)))
        assertNull(WakeSignalParser.fromUnifiedPushBody(ByteArray(0)))
    }

    @Test
    fun `count is clamped and defaults to one`() {
        assertEquals(100, WakeSignalParser.fromFcmData(mapOf("t" to "dm", "channel_id" to uuid, "count" to "99999999999"))?.count)
        assertEquals(0, WakeSignalParser.fromFcmData(mapOf("t" to "dm", "channel_id" to uuid, "count" to "-4"))?.count)
        assertEquals(1, WakeSignalParser.fromFcmData(mapOf("t" to "dm", "channel_id" to uuid))?.count)
    }

    @Test
    fun `uppercase channel id is normalized`() {
        val signal = WakeSignalParser.fromFcmData(mapOf("t" to "dm", "channel_id" to uuid.uppercase(), "count" to "1"))
        assertEquals(uuid, signal?.channelId)
    }

    private val callId = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b"
    private val callerId = "7a8b9c0d-1e2f-4a3b-8c4d-5e6f7a8b9c0d"

    private fun callData(
        state: String? = "ringing",
        reason: String? = null,
        call: String? = callId,
        caller: String? = callerId,
    ): Map<String, String> = buildMap {
        put("t", "call")
        put("channel_id", uuid)
        call?.let { put("call_id", it) }
        caller?.let { put("caller_id", it) }
        state?.let { put("call_state", it) }
        reason?.let { put("reason", it) }
    }

    @Test
    fun `fcm ringing call parses call info`() {
        val signal = WakeSignalParser.fromFcmData(callData())
        assertEquals(WakeKind.CALL, signal?.kind)
        assertEquals(uuid, signal?.channelId)
        assertEquals(CallInfo(callId, callerId, CallState.RINGING, reason = null), signal?.call)
    }

    @Test
    fun `fcm ended call parses reason`() {
        val signal = WakeSignalParser.fromFcmData(callData(state = "ended", reason = "no_answer"))
        assertEquals(CallInfo(callId, callerId, CallState.ENDED, CallEndReason.NO_ANSWER), signal?.call)
        assertEquals(
            CallEndReason.CANCELLED,
            WakeSignalParser.fromFcmData(callData(state = "ended", reason = "cancelled"))?.call?.reason,
        )
        assertEquals(
            CallEndReason.ALL_DECLINED,
            WakeSignalParser.fromFcmData(callData(state = "ended", reason = "all_declined"))?.call?.reason,
        )
        assertEquals(
            CallEndReason.LAST_LEFT,
            WakeSignalParser.fromFcmData(callData(state = "ended", reason = "last_left"))?.call?.reason,
        )
    }

    @Test
    fun `unified push call body parses call info`() {
        val body = """{"t":"call","channel_id":"$uuid","call_id":"$callId","caller_id":"$callerId",""" +
            """"call_state":"ended","reason":"cancelled"}"""
        val signal = WakeSignalParser.fromUnifiedPushBody(body.toByteArray())
        assertEquals(WakeKind.CALL, signal?.kind)
        assertEquals(CallInfo(callId, callerId, CallState.ENDED, CallEndReason.CANCELLED), signal?.call)
    }

    @Test
    fun `unified push ringing body parses without reason`() {
        val body = """{"t":"call","channel_id":"$uuid","call_id":"$callId","caller_id":"$callerId","call_state":"ringing"}"""
        val signal = WakeSignalParser.fromUnifiedPushBody(body.toByteArray())
        assertEquals(CallInfo(callId, callerId, CallState.RINGING, reason = null), signal?.call)
    }

    @Test
    fun `call ids are normalized to lowercase`() {
        val signal = WakeSignalParser.fromFcmData(callData(call = callId.uppercase(), caller = callerId.uppercase()))
        assertEquals(callId, signal?.call?.callId)
        assertEquals(callerId, signal?.call?.callerId)
    }

    @Test
    fun `call with missing or invalid call id yields null`() {
        assertNull(WakeSignalParser.fromFcmData(callData(call = null)))
        assertNull(WakeSignalParser.fromFcmData(callData(call = "not-a-uuid")))
        assertNull(WakeSignalParser.fromFcmData(callData(call = "1-2-3-4-5")))
        val body = """{"t":"call","channel_id":"$uuid","call_id":42,"caller_id":"$callerId","call_state":"ringing"}"""
        assertNull(WakeSignalParser.fromUnifiedPushBody(body.toByteArray()))
    }

    @Test
    fun `call with missing or invalid caller id yields null`() {
        assertNull(WakeSignalParser.fromFcmData(callData(caller = null)))
        assertNull(WakeSignalParser.fromFcmData(callData(caller = "bob")))
        val body = """{"t":"call","channel_id":"$uuid","call_id":"$callId","call_state":"ringing"}"""
        assertNull(WakeSignalParser.fromUnifiedPushBody(body.toByteArray()))
    }

    @Test
    fun `unknown call state and reason are kept as unknown`() {
        assertEquals(CallState.UNKNOWN, WakeSignalParser.fromFcmData(callData(state = "on_hold"))?.call?.state)
        assertEquals(CallState.UNKNOWN, WakeSignalParser.fromFcmData(callData(state = null))?.call?.state)
        assertEquals(
            CallEndReason.UNKNOWN,
            WakeSignalParser.fromFcmData(callData(state = "ended", reason = "exploded"))?.call?.reason,
        )
    }

    @Test
    fun `non call signals carry no call info`() {
        val data = callData() + ("t" to "dm")
        assertNull(WakeSignalParser.fromFcmData(data)?.call)
    }
}
