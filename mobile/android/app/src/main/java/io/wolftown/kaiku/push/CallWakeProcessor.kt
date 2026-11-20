package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.call.CallDoor
import io.wolftown.kaiku.data.call.CallInput
import io.wolftown.kaiku.data.call.CallRef
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject

/** Result of handling one call wake. */
enum class CallOutcome {
    /** A ringing wake was handed to the call owner (which rings, or keeps it as a busy ring). */
    INCOMING,

    /** An unanswered end was handed to the call owner (a missed call). */
    MISSED,

    /** Another end was handed to the call owner (the ring is removed). */
    CANCELLED,

    /** Unknown state or reason, another call than the shown one, a duplicate, or the work was stopped. */
    IGNORED,
    NOT_LOGGED_IN,

    /** The wake was queued for another account or server than the signed-in one. */
    WRONG_ACCOUNT,

    /** The DM is not the signed-in user's (e.g. a late wake for a previous account). */
    NOT_MEMBER,
}

/**
 * Handles a DM call wake (C20 addendum). It decides nothing about the call
 * itself: after its own checks (the account and session the wake was queued
 * for, the channel's current call in [CallNotificationStore] as set by
 * [CallWakeIntake], the wake's age, membership and the caller's name) it
 * hands the wake to the call owner ([CallDoor]), the same owner the socket's
 * call events go to. So a ring that comes by push follows the same rules as
 * one from the socket: not rung while another call is on, ended 100 s after
 * this wake was received, a missed call if it ends unanswered, one ring and
 * one missed call however many ways it arrived.
 *
 * Never fetches messages; a DM that is definitely not the user's
 * ([CallerLookup.NotMember]) hands over nothing.
 */
class CallWakeProcessor @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val refresher: TokenRefresher,
    private val names: NotificationTitleResolver,
    private val store: CallNotificationStore,
    private val door: CallDoor,
) {
    companion object {
        /** The access token must stay valid at least this long for the name lookup. */
        const val MIN_TOKEN_VALIDITY_MS = 30_000L

        /** A ringing call does not wait longer than this for the caller's name. */
        const val NAME_TIMEOUT_MS = 5_000L

        /** A ringing wake received longer ago than the ring timeout no longer rings. */
        const val RING_MAX_AGE_MS = 45_000L
    }

    internal var now: () -> Long = System::currentTimeMillis

    private data class Account(val userId: String, val generation: Long)

    /** [isStopped] reports that the work was stopped (replaced by a newer wake); checked right before the hand-over. */
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
        val name = when (val lookup = lookupCaller(channelId, call.callerId)) {
            is CallerLookup.Found -> lookup.name
            CallerLookup.Unknown -> null
            CallerLookup.NotMember -> return notMember(account, channelId, stored)
        }
        if (!isCurrent(account)) return CallOutcome.NOT_LOGGED_IN
        if (isStopped() || !isRinging(store.shown(account.userId, channelId), call)) return CallOutcome.IGNORED
        // The ring's time is this wake's receipt, as the intake recorded it.
        val ring = CallInput.PushRing(CallRef(account.userId, channelId, call.callId), call.callerId, name, stored.atMs)
        return if (door.fromPush(ring)) CallOutcome.INCOMING else CallOutcome.NOT_LOGGED_IN
    }

    private suspend fun ended(
        account: Account,
        channelId: String,
        call: CallInfo,
        isStopped: () -> Boolean,
    ): CallOutcome {
        val stored = store.shown(account.userId, channelId)
        if (!CallRules.endedApplies(stored, call, now())) return CallOutcome.IGNORED
        val (reason, unanswered) = when (call.reason) {
            CallEndReason.NO_ANSWER -> "no_answer" to true
            CallEndReason.CANCELLED -> "cancelled" to true
            CallEndReason.ALL_DECLINED -> "all_declined" to false
            CallEndReason.LAST_LEFT -> "last_left" to false
            CallEndReason.UNKNOWN, null -> return CallOutcome.IGNORED
        }
        // A missed call names the caller. Whether the conversation is one-to-one decides what an
        // unanswered end says about a ring that has already stopped (see CallInput.PushEnded).
        val found = if (!unanswered) null else when (val lookup = lookupCaller(channelId, call.callerId)) {
            is CallerLookup.Found -> lookup
            CallerLookup.Unknown -> null
            CallerLookup.NotMember -> return notMember(account, channelId, stored)
        }
        if (!isCurrent(account)) return CallOutcome.NOT_LOGGED_IN
        if (isStopped() || !CallRules.endedApplies(store.shown(account.userId, channelId), call, now())) {
            return CallOutcome.IGNORED
        }
        val end = CallInput.PushEnded(
            CallRef(account.userId, channelId, call.callId), call.callerId, found?.name, reason, found?.direct,
            ringReceivedAtMs = stored?.takeIf { it.callId == call.callId }?.ringReceivedAtMs,
        )
        if (!door.fromPush(end)) return CallOutcome.NOT_LOGGED_IN
        return if (unanswered) CallOutcome.MISSED else CallOutcome.CANCELLED
    }

    /**
     * The DM is not the user's: nothing is handed over, and what the intake
     * recorded for this channel ([stored], if still unchanged) is removed.
     */
    private fun notMember(account: Account, channelId: String, stored: ShownCall?): CallOutcome {
        if (stored != null && isCurrent(account)) store.clearIf(account.userId, channelId, stored)
        return CallOutcome.NOT_MEMBER
    }

    private fun isRinging(stored: ShownCall?, call: CallInfo): Boolean =
        stored != null && stored.callId == call.callId && stored.phase == CallPhase.RINGING

    /** Membership and caller name; [CallerLookup.Unknown] (generic text) when it cannot be looked up quickly. */
    private suspend fun lookupCaller(channelId: String, callerId: String): CallerLookup {
        if (refresher.ensureFresh(MIN_TOKEN_VALIDITY_MS) != RefreshResult.SUCCESS) return CallerLookup.Unknown
        return withTimeoutOrNull(NAME_TIMEOUT_MS) { names.lookupCaller(channelId, callerId) } ?: CallerLookup.Unknown
    }

    private fun isCurrent(account: Account): Boolean =
        tokenStorage.getSessionGeneration() == account.generation &&
            tokenStorage.getUserId() == account.userId &&
            tokenStorage.getRefreshToken() != null
}
