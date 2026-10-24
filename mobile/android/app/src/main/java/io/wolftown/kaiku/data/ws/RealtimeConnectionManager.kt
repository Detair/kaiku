package io.wolftown.kaiku.data.ws

import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.di.RealtimeCoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.util.concurrent.atomic.AtomicBoolean
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** What the realtime connection is doing, for UI and other observers. */
enum class RealtimeStatus { Idle, Connecting, Connected, Reconnecting, Offline }

/**
 * Owns the realtime connection: connects when logged in, refreshes the token
 * before every attempt, reconnects with backoff, reacts to network changes and
 * app foregrounding, and runs the ping/pong watchdog. [RealtimeSocket] is a
 * plain transport underneath.
 *
 * All inputs (auth state, socket state, server events, connectivity, timers,
 * foreground) go through one channel and are handled one at a time on a single
 * actor coroutine, so two inputs arriving together can never start two
 * connects. There is at most one pending retry, and every timer carries a
 * generation number so a timer that fired just before it was cancelled is
 * ignored.
 */
@Singleton
class RealtimeConnectionManager internal constructor(
    private val socket: RealtimeSocket,
    private val refresher: TokenRefresher,
    private val tokenStorage: TokenStorage,
    private val authState: AuthState,
    private val connectivity: ConnectivitySignal,
    private val scope: CoroutineScope,
    private val clock: WallClock,
    private val backoff: ReconnectBackoff,
    private val expiryClock: WallClock,
) {

    @Inject
    constructor(
        socket: RealtimeSocket,
        refresher: TokenRefresher,
        tokenStorage: TokenStorage,
        authState: AuthState,
        connectivity: ConnectivitySignal,
        @RealtimeCoroutineScope scope: CoroutineScope,
        clock: WallClock,
    ) : this(
        socket, refresher, tokenStorage, authState, connectivity, scope, clock,
        ReconnectBackoff(),
        // Token expiry is stored as wall-clock millis, so compare it against wall-clock time only.
        WallClock(System::currentTimeMillis),
    )

    companion object {
        private val logger = Logger.getLogger("RealtimeConnectionManager")

        const val PING_INTERVAL_MS = 30_000L
        const val PONG_TIMEOUT_MS = 75_000L
        const val CONNECT_TIMEOUT_MS = 20_000L
        const val REFRESH_LEAD_MS = 60_000L
        const val MAX_CONSECUTIVE_AUTH_REJECTS = 3

        /**
         * After the app returns to the foreground while Connected, a Ping is sent
         * at once and the socket is dropped if no Pong arrives within this time,
         * so a socket that died during doze is found in seconds instead of after
         * up to [PONG_TIMEOUT_MS].
         */
        const val FOREGROUND_PONG_TIMEOUT_MS = 10_000L

        /** Retry delay for the proactive refresh after a network error. */
        internal const val REFRESH_RETRY_MS = 30_000L

        /** Lower bound between proactive refreshes, so a short-lived token cannot cause a hot loop. */
        internal const val MIN_REFRESH_DELAY_MS = 10_000L
    }

    private sealed interface Input {
        data class Session(val session: AuthSession) : Input
        data class Socket(val state: ConnectionState) : Input
        data class Server(val event: ServerEvent) : Input
        data class Online(val online: Boolean) : Input
        data class NetworkChanged(val id: Long?) : Input
        data object Foreground : Input
        data class RetryDue(val generation: Int) : Input
        data class PingDue(val generation: Int) : Input
        data class WatchdogDue(val generation: Int) : Input
        data class ForegroundProbeDue(val generation: Int, val pongsAtSend: Long) : Input
        data class ConnectTimeoutDue(val generation: Int) : Input
        data class RefreshDue(val generation: Int) : Input
    }

    private val _status = MutableStateFlow(RealtimeStatus.Idle)
    val status: StateFlow<RealtimeStatus> = _status.asStateFlow()

    private val inbox = Channel<Input>(Channel.UNLIMITED)
    private val started = AtomicBoolean(false)

    // -- Actor-owned state (only touched on the actor coroutine) -------------

    /** True between our socket.connect() and the socket's terminal state or our socket.disconnect(). */
    private var socketActive = false
    private var consecutiveAuthRejects = 0
    private var lastPongAt = 0L

    /** Pongs received on the current socket; the foreground probe checks it moved. */
    private var pongCount = 0L
    private var lastNetworkId: Long? = null

    private var retryJob: Job? = null
    private var retryGeneration = 0
    private var heartbeatJobs: List<Job> = emptyList()
    private var heartbeatGeneration = 0
    private var connectTimeoutJob: Job? = null
    private var connectTimeoutGeneration = 0
    private var refreshJob: Job? = null
    private var refreshGeneration = 0

    /** Starts observing; idempotent. */
    fun start() {
        if (!started.compareAndSet(false, true)) return
        scope.launch {
            for (input in inbox) {
                try {
                    handle(input)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    logger.warning("input ${input::class.simpleName} failed: ${e.javaClass.simpleName}")
                    if (isLoggedIn() && connectivity.isConnected.value && !socketActive) {
                        enterReconnecting()
                    }
                }
            }
        }
        scope.launch { authState.session.collect { inbox.send(Input.Session(it)) } }
        scope.launch { socket.connectionState.collect { inbox.send(Input.Socket(it)) } }
        scope.launch { socket.events.collect { inbox.send(Input.Server(it)) } }
        scope.launch { connectivity.isConnected.collect { inbox.send(Input.Online(it)) } }
        scope.launch { connectivity.networkId.collect { inbox.send(Input.NetworkChanged(it)) } }
    }

    /** Call when the app comes to the foreground: reconnects now if not already (re)connected. */
    fun onAppForeground() {
        inbox.trySend(Input.Foreground)
    }

    // -- Actor ----------------------------------------------------------------

    private suspend fun handle(input: Input) {
        when (input) {
            is Input.Session -> onSession(input.session)
            is Input.Socket -> onSocketState(input.state)
            is Input.Server -> onServerEvent(input.event)
            is Input.Online -> onOnline(input.online)
            is Input.NetworkChanged -> onNetworkChanged(input.id)
            Input.Foreground -> onForeground()
            is Input.RetryDue -> if (input.generation == retryGeneration) {
                retryJob = null
                attemptConnect()
            }
            is Input.PingDue -> if (input.generation == heartbeatGeneration) onPingDue()
            is Input.WatchdogDue -> if (input.generation == heartbeatGeneration) onWatchdogDue()
            is Input.ForegroundProbeDue -> if (input.generation == heartbeatGeneration) {
                onForegroundProbeDue(input.pongsAtSend)
            }
            is Input.ConnectTimeoutDue -> if (input.generation == connectTimeoutGeneration) onConnectTimeout()
            is Input.RefreshDue -> if (input.generation == refreshGeneration) onRefreshDue()
        }
    }

    private suspend fun onSession(session: AuthSession) {
        when (session) {
            is AuthSession.LoggedIn -> {
                logger.info("session LoggedIn")
                backoff.reset()
                consecutiveAuthRejects = 0
                scheduleTokenRefresh(minDelayMs = 0)
                attemptConnect()
            }
            AuthSession.LoggedOut -> {
                logger.info("session LoggedOut")
                cancelRetry()
                stopHeartbeat()
                cancelTokenRefresh()
                socketActive = false
                socket.disconnect()
                setStatus(RealtimeStatus.Idle)
            }
        }
    }

    /** Step 1: refresh if needed, then open the socket. */
    private suspend fun attemptConnect() {
        cancelRetry()
        if (!isLoggedIn()) return
        if (!connectivity.isConnected.value) {
            setStatus(RealtimeStatus.Offline)
            return
        }
        if (socketActive) {
            val state = socket.connectionState.value
            if (state == ConnectionState.Connecting || state == ConnectionState.Connected) return
        }
        val url = tokenStorage.getServerUrl() ?: run {
            logger.warning("no server URL, not connecting")
            setStatus(RealtimeStatus.Idle)
            return
        }
        if (_status.value != RealtimeStatus.Reconnecting) setStatus(RealtimeStatus.Connecting)

        when (refresher.ensureFresh(REFRESH_LEAD_MS)) {
            RefreshResult.SUCCESS -> Unit
            RefreshResult.NETWORK_ERROR -> {
                // The session is kept. A still-valid token can open the socket; otherwise wait.
                if (!hasUnexpiredAccessToken()) {
                    logger.info("refresh NETWORK_ERROR and token expired, backing off")
                    enterReconnecting()
                    return
                }
                logger.info("refresh NETWORK_ERROR, connecting with the current token")
            }
            RefreshResult.AUTH_REJECTED, RefreshResult.NO_SESSION -> {
                // The refresher has already logged the user out.
                logger.info("refresh ended the session, not connecting")
                stopAfterSessionEnded()
                return
            }
        }

        // State may have changed while the refresh was suspended.
        if (!isLoggedIn()) return
        if (!connectivity.isConnected.value) {
            setStatus(RealtimeStatus.Offline)
            return
        }
        val token = tokenStorage.getAccessToken() ?: run {
            logger.warning("no access token after refresh, not connecting")
            stopAfterSessionEnded()
            return
        }
        scheduleTokenRefresh(minDelayMs = MIN_REFRESH_DELAY_MS)
        socket.connect(url, token)
        // Only after connect() returned: if it threw, the actor's catch schedules a retry.
        socketActive = true
        armConnectTimeout()
    }

    private suspend fun onSocketState(state: ConnectionState) {
        // Ignore stale states: the socket has moved on since this one was emitted.
        if (state != socket.connectionState.value) return
        if (!socketActive) return
        when (state) {
            ConnectionState.Connecting, ConnectionState.Connected -> Unit // Connected is driven by Ready.
            ConnectionState.Disconnected -> {
                logger.info("socket Disconnected")
                socketActive = false
                stopHeartbeat()
                if (!isLoggedIn()) return
                if (!connectivity.isConnected.value) {
                    setStatus(RealtimeStatus.Offline)
                    return
                }
                enterReconnecting()
            }
            ConnectionState.AuthRejected -> {
                logger.info("socket AuthRejected")
                socketActive = false
                stopHeartbeat()
                if (!isLoggedIn()) return
                onAuthRejected()
            }
        }
    }

    /** Step 3. */
    private suspend fun onAuthRejected() {
        consecutiveAuthRejects++
        if (consecutiveAuthRejects >= MAX_CONSECUTIVE_AUTH_REJECTS) {
            logger.warning("socket rejected $consecutiveAuthRejects times in a row, logging out")
            stopAfterSessionEnded()
            // Same as the refresher's AUTH_REJECTED path: without clearing, every cold
            // start would restore the session and loop home -> login again.
            tokenStorage.clear()
            authState.setLoggedOut()
            return
        }
        setStatus(RealtimeStatus.Reconnecting)
        when (refresher.forceRefresh()) {
            RefreshResult.SUCCESS -> attemptConnect()
            RefreshResult.NETWORK_ERROR -> enterReconnecting()
            RefreshResult.AUTH_REJECTED, RefreshResult.NO_SESSION -> {
                logger.info("forced refresh ended the session, not reconnecting")
                stopAfterSessionEnded()
            }
        }
    }

    private fun onServerEvent(event: ServerEvent) {
        when (event) {
            is ServerEvent.Ready -> {
                if (!socketActive || socket.connectionState.value != ConnectionState.Connected) return
                if (!isLoggedIn()) return
                logger.info("ready, status=Connected")
                cancelRetry()
                cancelConnectTimeout()
                backoff.reset()
                consecutiveAuthRejects = 0
                lastPongAt = clock.nowMillis()
                setStatus(RealtimeStatus.Connected)
                startHeartbeat()
            }
            ServerEvent.Pong -> {
                lastPongAt = clock.nowMillis()
                pongCount++
            }
            else -> Unit
        }
    }

    /** Step 6: offline pauses, online reconnects now. */
    private suspend fun onOnline(online: Boolean) {
        if (!online) {
            logger.info("network lost, status=Offline")
            cancelRetry()
            if (!isLoggedIn()) return
            stopHeartbeat()
            if (socketActive) {
                socketActive = false
                socket.disconnect()
            }
            setStatus(RealtimeStatus.Offline)
        } else {
            if (!isLoggedIn()) return
            logger.info("network available")
            backoff.reset()
            attemptConnect()
        }
    }

    /** Step 6: a different default network while Connected means the old socket is dead. */
    private suspend fun onNetworkChanged(id: Long?) {
        val previous = lastNetworkId
        if (id != null) lastNetworkId = id
        if (id == null || previous == null || id == previous) return
        if (_status.value != RealtimeStatus.Connected) return
        logger.info("default network changed while Connected, reconnecting")
        stopHeartbeat()
        socketActive = false
        socket.disconnect()
        backoff.reset()
        setStatus(RealtimeStatus.Reconnecting)
        attemptConnect()
    }

    /** Step 7. */
    private suspend fun onForeground() {
        if (!isLoggedIn()) return
        val socketState = socket.connectionState.value
        val terminal = socketState == ConnectionState.Disconnected || socketState == ConnectionState.AuthRejected
        if (socketActive && terminal) {
            // The socket ended without us observing it (StateFlow conflation): recover now.
            logger.info("app foreground found socket $socketState unobserved")
            socketActive = false
            stopHeartbeat()
        } else {
            val current = _status.value
            if (current == RealtimeStatus.Connected) {
                probeAfterForeground()
                return
            }
            if (current == RealtimeStatus.Connecting) return
        }
        logger.info("app foreground, connecting now")
        backoff.reset()
        attemptConnect()
    }

    // -- Heartbeat (step 5) ---------------------------------------------------

    private fun startHeartbeat() {
        stopHeartbeat()
        val generation = heartbeatGeneration
        val ping = scope.launch {
            while (true) {
                delay(PING_INTERVAL_MS)
                inbox.send(Input.PingDue(generation))
            }
        }
        heartbeatJobs = listOf(ping, launchWatchdog(PONG_TIMEOUT_MS + 1, generation))
    }

    private fun launchWatchdog(delayMs: Long, generation: Int): Job = scope.launch {
        delay(delayMs)
        inbox.send(Input.WatchdogDue(generation))
    }

    /** Stops every per-socket timer: ping, watchdog and the connect timeout. */
    private fun stopHeartbeat() {
        heartbeatGeneration++
        heartbeatJobs.forEach { it.cancel() }
        heartbeatJobs = emptyList()
        cancelConnectTimeout()
    }

    // -- Connect timeout --------------------------------------------------------

    /** Armed when a socket is opened; Ready or any terminal state clears it. */
    private fun armConnectTimeout() {
        cancelConnectTimeout()
        val generation = connectTimeoutGeneration
        connectTimeoutJob = scope.launch {
            delay(CONNECT_TIMEOUT_MS)
            inbox.send(Input.ConnectTimeoutDue(generation))
        }
    }

    private fun cancelConnectTimeout() {
        connectTimeoutGeneration++
        connectTimeoutJob?.cancel()
        connectTimeoutJob = null
    }

    /**
     * No Ready within [CONNECT_TIMEOUT_MS]: either a half-open socket (reads never
     * time out) or a close we never observed because the state flow conflated it.
     */
    private fun onConnectTimeout() {
        connectTimeoutJob = null
        if (!socketActive) return
        logger.warning("no ready within connect timeout, reconnecting")
        stopHeartbeat()
        socketActive = false
        socket.disconnect()
        if (!isLoggedIn()) return
        if (!connectivity.isConnected.value) {
            setStatus(RealtimeStatus.Offline)
            return
        }
        enterReconnecting()
    }

    /** Ping now and arm a short watchdog: the socket may have died while the app was in the background. */
    private fun probeAfterForeground() {
        logger.info("app foreground while Connected, probing socket")
        val pongsAtSend = pongCount
        socket.send(ClientEvent.Ping)
        val generation = heartbeatGeneration
        heartbeatJobs = heartbeatJobs.filter { it.isActive } + scope.launch {
            delay(FOREGROUND_PONG_TIMEOUT_MS)
            inbox.send(Input.ForegroundProbeDue(generation, pongsAtSend))
        }
    }

    private fun onForegroundProbeDue(pongsAtSend: Long) {
        if (_status.value != RealtimeStatus.Connected) return
        if (pongCount != pongsAtSend) return
        logger.warning("no pong after foreground probe, reconnecting")
        stopHeartbeat()
        socketActive = false
        socket.disconnect()
        enterReconnecting()
    }

    private fun onPingDue() {
        if (_status.value == RealtimeStatus.Connected) socket.send(ClientEvent.Ping)
    }

    private fun onWatchdogDue() {
        if (_status.value != RealtimeStatus.Connected) return
        val silentFor = clock.nowMillis() - lastPongAt
        if (silentFor > PONG_TIMEOUT_MS) {
            logger.warning("no pong within timeout, reconnecting")
            stopHeartbeat()
            socketActive = false
            socket.disconnect()
            enterReconnecting()
        } else {
            val generation = heartbeatGeneration
            heartbeatJobs = heartbeatJobs.filter { it.isActive } +
                launchWatchdog(PONG_TIMEOUT_MS - silentFor + 1, generation)
        }
    }

    // -- Retry (step 2) -------------------------------------------------------

    /** Status Reconnecting, then retry after the next backoff delay. */
    private fun enterReconnecting() {
        setStatus(RealtimeStatus.Reconnecting)
        val delayMs = backoff.next()
        logger.info("reconnect in ${delayMs}ms")
        cancelRetry()
        val generation = retryGeneration
        retryJob = scope.launch {
            delay(delayMs)
            inbox.send(Input.RetryDue(generation))
        }
    }

    private fun cancelRetry() {
        retryGeneration++
        retryJob?.cancel()
        retryJob = null
    }

    // -- Proactive token refresh (step 8) -------------------------------------

    private fun scheduleTokenRefresh(minDelayMs: Long) {
        cancelTokenRefresh()
        val dueIn = tokenStorage.getAccessTokenExpiresAtMillis() - REFRESH_LEAD_MS - expiryClock.nowMillis()
        val delayMs = dueIn.coerceAtLeast(minDelayMs)
        launchRefresh(delayMs)
    }

    private fun launchRefresh(delayMs: Long) {
        val generation = refreshGeneration
        refreshJob = scope.launch {
            delay(delayMs)
            inbox.send(Input.RefreshDue(generation))
        }
    }

    private fun cancelTokenRefresh() {
        refreshGeneration++
        refreshJob?.cancel()
        refreshJob = null
    }

    private suspend fun onRefreshDue() {
        refreshJob = null
        if (!isLoggedIn()) return
        when (refresher.ensureFresh(REFRESH_LEAD_MS)) {
            RefreshResult.SUCCESS -> scheduleTokenRefresh(minDelayMs = MIN_REFRESH_DELAY_MS)
            RefreshResult.NETWORK_ERROR -> {
                cancelTokenRefresh()
                launchRefresh(REFRESH_RETRY_MS)
            }
            RefreshResult.AUTH_REJECTED, RefreshResult.NO_SESSION -> stopAfterSessionEnded()
        }
    }

    // -- Helpers --------------------------------------------------------------

    /** The session ended (refresher or reject cap): stop everything without scheduling a retry. */
    private fun stopAfterSessionEnded() {
        cancelRetry()
        stopHeartbeat()
        cancelTokenRefresh()
        if (socketActive) {
            socketActive = false
            socket.disconnect()
        }
        setStatus(RealtimeStatus.Idle)
    }

    private fun isLoggedIn(): Boolean = authState.session.value is AuthSession.LoggedIn

    private fun hasUnexpiredAccessToken(): Boolean =
        tokenStorage.getAccessToken() != null &&
            tokenStorage.getAccessTokenExpiresAtMillis() > expiryClock.nowMillis()

    private fun setStatus(next: RealtimeStatus) {
        if (_status.value != next) {
            logger.info("status ${_status.value} -> $next")
            _status.value = next
        }
    }
}
