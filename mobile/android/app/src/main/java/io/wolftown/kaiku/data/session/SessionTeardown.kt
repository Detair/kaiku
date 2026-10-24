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
    suspend fun onLogout()
}

/**
 * Runs every [SessionTeardown] once per ended session.
 *
 * Two triggers share this runner:
 * - an explicit logout calls [tearDown] before the server logout and the
 *   token clear ([io.wolftown.kaiku.data.repository.AuthRepository.logout]);
 * - [start] observes [AuthState]: when a LoggedIn session becomes LoggedOut
 *   for any other reason (refresh rejected, WebSocket auth reject cap,
 *   PASSWORD_CHANGE_REQUIRED), the same teardown runs. This is the only hook
 *   for rejected sessions; the HTTP client and the connection manager keep
 *   just clearing tokens and setting LoggedOut.
 *
 * Sessions are identified by the [AuthSession.LoggedIn] instance (identity,
 * not equality: every login stores a new instance). The first trigger for a
 * session claims it under [mutex]; a later trigger for the same session waits
 * for the running teardown and then does nothing, so an explicit logout
 * (which also ends in LoggedOut) never tears down twice.
 *
 * Each step gets [STEP_TIMEOUT_MS]; a failure or timeout is logged (step name
 * and exception type only) and the next step runs. If a newer login took over
 * meanwhile, the remaining steps are skipped so the new session's state is
 * left alone.
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
    private val mutex = Mutex()
    private val started = AtomicBoolean(false)

    /** The session whose teardown was claimed last; guarded by [mutex]. */
    private var claimed: AuthSession.LoggedIn? = null

    /** Starts observing LoggedIn -> LoggedOut transitions; idempotent. */
    fun start() {
        if (!started.compareAndSet(false, true)) return
        scope.launch {
            var previous: AuthSession = authState.session.value
            authState.session.collect { current ->
                val ended = previous as? AuthSession.LoggedIn
                previous = current
                if (ended != null && current is AuthSession.LoggedOut) tearDown(ended)
            }
        }
    }

    /**
     * Runs the teardown steps for [session] unless they already ran (or are
     * running) for it. With a null [session] (nobody was logged in) the steps
     * always run; they are idempotent.
     */
    suspend fun tearDown(session: AuthSession.LoggedIn?) {
        mutex.withLock {
            if (session != null) {
                if (claimed === session) return
                claimed = session
            }
            for (step in steps) {
                if (isSuperseded(session)) {
                    logger.info("a newer session started, remaining teardown skipped")
                    return
                }
                runStep(step)
            }
        }
    }

    private fun isSuperseded(session: AuthSession.LoggedIn?): Boolean {
        val now = authState.session.value
        return now is AuthSession.LoggedIn && now !== session
    }

    private suspend fun runStep(step: SessionTeardown) {
        val name = step::class.simpleName
        try {
            val finished = withTimeoutOrNull(STEP_TIMEOUT_MS) {
                step.onLogout()
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
    override suspend fun onLogout() = voiceRepository.leaveChannel()
}

/**
 * Order 20: closes the realtime connection through the connection manager's
 * actor, which keeps it closed for the ending session.
 */
class RealtimeSessionTeardown @Inject constructor(
    private val realtimeConnectionManager: RealtimeConnectionManager,
) : SessionTeardown {
    override val order: Int = 20
    override suspend fun onLogout() = realtimeConnectionManager.closeForLogout()
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
    override suspend fun onLogout() {
        // First: a resync still running must not refill the caches.
        resyncCoordinator.cancelForLogout()
        chatRepository.clearSession()
        guildRepository.clearSession()
        dmRepository.clearSession()
    }
}
