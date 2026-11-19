package io.wolftown.kaiku.data.ws

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ServerEventDecoderTest {
    private val decoder = ServerEventDecoder(WsJson)

    @Test
    fun `known events decode`() {
        val r = decoder.decode("""{"type":"subscribed","channel_id":"c1"}""")
        assertEquals(ServerEventDecoder.Result.Event(ServerEvent.Subscribed("c1")), r)
        assertTrue("ready" in ServerEventDecoder.KNOWN_TYPES)
        assertTrue("dm_created" in ServerEventDecoder.KNOWN_TYPES)
    }

    @Test
    fun `unmodelled types are a counted skip, not an error`() {
        repeat(3) { decoder.decode("""{"type":"rich_presence_update","user_id":"u","activity":{"name":"x"}}""") }
        val r = decoder.decode("""{"type":"custom_status_update","user_id":"u","custom_status":null}""")
        assertEquals(ServerEventDecoder.Result.Skipped("custom_status_update"), r)
        assertEquals(mapOf("rich_presence_update" to 3L, "custom_status_update" to 1L), decoder.skippedCounts())
    }

    @Test
    fun `a known type with a broken body and garbage are reported as broken`() {
        val broken = decoder.decode("""{"type":"subscribed"}""")
        assertTrue(broken is ServerEventDecoder.Result.Broken && broken.type == "subscribed")
        assertTrue(decoder.decode("not json") is ServerEventDecoder.Result.Broken)
        assertTrue(decoder.decode("""{"no_type":1}""") is ServerEventDecoder.Result.Broken)
        assertTrue(decoder.skippedCounts().isEmpty())
    }

    @Test
    fun `a type that is not a string is broken, not a quiet skip`() {
        assertTrue(decoder.decode("""{"type":123}""") is ServerEventDecoder.Result.Broken)
        assertTrue(decoder.decode("""{"type":true}""") is ServerEventDecoder.Result.Broken)
        assertTrue(decoder.skippedCounts().isEmpty())
    }
}
