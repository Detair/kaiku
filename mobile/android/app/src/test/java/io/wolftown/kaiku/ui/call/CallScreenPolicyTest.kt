package io.wolftown.kaiku.ui.call

import org.junit.Assert.assertEquals
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import org.junit.Test

class CallScreenPolicyTest {

    @Test
    fun `a ring turns the display on, keeps it on and shows over the lock screen`() {
        assertEquals(
            CallScreenFlags(keepScreenOn = true, turnScreenOn = true, showWhenLocked = true, proximityScreenOff = false),
            CallScreenPolicy.of(CallUi(CallPhase.Ringing)),
        )
    }

    @Test
    fun `in the call on the earpiece the display goes off at the ear and is not kept on`() {
        for (phase in listOf(CallPhase.Calling, CallPhase.Connecting, CallPhase.Connected, CallPhase.Reconnecting)) {
            assertEquals(
                CallScreenFlags(keepScreenOn = false, turnScreenOn = false, showWhenLocked = true, proximityScreenOff = true),
                CallScreenPolicy.of(CallUi(phase, earpiece = true)),
            )
        }
    }

    @Test
    fun `on the speaker or a headset there is no proximity screen-off`() {
        assertEquals(false, CallScreenPolicy.of(CallUi(CallPhase.Connected, speaker = true)).proximityScreenOff)
        assertEquals(false, CallScreenPolicy.of(CallUi(CallPhase.Connected, earpiece = false)).proximityScreenOff)
    }

    @Test
    fun `after the call nothing stays over the lock screen once the end was shown`() {
        assertEquals(CallScreenFlags(false, false, true, false), CallScreenPolicy.of(CallUi(CallPhase.Ended)))
        assertEquals(CallScreenFlags(false, false, false, false), CallScreenPolicy.of(CallUi(CallPhase.None)))
    }

    @Test
    fun `the call screen waits for a call that started while it was closing`() = kotlinx.coroutines.test.runTest {
        val state = kotlinx.coroutines.flow.MutableStateFlow<io.wolftown.kaiku.data.call.CallState>(
            io.wolftown.kaiku.data.call.CallState.Incoming("dm-1", "c-1", "anna", "Anna", 0L),
        )
        var over = false
        val wait = launch { CallScreenPolicy.awaitOver(state, 300L); over = true }
        state.value = io.wolftown.kaiku.data.call.CallState.Idle
        advanceTimeBy(100); runCurrent()
        // A second call rings within the close delay.
        state.value = io.wolftown.kaiku.data.call.CallState.Incoming("dm-2", "c-2", "bob", "Bob", 0L)
        advanceTimeBy(1_000); runCurrent()
        org.junit.Assert.assertFalse("closed with a call on", over)
        state.value = io.wolftown.kaiku.data.call.CallState.Idle
        advanceTimeBy(301); runCurrent()
        org.junit.Assert.assertTrue("stayed behind after the second call", over)
        wait.cancel()
    }
}
