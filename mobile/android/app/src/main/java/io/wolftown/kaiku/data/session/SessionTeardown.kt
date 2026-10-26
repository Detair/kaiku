package io.wolftown.kaiku.data.session

import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.sync.ResyncCoordinator
import io.wolftown.kaiku.data.ws.RealtimeConnectionManager
import io.wolftown.kaiku.di.AuthCoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import java.util.concurrent.atomic.AtomicBoolean
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * One step of ending a session on this device. Bound with Hilt `@IntoSet`
 * (see `SessionTeardownModule`) and run by [SessionTeardownRunner] in
 * ascending [order]:
 *
 * - 0: reserved for push unregistration (push plan Task 7), which must run
 *   first while the session token still works
 * - 10: leave voice ([VoiceSessionTeardown])
 * - 20: close the realtime connection ([RealtimeSessionTeardown])
 * - 30: clear chat, server and DM caches ([CacheSessionTeardown])
 *
 * Every step must be idempotent and must not log tokens.
 */
interface SessionTeardown {
    val order: Int

    /**
     * Tears down what belongs to the ended session [sessionId] (null when
     * nobody was logged in). Must leave a newer session alone.
     */
    suspend fun onLogout(sessionId: Long?)
}

/**
 * Runs every [SessionTeardown] once per ended session.
 *
 * Sessions are identified by [AuthSession.LoggedIn.sessionId]. For each
 * session there is one shared teardown [Job] on the runner's application
 * scope; [tearDown] starts it or returns the running/completed one, so every
 * trigger is harmless:
 * - the explicit logout ([io.wolftown.kaiku.data.repository.AuthRepository.logout])
 *   joins it before the server logout and the token clear;
 * - [start] observes [AuthState.endedSessions] (emitted by the transition
 *   itself, never conflated): every session that ends for any reason (logout,
 *   refresh rejected, WebSocket auth reject cap, PASSWORD_CHANGE_REQUIRED) is
 *   torn down. The HTTP client and the connection manager only clear tokens
 *   and set LoggedOut; this observer is their hook, without a DI cycle.
 *
 * The job does not belong to the caller: a ViewModel that dies mid-logout
 * cancels only its own wait, the remaining steps still run. A job that ended
 * abnormally (cancelled with the scope) releases its claim, so a later
 * trigger runs the teardown again.
 *
 * Steps run one at a time ([stepMutex]), in ascending order, each bounded by
 * [STEP_TIMEOUT_MS]; a failure or timeout is logged (step name and exception
 * type only) and the next step runs. If a newer session is current when a
 * step is due, the remaining steps are skipped so its state is left alone.
 * That is only a safety net: logins wait for [awaitIdle] first.
 */
@Singleton
class SessionTeardownRunner @Inject constructor(
    teardowns: Set<@JvmSuppressWildcards SessionTeardown>,
    private val authState: AuthState,
    @AuthCoroutineScope private val scope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("SessionTeardownRunner")

        /** Upper bound for one teardown step. */
        const val STEP_TIMEOUT_MS = 3_000L
    }

    private val steps: List<SessionTeardown> = teardowns.sortedBy { it.order }
    private val stepMutex = Mutex()
    private val started = AtomicBoolean(false)

    /** Teardown job per session id; guarded by itself. */
    private val jobs = HashMap<Long, Job>()

    /** Running teardowns without a session (logout while logged out); guarded by [jobs]. */
    private val anonymousJobs = HashSet<Job>()

    /**
     * Waits until no teardown is pending or running, bounded by the steps'
     * timeouts. Every login calls this before it stores tokens or sets
     * LoggedIn, so the previous session's cleanup always completes first and
     * never has to skip steps for a newer session. A session that just ended
     * but whose observer has not run yet is started here.
     */
    suspend fun awaitIdle() {
        authState.lastEndedSessionId?.let { tearDown(it) }
        val pending = synchronized(jobs) {
            (jobs.values + anonymousJobs).filter { it.isActive }
        }
        if (pending.isEmpty()) return
        val idle = withTimeoutOrNull((steps.size + 1) * STEP_TIMEOUT_MS) { pending.joinAll() }
        if (idle == null) logger.warning("teardown still running, continuing")
    }

    /** Starts observing ended sessions; idempotent. */
    fun start() {
        if (!started.compareAndSet(false, true)) return
        // UNDISPATCHED: subscribed before start() returns (endedSessions has no replay).
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            authState.endedSessions.collect { ended ->
                // Only starts the job; never runs the teardown inside collect.
                tearDown(ended.sessionId)
            }
        }
    }

    /**
     * Starts the teardown of [sessionId], or returns the job already running
     * or completed for it. With a null [sessionId] (nobody was logged in) a
     * new run always starts; the steps are idempotent. Join the result to
     * wait; cancelling the wait does not cancel the teardown.
     */
    fun tearDown(sessionId: Long?): Job {
        if (sessionId == null) {
            synchronized(jobs) {
                val job = scope.launch(start = CoroutineStart.LAZY) { runSteps(null) }
                anonymousJobs += job
                job.invokeOnCompletion { synchronized(jobs) { anonymousJobs -= job } }
                job.start()
                return job
            }
        }
        synchronized(jobs) {
            val existing = jobs[sessionId]
            // Running or completed normally: share it. Cancelled/failed: run again.
            if (existing != null && !existing.isCancelled) return existing
            val job = scope.launch(start = CoroutineStart.LAZY) { runSteps(sessionId) }
            jobs[sessionId] = job
            job.invokeOnCompletion { cause ->
                // Abnormal end: release the claim so a later trigger retries.
                if (cause != null) synchronized(jobs) { if (jobs[sessionId] === job) jobs.remove(sessionId) }
            }
            job.start()
            return job
        }
    }

    private suspend fun runSteps(sessionId: Long?) {
        stepMutex.withLock {
            for (step in steps) {
                if (isSuperseded(sessionId)) {
                    logger.info("a newer session started, remaining teardown skipped")
                    return
                }
                runStep(step, sessionId)
            }
        }
    }

    private fun isSuperseded(sessionId: Long?): Boolean {
        val now = authState.session.value
        return now is AuthSession.LoggedIn && now.sessionId != sessionId
    }

    private suspend fun runStep(step: SessionTeardown, sessionId: Long?) {
        val name = step::class.simpleName
        try {
            val finished = withTimeoutOrNull(STEP_TIMEOUT_MS) {
                step.onLogout(sessionId)
                true
            }
            if (finished == null) logger.warning("teardown $name timed out")
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.warning("teardown $name failed: ${e::class.simpleName}")
        }
    }
}

/** Order 10: leaves the voice channel (stops the call service and releases media). */
class VoiceSessionTeardown @Inject constructor(
    private val voiceRepository: VoiceRepository,
) : SessionTeardown {
    override val order: Int = 10
    override suspend fun onLogout(sessionId: Long?) = voiceRepository.leaveChannel()
}

/**
 * Order 20: closes the realtime connection through the connection manager's
 * actor, which keeps it closed for the ending session.
 */
class RealtimeSessionTeardown @Inject constructor(
    private val realtimeConnectionManager: RealtimeConnectionManager,
) : SessionTeardown {
    override val order: Int = 20
    override suspend fun onLogout(sessionId: Long?) = realtimeConnectionManager.closeForLogout(sessionId)
}

/**
 * Order 30: stops resync work and clears every per-session cache and
 * active-channel state, so no data of the old session stays on screen.
 */
class CacheSessionTeardown @Inject constructor(
    private val resyncCoordinator: ResyncCoordinator,
    private val chatRepository: ChatRepository,
    private val guildRepository: GuildRepository,
    private val dmRepository: DmRepository,
) : SessionTeardown {
    override val order: Int = 30
    override suspend fun onLogout(sessionId: Long?) {
        // First: a resync still running must not refill the caches.
        resyncCoordinator.cancelForLogout()
        chatRepository.clearSession()
        guildRepository.clearSession()
        dmRepository.clearSession()
    }
}
