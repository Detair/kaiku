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
 *   work being replaced by a fast `ended`, which then still shows the missed call;
 * - marks an accepted `ended` as [CallPhase.ENDING], so a duplicate ringing
 *   of that call cannot replace the pending ended work.
 */
class CallWakeIntake @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val store: CallNotificationStore,
) {
    private companion object {
        /** Compare-and-set attempts against a concurrent writer (the call work). */
        const val MAX_TRIES = 3
    }

    internal var now: () -> Long = System::currentTimeMillis

    /** Returns whether the call work should be queued for [signal]. */
    fun accept(signal: WakeSignal): Boolean {
        val call = signal.call ?: return false
        if (signal.kind != WakeKind.CALL) return false
        val userId = tokenStorage.getUserId() ?: return false
        repeat(MAX_TRIES) {
            val stored = store.shown(userId, signal.channelId)
            val nowMs = now()
            val current = CallRules.live(stored, nowMs)
            val sameCall = current != null && current.callId == call.callId
            val next = when (call.state) {
                CallState.RINGING -> {
                    if (!CallRules.ringingApplies(stored, call, nowMs)) return false
                    // A repeat keeps the first receive time.
                    if (sameCall) return true
                    ShownCall(call.callId, CallPhase.RINGING, nowMs)
                }
                CallState.ENDED -> {
                    if (!CallRules.endedApplies(stored, call, nowMs)) return false
                    if (sameCall && current?.phase == CallPhase.ENDING) return true
                    ShownCall(call.callId, CallPhase.ENDING, nowMs)
                }
                CallState.UNKNOWN -> return false
            }
            if (store.setShownIf(userId, signal.channelId, stored, next)) return true
        }
        return false
    }
}
