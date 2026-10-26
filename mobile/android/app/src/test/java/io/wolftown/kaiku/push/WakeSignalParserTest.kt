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
}
