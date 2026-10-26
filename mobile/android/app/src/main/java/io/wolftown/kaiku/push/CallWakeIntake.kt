package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.local.TokenStorage
import javax.inject.Inject

/**
 * The cheap synchronous step of a call wake, run by [WakeScheduler] before
 * anything is queued:
 *
 * - decides whether the wake is queued at all, so a wake that would be
 *   ignored (unknown reason, another call, a duplicate) never replaces the
 *   channel's pending call work;
 * - makes an accepted new `ringing` the channel's current call for the
 *   signed-in account, with its receive time. Recorded here, it survives its
 *   work being replaced by a fast `ended`, which then still shows the missed call.
 */
class CallWakeIntake @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val store: CallNotificationStore,
) {
    internal var now: () -> Long = System::currentTimeMillis

    /** Returns whether the call work should be queued for [signal]. */
    fun accept(signal: WakeSignal): Boolean {
        val call = signal.call ?: return false
        if (signal.kind != WakeKind.CALL) return false
        val userId = tokenStorage.getUserId() ?: return false
        val stored = store.shown(userId, signal.channelId)
        return when (call.state) {
            CallState.RINGING -> {
                if (!CallRules.ringingApplies(stored, call)) return false
                // A repeat keeps the first receive time.
                if (stored?.callId != call.callId) {
                    store.setShown(userId, signal.channelId, ShownCall(call.callId, CallPhase.RINGING, now()))
                }
                true
            }
            CallState.ENDED -> CallRules.endedApplies(stored, call)
            CallState.UNKNOWN -> false
        }
    }
}
