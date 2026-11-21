package io.wolftown.kaiku.ui.call

import io.wolftown.kaiku.data.call.CallState
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first

/** How the call screen treats the display and the lock screen. */
data class CallScreenFlags(
    /** Keep the display on. */
    val keepScreenOn: Boolean,
    /** Turn the display on when the screen appears. */
    val turnScreenOn: Boolean,
    /** Show over the lock screen. */
    val showWhenLocked: Boolean,
    /** Turn the display off while something is near the sensor (the phone at the ear). */
    val proximityScreenOff: Boolean,
)

/**
 * The display while a call is shown: a ring turns it on and keeps it on (over
 * the lock screen); in the call nothing keeps it on, and on the earpiece it
 * goes off at the ear so a cheek cannot press Mute or Hang up. The screen
 * stays over the lock screen during the call and until its end was shown.
 */
object CallScreenPolicy {
    fun of(ui: CallUi): CallScreenFlags = when (ui.phase) {
        CallPhase.Ringing -> CallScreenFlags(keepScreenOn = true, turnScreenOn = true, showWhenLocked = true, proximityScreenOff = false)
        CallPhase.Calling, CallPhase.Connecting, CallPhase.Connected, CallPhase.Reconnecting ->
            CallScreenFlags(keepScreenOn = false, turnScreenOn = false, showWhenLocked = true, proximityScreenOff = ui.earpiece && !ui.speaker)
        CallPhase.Ended -> CallScreenFlags(keepScreenOn = false, turnScreenOn = false, showWhenLocked = true, proximityScreenOff = false)
        CallPhase.None -> CallScreenFlags(keepScreenOn = false, turnScreenOn = false, showWhenLocked = false, proximityScreenOff = false)
    }

    /**
     * Returns once the calls shown are over: no call for [closeDelayMs] after the last one
     * ended. A call that starts within that moment is shown, and the wait goes on until it
     * is over too (the screen never stays behind without a call).
     */
    suspend fun awaitOver(state: StateFlow<CallState>, closeDelayMs: Long) {
        while (true) {
            state.first { it is CallState.Idle }
            delay(closeDelayMs)
            if (state.value is CallState.Idle) return
        }
    }
}
