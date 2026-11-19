package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.concurrent.atomic.AtomicBoolean
import java.util.logging.Level
import java.util.logging.Logger

/** What the voice connection is doing, for the voice screen and the ongoing-call notification. */
sealed interface VoiceLinkState {
    /** No voice session, or the session is connected. */
    data object Stable : VoiceLinkState

    /**
     * The session has not been connected yet: its first `voice_join` is held
     * until a socket is authenticated, or its publisher is not connected yet
     * (at most [VoiceSignalling.ATTEMPT_TIMEOUT_MS] after the join went out).
     */
    data object Connecting : VoiceLinkState

    /**
     * The connection is degraded and nothing was torn down: ICE reported
     * DISCONNECTED (it may recover by itself), or no authenticated socket
     * carries the session's `voice_join` and the session waits for one.
     * A "reconnecting" indication only.
     */
    data object Interrupted : VoiceLinkState

    /** Rejoin attempt [attempt] (1-based) is pending or running. */
    data class Rejoining(val attempt: Int) : VoiceLinkState
}

/**
 * The signalling state machine of a voice session (contract C32 with its
 * amendments): when the first `voice_join` goes out, when the channel is
 * rejoined and when the session is given up.
 *
 * Pure logic (no Android, no WebRTC) so it runs on virtual time in unit tests.
 * It decides *when*; *how* each step is done is [Steps].
 *
 * **One owner.** [run] is the machine. One coroutine runs it per session, and
 * nothing else decides to send a `voice_join`. Whoever ends the session (own
 * leave, `voice_removed`, channel switch) cancels that coroutine and waits
 * for it.
 *
 * **Level-triggered.** The machine never waits for something to *happen*.
 * Every wait is on a predicate over state that can be read at any time:
 * - [socket], the id of the authenticated socket (null: none). Ids never
 *   repeat, so "socket X is not the authenticated socket" stays true once it
 *   is true, and a socket that was lost and replaced between two looks is
 *   seen as what it is: another socket.
 * - whether the session's connections have failed ([Steps.awaitConnectionFailed]).
 *
 * So no wake-up can be lost: whatever happened while the machine was busy is
 * in the state it reads next.
 *
 * **Socket loss** is one comparison: the authenticated socket is not the one
 * that carried the session's last `voice_join` ([Steps.joinSocket]; the
 * server drops a voice peer with its socket). The rejoin, the attempt budget
 * and the 60 s deadline all follow from it:
 *
 * | Mode | Leaves it when | Next |
 * |---|---|---|
 * | first join (held while no socket is authenticated) | the `voice_join` went out on the authenticated socket and its publisher connected | up |
 * | | the publisher did not connect within 15 s | give up ("couldn't connect") |
 * | | the socket that carried the join is lost before the publisher connected | recovering, 0 attempts used |
 * | up on socket S | the authenticated socket is not S, or the connections failed | recovering, 0 attempts used |
 * | recovering, n used: delay (1 s, 3 s, 9 s), then one attempt of at most 15 s | attempt connected | up on that socket |
 * | | attempt failed or timed out | n + 1 used; the third failure gives up |
 * | | the socket is another one (or none) | same n: not used up; again on the next socket |
 * | any | for 60 s in one stretch, no authenticated socket carried the session's `voice_join` | give up |
 *
 * **The 60 s deadline** measures the time without an authenticated socket
 * that carries the session's `voice_join`. It starts when the socket that
 * carried the join is lost (for a first join: when the session starts) and
 * ends only when a `voice_join` of the session went out on an authenticated
 * socket. A socket that merely authenticates does not end it: sockets that
 * never stay up for the delay before an attempt end the session after 60 s.
 * Once the join is carried, an attempt that does not connect is a matter of
 * its 15 s and of the three attempts; if that socket is lost again, new 60 s
 * start from that loss.
 *
 * ICE `DISCONNECTED` is an indication only ([VoiceLinkState.Interrupted]).
 *
 * Until the session was connected once, its state reads
 * [VoiceLinkState.Connecting] (also while it is held for a socket or
 * recovering), and a give-up says [GiveUp.NeverConnected].
 */
class VoiceSignalling(
    private val socket: StateFlow<Long?>,
    private val delaysMs: List<Long> = DEFAULT_DELAYS_MS,
    private val attemptTimeoutMs: Long = ATTEMPT_TIMEOUT_MS,
    private val joinDeadlineMs: Long = JOIN_DEADLINE_MS,
    /** Real elapsed time incl. suspension (C32); the deadline counts the larger of it and the delays waited. */
    private val clock: ElapsedClock = ElapsedClock.STOPPED,
) {
    companion object {
        private val logger = Logger.getLogger("VoiceSignalling")

        val DEFAULT_DELAYS_MS = listOf(1_000L, 3_000L, 9_000L)
        const val ATTEMPT_TIMEOUT_MS = 15_000L

        /** How long a session may be without an authenticated socket that carries its `voice_join`. */
        const val JOIN_DEADLINE_MS = 60_000L

        /**
         * The deadline wakes up at least this often to compare the real
         * clock: after a device suspension the coroutine delay alone would
         * still be counting.
         */
        const val DEADLINE_CHECK_MS = 5_000L

        /**
         * A carried session looks at the clock every [DEADLINE_CHECK_MS]; a
         * look that comes this much later than due is a gap in the client's
         * own activity (the device slept), after which "carried" must be
         * proven again (C32 "proof of carried").
         */
        const val GAP_MS = 10_000L

        /** How long a proof may take: a fresh inbound frame within 5 s (C32). */
        const val PROOF_TIMEOUT_MS = 5_000L
    }

    /** How a rejoin attempt ended. */
    enum class Attempt {
        /** The publisher connection of this attempt is connected. */
        Connected,

        /** It did not connect; one attempt of the budget is used up. */
        Failed,

        /** Its `voice_join` could not go out on the socket it was for. Not used up. */
        SocketGone,

        /**
         * The session may not rejoin at all (a DM call that is over on the
         * server, C32 "DM calls"); nothing was sent. The session is given up
         * at once with [GiveUp.Refused].
         */
        Refused,
    }

    /** Why the machine gave the session up ([Steps.giveUp]). */
    enum class GiveUp {
        /** The session was never connected: its first join did not connect, or all attempts after it failed. */
        NeverConnected,

        /** The session was connected and could not be recovered (attempts or the 60 s deadline). */
        Lost,

        /** A rejoin was refused ([Attempt.Refused]): the call it belongs to is over. */
        Refused,
    }

    /** The steps of one session. Each is called by the owner only, one at a time. */
    interface Steps {
        /**
         * The socket that carried the session's last `voice_join`, or null
         * while none went out. It changes at the very moment a `voice_join`
         * goes out ([join], [rejoin]) and at no other time; the 60 s
         * deadline runs whenever it is not the authenticated socket.
         */
        val joinSocket: StateFlow<Long?>

        /** Everything the first join needs before its `voice_join`. Uses no socket. Throws if the join cannot be set up. */
        suspend fun prepare()

        /**
         * Sends the first `voice_join` on [socket] and completes the join.
         * Returns false, having sent nothing, if [socket] is not the
         * authenticated socket any more or refused the frame. Throws if the
         * join fails.
         */
        suspend fun join(socket: Long): Boolean

        /** Returns once the publisher of the session's current connections is connected. */
        suspend fun awaitConnected()

        /** Returns once the session's current connections have failed for good (ICE `FAILED`). */
        suspend fun awaitConnectionFailed()

        /**
         * C32 "proof of carried", after a gap: the publisher read from the
         * connection is live and a fresh inbound frame arrives on [socket]
         * (a ping is sent for it). Returns false if not, within
         * [PROOF_TIMEOUT_MS] at most (the machine also bounds it).
         */
        suspend fun proveCarried(socket: Long): Boolean

        /**
         * One rejoin attempt on [socket]: new connections, `voice_join`, wait
         * for the publisher. Whatever ends it short of [Attempt.Connected]
         * (also its cancellation) closes the connections it created.
         */
        suspend fun rejoin(socket: Long): Attempt

        /** Ends the call locally. Called once, when the machine is over, while [state] still says "reconnecting" or "connecting". */
        suspend fun giveUp(why: GiveUp)
    }

    private sealed interface Mode {
        data object FirstJoin : Mode
        data class Up(val socket: Long) : Mode
        data class Recovering(val failures: Int) : Mode
    }

    /** The state of one [run]. */
    private class Session {
        @Volatile
        var mode: Mode = Mode.FirstJoin

        /**
         * The last socket that could not take a `voice_join` of this session:
         * it was replaced, or it refused the frame while it still was the
         * authenticated one (then it is closing). As ids never repeat, the
         * owner treats it as no socket from then on and waits for another.
         */
        val unusable = MutableStateFlow<Long?>(null)

        /** Whether the session's publisher was connected at least once. */
        @Volatile
        var connectedOnce = false

        /**
         * Set when a proof failed: the clock time of the last proof, from
         * which the 60 s count (the session was not carried since then).
         * Read once by the deadline.
         */
        @Volatile
        var notCarriedSinceMs: Long? = null
    }

    private val lock = Any()
    private val _state = MutableStateFlow<VoiceLinkState>(VoiceLinkState.Stable)

    /** Derived from the machine's mode, the socket and the ICE indication; see [refresh]. */
    val state: StateFlow<VoiceLinkState> = _state.asStateFlow()

    // Guarded by [lock].
    private var current: Session? = null
    private var iceDisconnected = false

    /**
     * Owns the signalling of one session. Returns after it gave the session
     * up ([Steps.giveUp] ran); otherwise it runs until it is cancelled.
     * Throws what [Steps.prepare] or [Steps.join] throw: the first join failed.
     */
    suspend fun run(steps: Steps) {
        val session = Session()
        synchronized(lock) {
            current = session
            iceDisconnected = false
        }
        refresh()
        try {
            val ended = withinJoinDeadline(session, steps) { signal(session, steps) }
            logger.info(
                when (ended) {
                    End.AttemptsUsedUp -> "voice session gave up: ${delaysMs.size} rejoin attempts failed"
                    End.FirstJoinTimedOut ->
                        "voice session gave up: the first join did not connect within ${attemptTimeoutMs / 1000} s"
                    End.Refused -> "voice session gave up: its rejoin was refused"
                    null -> "voice session gave up: no socket carried its voice_join for ${joinDeadlineMs / 1000} s"
                }
            )
            steps.giveUp(
                when {
                    ended == End.Refused -> GiveUp.Refused
                    session.connectedOnce -> GiveUp.Lost
                    else -> GiveUp.NeverConnected
                }
            )
        } finally {
            synchronized(lock) {
                if (current === session) {
                    current = null
                    iceDisconnected = false
                }
            }
            refresh()
        }
    }

    /** The publisher reported ICE `DISCONNECTED`: indication only, it may recover by itself. */
    fun onIceDisconnected() {
        synchronized(lock) { iceDisconnected = true }
        refresh()
    }

    /** The publisher is connected (again). */
    fun onIceConnected() {
        synchronized(lock) { iceDisconnected = false }
        refresh()
    }

    // -- The machine ------------------------------------------------------------

    /** How "up" ended: the connections failed (null), or no proof after a gap since that time. */
    private data class UpEnd(val notProvenSinceMs: Long?)

    /** How [signal] ended by itself. */
    private enum class End { AttemptsUsedUp, FirstJoinTimedOut, Refused }

    /** Returns when the session is given up; otherwise runs until cancelled. */
    private suspend fun signal(session: Session, steps: Steps): End {
        steps.prepare()

        // First join: held until a socket is authenticated, then sent on
        // exactly that socket. A refusal goes around on the state as it is now.
        var joined: Long
        while (true) {
            val id = usableSocket(session)
            if (steps.join(id)) {
                joined = id
                break
            }
            session.unusable.value = id
        }

        // The first join counts once its publisher is connected (C32 amendment:
        // the same criterion and the same 15 s as a rejoin attempt). If the
        // socket that carried it is lost first, the session recovers.
        val first = whileSocketIs(joined) {
            withTimeoutOrNull(attemptTimeoutMs) { steps.awaitConnected() } != null
        }
        when (first) {
            true -> session.connectedOnce = true
            false -> return End.FirstJoinTimedOut
            null -> Unit
        }

        // A first join whose socket was lost before it connected goes straight to recovering.
        var up = first == true
        while (true) {
            if (up) {
                // Up: nothing to do while the socket that carried the join is the
                // authenticated one and the connections hold.
                enter(session, Mode.Up(joined))
                val lostAt = whileSocketIs(joined) {
                    firstOf({ steps.awaitConnectionFailed(); UpEnd(null) }, { UpEnd(watchCarried(steps, joined)) })
                }?.notProvenSinceMs
                if (lostAt != null) {
                    // Not proven: the session is not carried since its last proof. The socket
                    // is treated as gone; the 60 s count from that proof.
                    logger.info("voice session not proven carried after a gap")
                    session.notCarriedSinceMs = lostAt
                    session.unusable.value = joined
                }
                refresh()
            }
            up = true

            // Recovering, with a fresh budget. Until a socket is there, the
            // state stays "interrupted". If the socket is another one, the
            // 60 s deadline has been running since it changed.
            var failures = 0
            var id = usableSocket(session)
            recovering@ while (true) {
                enter(session, Mode.Recovering(failures))
                val outcome = whileSocketIs(id) {
                    delay(delaysMs[failures])
                    attempt(steps, id, number = failures + 1)
                } ?: Attempt.SocketGone
                when (outcome) {
                    Attempt.Connected -> break@recovering
                    Attempt.Refused -> return End.Refused
                    Attempt.Failed -> {
                        logger.info("voice rejoin attempt ${failures + 1}/${delaysMs.size} failed")
                        if (++failures == delaysMs.size) return End.AttemptsUsedUp
                    }
                    Attempt.SocketGone -> {
                        logger.info("voice rejoin attempt ${failures + 1} interrupted by socket loss")
                        session.unusable.value = id
                        id = usableSocket(session)
                    }
                }
            }
            joined = id
            session.connectedOnce = true
            synchronized(lock) { iceDisconnected = false }
        }
    }

    /**
     * While the session is up on [socket]: looks at the clock every
     * [DEADLINE_CHECK_MS]. After a gap ([GAP_MS]) the session must prove that
     * it is still carried; returns the clock time of the last proof once it
     * cannot. Otherwise runs until cancelled.
     */
    private suspend fun watchCarried(steps: Steps, socket: Long): Long {
        // A clock that never moves (unit tests) cannot show a gap: nothing to watch.
        if (clock === ElapsedClock.STOPPED) awaitCancellation()
        var lastProof = clock.nowMs()
        while (true) {
            delay(DEADLINE_CHECK_MS)
            val now = clock.nowMs()
            if (now - lastProof > GAP_MS) {
                val proven = withTimeoutOrNull(PROOF_TIMEOUT_MS) { steps.proveCarried(socket) } == true
                if (!proven) return lastProof
                lastProof = clock.nowMs()
            } else {
                lastProof = now
            }
        }
    }

    /** The result of whichever of [a] and [b] returns first; the other is cancelled. */
    private suspend fun <T> firstOf(a: suspend () -> T, b: suspend () -> T): T = coroutineScope {
        val result = CompletableDeferred<T>()
        val ja = launch { result.complete(a()) }
        val jb = launch { result.complete(b()) }
        try {
            result.await()
        } finally {
            ja.cancel()
            jb.cancel()
        }
    }

    /** One attempt, at most [attemptTimeoutMs]. Anything it throws by itself is a failed attempt. */
    private suspend fun attempt(steps: Steps, id: Long, number: Int): Attempt = try {
        withTimeoutOrNull(attemptTimeoutMs) { steps.rejoin(id) } ?: Attempt.Failed
    } catch (e: CancellationException) {
        // Cancelled from outside (socket gone, deadline, session end): not ours to judge.
        currentCoroutineContext().ensureActive()
        logger.log(Level.WARNING, "voice rejoin attempt $number was cancelled from inside", e)
        Attempt.Failed
    } catch (e: Exception) {
        logger.log(Level.WARNING, "voice rejoin attempt $number threw", e)
        Attempt.Failed
    }

    /**
     * The authenticated socket, unless it is one this session found unusable:
     * then, and while none is authenticated, there is no socket for it.
     */
    private fun usableSockets(session: Session): Flow<Long?> =
        combine(socket, session.unusable) { id, unusable -> id.takeIf { it != unusable } }.distinctUntilChanged()

    /** The usable socket, as soon as there is one. */
    private suspend fun usableSocket(session: Session): Long = usableSockets(session).filterNotNull().first()

    /**
     * Runs [block] while [id] is the authenticated socket. Returns its result,
     * or null once the authenticated socket is another one or none; [block]
     * has finished cancelling by then.
     */
    private suspend fun <T : Any> whileSocketIs(id: Long, block: suspend () -> T): T? = coroutineScope {
        val gone = AtomicBoolean(false)
        val work = async { block() }
        val watch = launch {
            socket.first { it != id }
            gone.set(true)
            work.cancel()
        }
        try {
            work.await()
        } catch (e: CancellationException) {
            // Our own cancellation wins, and so does a step that ended by itself.
            currentCoroutineContext().ensureActive()
            if (!gone.get()) throw e
            null
        } finally {
            watch.cancel()
        }
    }

    /** Which socket carried the session's `voice_join`, and whether it is the authenticated one. */
    private data class Carrier(val joinSocket: Long?, val authenticated: Boolean)

    /**
     * Runs [block] under the 60 s deadline: once no authenticated socket has
     * carried the session's `voice_join` for [joinDeadlineMs] in one stretch,
     * [block] is cancelled, wherever it is (a first join that is held or
     * still setting up, the delay before an attempt, an attempt that has not
     * sent its join yet). Returns what [block] returned, or null if the
     * deadline cut it short; it has finished cancelling by then.
     *
     * The clock is a function of two states, the authenticated socket and
     * [Steps.joinSocket]: it runs while they differ (or there is no join
     * socket yet). The authenticated socket changing from one that does not
     * carry the join to another one, or to none, changes nothing. A join that
     * went out and whose socket was lost again before anyone looked still
     * shows as another join socket, so its new 60 s start.
     */
    private suspend fun <T : Any> withinJoinDeadline(
        session: Session,
        steps: Steps,
        block: suspend () -> T,
    ): T? = coroutineScope {
        val expired = AtomicBoolean(false)
        val work = async { block() }
        val watchdog = launch {
            // Keeps [state] in step with the socket.
            launch { usableSockets(session).collect { refresh() } }
            // A socket whose proof failed ([Session.unusable]) carries nothing.
            combine(socket, steps.joinSocket, session.unusable) { authenticated, joined, unusable ->
                Carrier(joined, authenticated = joined != null && joined == authenticated && authenticated != unusable)
            }.distinctUntilChanged().collectLatest { carrier ->
                if (!carrier.authenticated) {
                    val since = session.notCarriedSinceMs
                    session.notCarriedSinceMs = null
                    awaitElapsed(joinDeadlineMs, since ?: clock.nowMs())
                    expired.set(true)
                    work.cancel()
                }
            }
        }
        try {
            work.await()
        } catch (e: CancellationException) {
            currentCoroutineContext().ensureActive()
            if (!expired.get()) throw e
            null
        } finally {
            watchdog.cancel()
        }
    }

    /**
     * Waits until [ms] of real time have passed (C32): the larger of the
     * delays waited and the [clock]'s difference, which also counts time the
     * device was suspended. Wakes at least every [DEADLINE_CHECK_MS] to
     * compare, so a long sleep is noticed at the first wake-up.
     */
    private suspend fun awaitElapsed(ms: Long, start: Long) {
        var waited = 0L
        while (true) {
            val elapsed = maxOf(waited, clock.nowMs() - start)
            if (elapsed >= ms) return
            val step = minOf(ms - elapsed, DEADLINE_CHECK_MS)
            delay(step)
            waited += step
        }
    }

    private fun enter(session: Session, mode: Mode) {
        session.mode = mode
        refresh()
    }

    private fun refresh() {
        synchronized(lock) {
            val session = current
            val id = socket.value.takeIf { it != session?.unusable?.value }
            _state.value = when (val mode = session?.mode) {
                null -> VoiceLinkState.Stable
                Mode.FirstJoin -> VoiceLinkState.Connecting
                is Mode.Up -> if (id != mode.socket || iceDisconnected) VoiceLinkState.Interrupted else VoiceLinkState.Stable
                is Mode.Recovering ->
                    if (session.connectedOnce) VoiceLinkState.Rejoining(mode.failures + 1) else VoiceLinkState.Connecting
            }
        }
    }
}
