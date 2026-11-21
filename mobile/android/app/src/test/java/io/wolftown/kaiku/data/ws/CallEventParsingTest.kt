package io.wolftown.kaiku.data.ws

import org.junit.Assert.assertEquals
import org.junit.Test
import kotlin.test.assertIs

/** DM call events as the server sends them (`server/src/ws/events.rs`). */
class CallEventParsingTest {

    private val decoder = ServerEventDecoder(WsJson)

    private fun decode(json: String): ServerEvent {
        val result = decoder.decode(json)
        assertIs<ServerEventDecoder.Result.Event>(result)
        return result.event
    }

    @Test
    fun `incoming call carries the call id and the caller`() {
        val e = decode(
            """{"type":"incoming_call","channel_id":"dm-1","call_id":"c-1","initiator":"anna","initiator_name":"anna","capabilities":["audio"]}"""
        )
        assertEquals(ServerEvent.IncomingCall("dm-1", "c-1", "anna", "anna"), e)
    }

    @Test
    fun `call ended with and without a duration`() {
        assertEquals(
            ServerEvent.CallEnded("dm-1", "c-1", "no_answer", null),
            decode("""{"type":"call_ended","channel_id":"dm-1","call_id":"c-1","reason":"no_answer","duration_secs":null}"""),
        )
        assertEquals(
            ServerEvent.CallEnded("dm-1", "c-1", "last_left", 42),
            decode("""{"type":"call_ended","channel_id":"dm-1","call_id":"c-1","reason":"last_left","duration_secs":42}"""),
        )
    }

    @Test
    fun `participant joined left declined and started`() {
        assertEquals(
            ServerEvent.CallParticipantJoined("dm-1", "c-1", "me", "me"),
            decode("""{"type":"call_participant_joined","channel_id":"dm-1","call_id":"c-1","user_id":"me","username":"me"}"""),
        )
        assertEquals(
            ServerEvent.CallParticipantLeft("dm-1", "c-1", "anna"),
            decode("""{"type":"call_participant_left","channel_id":"dm-1","call_id":"c-1","user_id":"anna"}"""),
        )
        assertEquals(
            ServerEvent.CallDeclined("dm-1", "c-1", "me"),
            decode("""{"type":"call_declined","channel_id":"dm-1","call_id":"c-1","user_id":"me"}"""),
        )
        assertEquals(
            ServerEvent.CallStarted("dm-1", "c-1", "me"),
            decode("""{"type":"call_started","channel_id":"dm-1","call_id":"c-1","initiator":"me","initiator_name":"me"}"""),
        )
    }

    @Test
    fun `an event from an old server without a call id still decodes`() {
        assertEquals(
            ServerEvent.CallDeclined("dm-1", "", "me"),
            decode("""{"type":"call_declined","channel_id":"dm-1","user_id":"me"}"""),
        )
    }
}
