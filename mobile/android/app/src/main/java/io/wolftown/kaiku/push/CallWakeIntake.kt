package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.local.TokenStorage
import javax.inject.Inject

/**
 * The cheap synchronous step of a call wake, run by [WakeScheduler] before
 * the work is queued: an accepted `ringing` becomes the channel's current
 * call for the signed-in account. Recorded here, it survives its work being
 * replaced by a fast `ended`, which then still shows the missed call.
 */
class CallWakeIntake @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val store: CallNotificationStore,
) {
    fun accept(signal: WakeSignal) {
        val call = signal.call ?: return
        if (signal.kind != WakeKind.CALL || call.state != CallState.RINGING) return
        val userId = tokenStorage.getUserId() ?: return
        // A repeated ringing keeps what is known about its call (e.g. already missed).
        if (store.shown(userId, signal.channelId)?.callId == call.callId) return
        store.setShown(userId, signal.channelId, ShownCall(call.callId, missed = false))
    }
}
