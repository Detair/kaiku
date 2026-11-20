package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.call.CallNotifier
import io.wolftown.kaiku.data.session.SessionGeneration
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The call notifications of [io.wolftown.kaiku.data.call.CallController],
 * posted through the presenter and recorded in the per-account store the
 * push intake writes too ([CallWakeIntake]). So a ring the socket and a
 * push both deliver alerts once, a ring stopped here is not rung again by a
 * late push, and a call is shown as missed once.
 *
 * Every post is for the signed-in session it names and is checked in one
 * step with the post, inside the presenter's critical section, which
 * [stopAll] shares: a session that ended posts nothing, and what it posted
 * before is removed.
 */
@Singleton
class SocketCallNotifier internal constructor(
    private val sink: CallNotificationSink,
    private val store: CallNotificationStore,
    private val now: () -> Long,
) : CallNotifier {

    @Inject
    constructor(sink: CallNotificationSink, store: CallNotificationStore) : this(sink, store, System::currentTimeMillis)

    override fun ring(session: SessionGeneration.Ticket, channelId: String, callId: String, callerId: String, callerName: String?) {
        val user = session.userId
        sink.showCall {
            if (!session.alive) return@showCall null
            val nowMs = now()
            val stored = store.shown(user, channelId)
            val info = CallInfo(callId, callerId, state = CallState.RINGING)
            // Ending, missed or ended: this call never rings again.
            if (!CallRules.ringingApplies(stored, info, nowMs)) return@showCall null
            val live = CallRules.live(stored, nowMs)
            // The ring's time is when it was first received (a push wake recorded it at its receipt).
            val since = if (live != null && live.callId == callId && live.phase == CallPhase.RINGING) {
                live.atMs
            } else {
                store.setShownIf(user, channelId, stored, ShownCall(callId, CallPhase.RINGING, nowMs))
                nowMs
            }
            CallNotification(user, channelId, callId, CallKind.INCOMING, callerName, since, callerId)
        }
    }

    override fun stopRing(session: SessionGeneration.Ticket, channelId: String, callId: String) {
        if (!session.alive) return
        val user = session.userId
        val stored = store.shown(user, channelId)
        // What counts is the call that is live there (a ring whose end never came is none).
        val live = CallRules.live(stored, now())
        // Another, newer call of this DM keeps its notification.
        if (live != null && live.callId != callId) return
        if (live?.phase == CallPhase.MISSED) return
        // Tombstone: a late or duplicate push ringing of this call is not accepted.
        store.setShownIf(user, channelId, stored, ShownCall(callId, CallPhase.ENDED, now()))
        sink.cancelCall(channelId)
    }

    override fun missed(session: SessionGeneration.Ticket, channelId: String, callId: String, callerName: String?) {
        val user = session.userId
        sink.showCall {
            if (!session.alive) return@showCall null
            val stored = CallRules.live(store.shown(user, channelId), now())
            when {
                // Already shown as missed (a repeat).
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

    override fun missedIfRinging(session: SessionGeneration.Ticket, channelId: String, callId: String, callerName: String?) {
        val user = session.userId
        sink.showCall {
            if (!session.alive) return@showCall null
            val stored = store.shown(user, channelId)
            // Only if the ring is the last thing known about that call (not declined, ended or missed since).
            if (stored == null || stored.callId != callId || stored.phase != CallPhase.RINGING) return@showCall null
            store.setShown(user, channelId, ShownCall(callId, CallPhase.MISSED, now()))
            CallNotification(user, channelId, callId, CallKind.MISSED, callerName)
        }
    }

    override fun stopAll() {
        sink.cancelAllCalls()
    }
}
