package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject

/** Result of handling one call wake. */
enum class CallOutcome {
    INCOMING,
    MISSED,
    CANCELLED,

    /** Unknown state or reason, another call than the shown one, a duplicate, or the work was stopped. */
    IGNORED,

    /** The notification was ready, but notifications are disabled. */
    NOTIFICATIONS_OFF,
    NOT_LOGGED_IN,

    /** The wake was queued for another account or server than the signed-in one. */
    WRONG_ACCOUNT,
}

/**
 * Handles a DM call wake (C20 addendum): `ringing` posts a heads-up "is
 * calling you" notification, `ended` turns it into a missed call
 * (`no_answer`, `cancelled`) or removes it (`all_declined`, `last_left`).
 * Only the channel's current call ([CallNotificationStore], set by
 * [CallWakeIntake]) is acted on; an ended with no call stored still shows a
 * missed call ([CallRules]). Never fetches messages; the caller's name is looked up in the DM
 * participants, with a generic text when that fails.
 *
 * Bound to the account and session the wake was queued for, like
 * [WakeFetchProcessor]: without that session nothing is posted.
 */
class CallWakeProcessor @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val refresher: TokenRefresher,
    private val names: NotificationTitleResolver,
    private val store: CallNotificationStore,
    private val sink: CallNotificationSink,
) {
    companion object {
        /** The access token must stay valid at least this long for the name lookup. */
        const val MIN_TOKEN_VALIDITY_MS = 30_000L

        /** A ringing call does not wait longer than this for the caller's name. */
        const val NAME_TIMEOUT_MS = 5_000L

        /** A ringing wake received longer ago than the ring timeout no longer posts. */
        const val RING_MAX_AGE_MS = 45_000L
    }

    internal var now: () -> Long = System::currentTimeMillis

    private data class Account(val userId: String, val generation: Long)

    /** [isStopped] reports that the work was stopped (replaced by a newer wake); checked right before posting. */
    suspend fun process(signal: WakeSignal, owner: WakeOwner, isStopped: () -> Boolean = { false }): CallOutcome {
        val call = signal.call
        if (signal.kind != WakeKind.CALL || call == null) return CallOutcome.IGNORED
        val generation = tokenStorage.getSessionGeneration()
        val userId = tokenStorage.getUserId()
        if (userId == null || tokenStorage.getRefreshToken() == null) return CallOutcome.NOT_LOGGED_IN
        if (owner.userId != userId || owner.serverUrl != tokenStorage.getServerUrl()) {
            return CallOutcome.WRONG_ACCOUNT
        }
        val account = Account(userId, generation)
        return when (call.state) {
            CallState.RINGING -> ringing(account, signal.channelId, call, isStopped)
            CallState.ENDED -> ended(account, signal.channelId, call, isStopped)
            CallState.UNKNOWN -> CallOutcome.IGNORED
        }
    }

    private suspend fun ringing(
        account: Account,
        channelId: String,
        call: CallInfo,
        isStopped: () -> Boolean,
    ): CallOutcome {
        // Only the call the intake accepted, while it still rings, and not after its ring timeout
        // (deferred work running late must not ring for a call that is long over).
        val stored = store.shown(account.userId, channelId)?.takeIf { isRinging(it, call) }
            ?: return CallOutcome.IGNORED
        if (now() - stored.atMs > RING_MAX_AGE_MS) return CallOutcome.IGNORED
        val name = callerName(channelId, call.callerId)
        return post(account, channelId, call, CallKind.INCOMING, name, isStopped) {
            isRinging(store.shown(account.userId, channelId), call)
        }
    }

    private suspend fun ended(
        account: Account,
        channelId: String,
        call: CallInfo,
        isStopped: () -> Boolean,
    ): CallOutcome {
        if (!CallRules.endedApplies(store.shown(account.userId, channelId), call)) return CallOutcome.IGNORED
        return when (call.reason) {
            CallEndReason.NO_ANSWER, CallEndReason.CANCELLED -> {
                val name = callerName(channelId, call.callerId)
                post(account, channelId, call, CallKind.MISSED, name, isStopped) {
                    CallRules.endedApplies(store.shown(account.userId, channelId), call)
                }
            }
            CallEndReason.ALL_DECLINED, CallEndReason.LAST_LEFT -> {
                if (!isCurrent(account)) return CallOutcome.NOT_LOGGED_IN
                sink.cancelCall(channelId)
                // Tombstone: a duplicate or reordered ringing of this call is not accepted again.
                store.setShown(account.userId, channelId, ShownCall(call.callId, CallPhase.ENDED, now()))
                CallOutcome.CANCELLED
            }
            CallEndReason.UNKNOWN, null -> CallOutcome.IGNORED
        }
    }

    private fun isRinging(stored: ShownCall?, call: CallInfo): Boolean =
        stored != null && stored.callId == call.callId && stored.phase == CallPhase.RINGING

    /**
     * Posts [kind] for [call] after the final checks in the presenter's
     * critical section ([stillApplies] re-reads the store); a missed call is recorded.
     */
    private fun post(
        account: Account,
        channelId: String,
        call: CallInfo,
        kind: CallKind,
        name: String?,
        isStopped: () -> Boolean,
        stillApplies: () -> Boolean,
    ): CallOutcome {
        if (!isCurrent(account)) return CallOutcome.NOT_LOGGED_IN
        var refused = CallOutcome.IGNORED
        val result = sink.showCall {
            when {
                isStopped() -> null
                !isCurrent(account) -> null.also { refused = CallOutcome.NOT_LOGGED_IN }
                !stillApplies() -> null
                else -> {
                    if (kind == CallKind.MISSED) {
                        store.setShown(account.userId, channelId, ShownCall(call.callId, CallPhase.MISSED, now()))
                    }
                    CallNotification(account.userId, channelId, call.callId, kind, name)
                }
            }
        }
        return when (result) {
            ShowResult.REFUSED -> refused
            ShowResult.DISABLED -> CallOutcome.NOTIFICATIONS_OFF
            ShowResult.POSTED -> if (kind == CallKind.MISSED) CallOutcome.MISSED else CallOutcome.INCOMING
        }
    }

    /** The caller's name, or null (generic text) when it cannot be looked up quickly. */
    private suspend fun callerName(channelId: String, callerId: String): String? {
        if (refresher.ensureFresh(MIN_TOKEN_VALIDITY_MS) != RefreshResult.SUCCESS) return null
        return withTimeoutOrNull(NAME_TIMEOUT_MS) { names.callerName(channelId, callerId) }
    }

    private fun isCurrent(account: Account): Boolean =
        tokenStorage.getSessionGeneration() == account.generation &&
            tokenStorage.getUserId() == account.userId &&
            tokenStorage.getRefreshToken() != null
}
