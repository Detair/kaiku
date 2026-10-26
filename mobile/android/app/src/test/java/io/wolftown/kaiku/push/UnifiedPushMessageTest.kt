package io.wolftown.kaiku.push

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UnifiedPushMessageTest {

    private val channel = "6f1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"

    @Test
    fun `unified push message body schedules wake`() {
        val woken = mutableListOf<WakeSignal>()
        val body = """{"t":"dm","channel_id":"$channel","count":3}""".toByteArray()

        assertTrue(handleUnifiedPushMessage(body, woken::add))

        assertEquals(listOf(WakeSignal(WakeKind.DM, channel, 3)), woken)
    }

    @Test
    fun `garbage or channel-less body schedules nothing`() {
        val woken = mutableListOf<WakeSignal>()

        assertFalse(handleUnifiedPushMessage("not json".toByteArray(), woken::add))
        assertFalse(handleUnifiedPushMessage("""{"t":"dm","count":1}""".toByteArray(), woken::add))
        assertFalse(handleUnifiedPushMessage("[]".toByteArray(), woken::add))

        assertTrue(woken.isEmpty())
    }
}
