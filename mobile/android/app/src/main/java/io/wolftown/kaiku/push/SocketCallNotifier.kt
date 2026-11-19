package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.call.CallNotifier
import io.wolftown.kaiku.data.call.signedInUserId
import io.wolftown.kaiku.data.local.AuthState
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The call notifications of [io.wolftown.kaiku.data.call.CallController],
 * posted through the same presenter and recorded in the same per-account
 * store as the push path ([CallWakeIntake], [CallWakeProcessor]). So a ring
 * the socket and a push both deliver alerts once, a ring stopped here is not
 * rung again by a late push, and a call is shown as missed once.
 */
@Singleton
class SocketCallNotifier internal constructor(
    private val sink: CallNotificationSink,
    private val store: CallNotificationStore,
    private val currentUser: () -> String?,
    private val now: () -> Long,
) : CallNotifier {

    @Inject
    constructor(sink: CallNotificationSink, store: CallNotificationStore, authState: AuthState) :
        this(sink, store, authState::signedInUserId, System::currentTimeMillis)

    override fun ring(channelId: String, callId: String, callerId: String, callerName: String?) {
        val user = currentUser() ?: return
        val nowMs = now()
        val stored = store.shown(user, channelId)
        val info = CallInfo(callId, callerId, state = CallState.RINGING)
        // Ending, missed or ended: this call never rings again.
        if (!CallRules.ringingApplies(stored, info, nowMs)) return
        val live = CallRules.live(stored, nowMs)
        val since = if (live != null && live.callId == callId && live.phase == CallPhase.RINGING) {
            live.atMs
        } else {
            store.setShownIf(user, channelId, stored, ShownCall(callId, CallPhase.RINGING, nowMs))
            nowMs
        }
        sink.showCall { CallNotification(user, channelId, callId, CallKind.INCOMING, callerName, since, callerId) }
    }

    override fun stopRing(channelId: String, callId: String) {
        val user = currentUser()
        if (user != null) {
            val stored = store.shown(user, channelId)
            // Another, newer call of this DM keeps its notification.
            if (stored != null && stored.callId != callId) return
            if (stored?.phase == CallPhase.MISSED) return
            // Tombstone: a late or duplicate push ringing of this call is not accepted.
            store.setShownIf(user, channelId, stored, ShownCall(callId, CallPhase.ENDED, now()))
        }
        sink.cancelCall(channelId)
    }

    override fun missed(channelId: String, callId: String, callerName: String?) {
        val user = currentUser() ?: return
        sink.showCall {
            val stored = store.shown(user, channelId)
            when {
                // Already shown as missed (the push's ended, or a repeat).
                stored != null && stored.callId == callId && stored.phase == CallPhase.MISSED -> null
                // A newer call of this DM is shown: leave it.
                stored != null && stored.callId != callId &&
                    (stored.phase == CallPhase.RINGING || stored.phase == CallPhase.ENDING) -> null
                else -> {
                    store.setShown(user, channelId, ShownCall(callId, CallPhase.MISSED, now()))
                    CallNotification(user, channelId, callId, CallKind.MISSED, callerName)
                }
            }
        }
    }

    override fun stopAll() {
        sink.cancelAllCalls()
    }
}
