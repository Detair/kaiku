package io.wolftown.kaiku.ui.call

import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.call.CallEndReason
import io.wolftown.kaiku.data.call.CallState
import io.wolftown.kaiku.data.voice.AudioRoute
import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.ws.VoiceRemovedReason
import org.junit.Assert.assertEquals
import org.junit.Test

class CallUiMapperTest {
    private val title = { _: String -> "Anna" }

    private fun map(state: CallState, link: VoiceLinkState = VoiceLinkState.Stable, now: Long = 0L, route: AudioRoute = AudioRoute.Speaker) =
        CallUiMapper.map(state, link, muted = false, route = route, nowMs = now, titleOf = title)

    @Test
    fun `ringing shows the caller and nothing else`() {
        assertEquals(
            CallUi(CallPhase.Ringing, "Anna B.", R.string.voice_call_incoming),
            map(CallState.Incoming("dm", "c", "anna", "Anna B.", 0L)),
        )
        // Unknown caller name: the DM's title.
        assertEquals("Anna", map(CallState.Incoming("dm", "c", "anna", null, 0L)).name)
    }

    @Test
    fun `calling and connecting`() {
        assertEquals(CallPhase.Calling, map(CallState.Outgoing("dm", null, 0L)).phase)
        assertEquals(R.string.voice_call_calling, map(CallState.Outgoing("dm", null, 0L)).status)
        assertEquals(R.string.voice_call_connecting, map(CallState.Connecting("dm", "c")).status)
        // In the call, but voice is still connecting: no running time yet.
        val connecting = map(CallState.Connected("dm", "c", 0L), link = VoiceLinkState.Connecting, now = 5_000L)
        assertEquals(CallUi(CallPhase.Connecting, "Anna", R.string.voice_call_connecting, null, false, true), connecting)
    }

    @Test
    fun `connected shows the running time and reconnecting keeps it`() {
        val connected = map(CallState.Connected("dm", "c", 1_000L), now = 66_500L)
        assertEquals(CallPhase.Connected, connected.phase)
        assertEquals(65L, connected.durationSecs)
        assertEquals(R.string.voice_call_in_call, connected.status)

        val rejoining = map(CallState.Connected("dm", "c", 1_000L), link = VoiceLinkState.Rejoining(2), now = 11_000L)
        assertEquals(CallPhase.Reconnecting, rejoining.phase)
        assertEquals(R.string.voice_call_reconnecting, rejoining.status)
        assertEquals(10L, rejoining.durationSecs)
    }

    @Test
    fun `speaker follows the route`() {
        assertEquals(true, map(CallState.Connected("dm", "c", 0L)).speaker)
        assertEquals(false, map(CallState.Connected("dm", "c", 0L), route = AudioRoute.Earpiece).speaker)
    }

    @Test
    fun `every end reason has its line`() {
        for (reason in CallEndReason.entries) {
            CallUiMapper.endedText(reason, null)
        }
        assertEquals(R.string.voice_call_ended_no_answer, CallUiMapper.endedText(CallEndReason.NoAnswer, null))
        assertEquals(R.string.voice_call_taken_elsewhere, CallUiMapper.endedText(CallEndReason.Removed, VoiceRemovedReason.SessionReplaced))
        assertEquals(R.string.voice_call_removed, CallUiMapper.endedText(CallEndReason.Removed, VoiceRemovedReason.Kicked))
        assertEquals(R.string.voice_call_cut_off, CallUiMapper.endedText(CallEndReason.ConnectionLost, null))
        assertEquals(
            CallUi(CallPhase.Ended, "Anna", R.string.voice_call_ended_declined),
            map(CallState.Ended("dm", CallEndReason.Declined)),
        )
    }

    @Test
    fun `speaker off goes to a headset when one is connected`() {
        assertEquals(AudioRoute.Speaker, CallUiMapper.speakerTarget(true, setOf(AudioRoute.Earpiece)))
        assertEquals(AudioRoute.Earpiece, CallUiMapper.speakerTarget(false, setOf(AudioRoute.Speaker, AudioRoute.Earpiece)))
        assertEquals(AudioRoute.Bluetooth, CallUiMapper.speakerTarget(false, setOf(AudioRoute.Bluetooth, AudioRoute.WiredHeadset)))
        assertEquals(AudioRoute.WiredHeadset, CallUiMapper.speakerTarget(false, setOf(AudioRoute.WiredHeadset, AudioRoute.Earpiece)))
    }

    @Test
    fun `duration format`() {
        assertEquals("0:05", CallUiMapper.formatDuration(5))
        assertEquals("12:34", CallUiMapper.formatDuration(754))
        assertEquals("1:02:03", CallUiMapper.formatDuration(3723))
    }
}
