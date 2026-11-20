package io.wolftown.kaiku.data.session

import io.wolftown.kaiku.data.api.SessionChangedException
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import java.util.concurrent.CopyOnWriteArrayList
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The process-wide session generation: one number for "whoever is signed in
 * now", advanced synchronously by [SessionEndHook] right before a stored
 * session is removed or replaced, before anything else happens.
 *
 * Everything asynchronous that belongs to a signed-in account (requests,
 * queues, timers, collectors, notification posts, answers that arrive late)
 * takes a [Ticket] when it is created and is inert once the generation
 * moved: it does not run, send, publish or post. The pieces that make this
 * hold:
 * - [Ticket.scope]: coroutines of a generation run in a scope that
 *   [advance] cancels, before the token store changes;
 * - [Ticket.alive], checked at every point of no return (in one critical
 *   section with what a sign-out resets);
 * - [Ticket.requestSession]: requests carry the session they were built for
 *   through the HTTP client's required-session check, so they never leave
 *   with another account's token;
 * - [onAdvance]: caches of a generation are cleared by the advance itself.
 *
 * While a sign-out is running ([close], the first teardown step) the ending
 * session takes no new work ([Ticket.open]); what is already running ends
 * with the advance.
 */
@Singleton
class SessionGeneration internal constructor(
    private val session: () -> AuthSession,
    /** The stored account of a process that has not restored its session (a push wake), or null. */
    private val storedAccount: () -> String? = { null },
) {
    @Inject
    constructor(authState: AuthState, tokenStorage: TokenStorage) : this(
        { authState.session.value },
        { tokenStorage.getUserId()?.takeIf { tokenStorage.getRefreshToken() != null } },
    )

    /** For a component used on its own (tests): the session of [authState], no stored account. */
    internal constructor(authState: AuthState) : this({ authState.session.value })

    /** Work of one account in one generation. Two tickets are equal when both are. */
    class Ticket internal constructor(
        private val owner: SessionGeneration,
        val number: Long,
        val userId: String,
    ) {
        /** The generation has not moved and this account is still the signed-in one. */
        val alive: Boolean get() = owner.isAlive(this)

        /** [alive], and no sign-out of this session has started. */
        val open: Boolean get() = owner.isOpen(this)

        /** A child of [parent] that also ends with this ticket's generation. */
        fun scope(parent: CoroutineScope): CoroutineScope = owner.scopeFor(this, parent)

        /**
         * The session id a request of this ticket is bound to (the HTTP
         * client's required-session check). Throws [SessionChangedException]
         * when that session is not the current one: the request is not sent.
         */
        fun requestSession(): Long = owner.requestSession(this)

        override fun equals(other: Any?): Boolean =
            other is Ticket && other.owner === owner && other.number == number && other.userId == userId

        override fun hashCode(): Int = 31 * number.hashCode() + userId.hashCode()

        override fun toString(): String = "Ticket($number)"
    }

    private val lock = Any()

    @Volatile
    private var number = 1L

    /** Sessions (by `AuthSession.LoggedIn.sessionId`) up to this one ended in a hook. */
    @Volatile
    private var endedUpTo = 0L

    /** Sessions up to this one are being signed out: no new work. */
    @Volatile
    private var closingUpTo = 0L

    /** Scopes of the current generation; guarded by [lock]. */
    private val scopes = ArrayList<Job>()

    private val resets = CopyOnWriteArrayList<() -> Unit>()

    /** The current generation's number (for logs and tests). */
    val current: Long get() = number

    private class Account(val userId: String, val sessionId: Long?)

    private fun account(): Account? {
        val now = session()
        if (now is AuthSession.LoggedIn) {
            return if (now.sessionId > endedUpTo) Account(now.userId, now.sessionId) else null
        }
        return storedAccount()?.let { Account(it, null) }
    }

    /**
     * A ticket for the signed-in account, or null when nobody is signed in
     * (or the session is just ending) or, with [account], when someone else is.
     */
    fun enter(account: String? = null): Ticket? {
        val generation = number
        val now = account() ?: return null
        if (account != null && now.userId != account) return null
        return Ticket(this, generation, now.userId).takeIf { isAlive(it) }
    }

    private fun isAlive(ticket: Ticket): Boolean =
        ticket.number == number && account()?.userId == ticket.userId

    private fun isOpen(ticket: Ticket): Boolean {
        if (!isAlive(ticket)) return false
        val now = session()
        return now !is AuthSession.LoggedIn || now.sessionId > closingUpTo
    }

    private fun requestSession(ticket: Ticket): Long {
        val now = session() as? AuthSession.LoggedIn
        if (now == null || ticket.number != number || now.userId != ticket.userId || now.sessionId <= endedUpTo) {
            throw SessionChangedException()
        }
        return now.sessionId
    }

    private fun scopeFor(ticket: Ticket, parent: CoroutineScope): CoroutineScope {
        val job = SupervisorJob(parent.coroutineContext[Job])
        val current = synchronized(lock) {
            if (ticket.number == number) {
                scopes += job
                true
            } else {
                false
            }
        }
        if (current) {
            job.invokeOnCompletion { synchronized(lock) { scopes.remove(job) } }
        } else {
            job.cancel(Ended())
        }
        return CoroutineScope(parent.coroutineContext + job)
    }

    /** Runs [reset] in every [advance], on the advancing thread: quick, no blocking, no WebRTC. */
    fun onAdvance(reset: () -> Unit) {
        resets += reset
    }

    /**
     * A sign-out of session [sessionId] has started: from here on that
     * session takes no new work. Its running work ends with [advance].
     */
    fun close(sessionId: Long?) {
        if (sessionId == null) return
        synchronized(lock) { if (sessionId > closingUpTo) closingUpTo = sessionId }
    }

    /**
     * The stored session is about to be removed or replaced: a new
     * generation starts. Synchronous: when it returns, no ticket of the old
     * generation is alive, its scopes are cancelled and its caches cleared.
     */
    fun advance() {
        val ending = session() as? AuthSession.LoggedIn
        val ended = synchronized(lock) {
            number++
            if (ending != null && ending.sessionId > endedUpTo) endedUpTo = ending.sessionId
            scopes.toList().also { scopes.clear() }
        }
        ended.forEach { it.cancel(Ended()) }
        for (reset in resets) {
            try {
                reset()
            } catch (e: Exception) {
                logger.log(Level.WARNING, "session reset failed: ${e::class.simpleName}")
            }
        }
    }

    /** The generation a piece of work belonged to is over. */
    class Ended : CancellationException("session generation ended")

    private companion object {
        val logger: Logger = Logger.getLogger("SessionGeneration")
    }
}
