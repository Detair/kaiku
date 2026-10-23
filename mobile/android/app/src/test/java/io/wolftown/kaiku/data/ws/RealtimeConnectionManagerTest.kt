package io.wolftown.kaiku.data.ws

import app.cash.turbine.test
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class RealtimeConnectionManagerTest {

    // -- Fakes ----------------------------------------------------------------

    private class FakeSocket(private val log: MutableList<String>) : RealtimeSocket {
        val state = MutableStateFlow(ConnectionState.Disconnected)
        override val connectionState: StateFlow<ConnectionState> = state

        private val _events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
        override val events: SharedFlow<ServerEvent> = _events

        /** Access tokens of every connect that opened a socket. */
        val connects = mutableListOf<String>()
        var disconnects = 0
        val sent = mutableListOf<ClientEvent>()

        /**
         * When set, the next connect fails synchronously into this state. Collectors
         * only see the conflated value, so e.g. Disconnected -> Connecting -> Disconnected
         * is never observed (like OkHttp failing before the collector resumes).
         */
        var failNextConnectWith: ConnectionState? = null

        override fun connect(serverUrl: String, accessToken: String) {
            if (state.value == ConnectionState.Connecting || state.value == ConnectionState.Connected) return
            connects += accessToken
            log += "connect:$accessToken"
            state.value = ConnectionState.Connecting
            failNextConnectWith?.let {
                failNextConnectWith = null
                state.value = it
            }
        }

        override fun disconnect() {
            disconnects++
            log += "disconnect"
            state.value = ConnectionState.Disconnected
        }

        override fun send(event: ClientEvent): Boolean {
            sent += event
            return state.value == ConnectionState.Connected
        }

        fun ready() {
            state.value = ConnectionState.Connected
            _events.tryEmit(ServerEvent.Ready("user-1"))
        }

        fun pong() {
            _events.tryEmit(ServerEvent.Pong)
        }

        /** Server or network closed the socket. */
        fun drop() {
            state.value = ConnectionState.Disconnected
        }

        /** Server closed with 1008. */
        fun rejectAuth() {
            state.value = ConnectionState.AuthRejected
        }
    }

    /** Token store backed by plain fields, with expiry on the test's virtual clock. */
    private class Tokens(private val now: () -> Long) {
        var accessToken: String? = "token-0"
        var expiresAt: Long = 15 * 60_000L
        private var rotations = 0

        val storage: TokenStorage = mockk(relaxed = true) {
            every { getAccessToken() } answers { accessToken }
            every { getAccessTokenExpiresAtMillis() } answers { expiresAt }
            every { getServerUrl() } returns "https://kaiku.example.com"
        }

        fun rotate() {
            rotations++
            accessToken = "token-$rotations"
            expiresAt = now() + 15 * 60_000L
        }
    }

    /** Behaves like KaikuHttpClient: AUTH_REJECTED / NO_SESSION end the session. */
    private class FakeRefresher(
        private val tokens: Tokens,
        private val authState: AuthState,
        private val now: () -> Long,
        private val log: MutableList<String>,
    ) : TokenRefresher {
        var ensureResult = RefreshResult.SUCCESS
        var forceResult = RefreshResult.SUCCESS
        var ensureCalls = 0
        var forceCalls = 0

        override suspend fun ensureFresh(minValidityMs: Long): RefreshResult {
            ensureCalls++
            log += "ensureFresh"
            val result = ensureResult
            if (result == RefreshResult.SUCCESS && tokens.expiresAt - now() < minValidityMs) tokens.rotate()
            endSessionIfRejected(result)
            return result
        }

        override suspend fun forceRefresh(): RefreshResult {
            forceCalls++
            log += "forceRefresh"
            val result = forceResult
            if (result == RefreshResult.SUCCESS) tokens.rotate()
            endSessionIfRejected(result)
            return result
        }

        private fun endSessionIfRejected(result: RefreshResult) {
            if (result == RefreshResult.AUTH_REJECTED || result == RefreshResult.NO_SESSION) {
                tokens.accessToken = null
                authState.setLoggedOut()
            }
        }
    }

    private class FakeConnectivity : ConnectivitySignal {
        override val isConnected = MutableStateFlow(true)
        override val networkId = MutableStateFlow<Long?>(1L)
    }

    private class Harness(scope: TestScope) {
        val now: () -> Long = { scope.testScheduler.currentTime }
        val log = mutableListOf<String>()
        val socket = FakeSocket(log)
        val tokens = Tokens(now)
        val authState = AuthState(scope.backgroundScope)
        val refresher = FakeRefresher(tokens, authState, now, log)
        val connectivity = FakeConnectivity()
        val manager = RealtimeConnectionManager(
            socket = socket,
            refresher = refresher,
            tokenStorage = tokens.storage,
            authState = authState,
            connectivity = connectivity,
            scope = scope.backgroundScope,
            clock = WallClock { scope.testScheduler.currentTime },
            backoff = ReconnectBackoff(jitter = 0.0),
            expiryClock = WallClock { scope.testScheduler.currentTime },
        )
    }

    private fun runManagerTest(body: suspend TestScope.(Harness) -> Unit) =
        runTest(StandardTestDispatcher()) { body(Harness(this)) }

    /** Logs in, starts the manager and completes the handshake. */
    private fun TestScope.connected(h: Harness) {
        h.authState.setLoggedIn("user-1")
        h.manager.start()
        runCurrent()
        h.socket.ready()
        runCurrent()
        assertEquals(RealtimeStatus.Connected, h.manager.status.value)
    }

    private val fiveMinutes = 5 * 60_000L

    // -- Tests ----------------------------------------------------------------

    @Test
    fun `login with expired token refreshes before connecting`() = runManagerTest { h ->
        h.tokens.expiresAt = 0L // expired locally
        h.authState.setLoggedIn("user-1")
        h.manager.start()
        runCurrent()

        assertEquals(listOf("ensureFresh", "connect:token-1"), h.log.take(2))
        assertEquals(listOf("token-1"), h.socket.connects)
    }

    @Test
    fun `disconnect schedules reconnect with backoff`() = runManagerTest { h ->
        connected(h)

        h.socket.drop()
        runCurrent()
        assertEquals(RealtimeStatus.Reconnecting, h.manager.status.value)

        advanceTimeBy(999)
        assertEquals(1, h.socket.connects.size)

        advanceTimeBy(2)
        assertEquals(2, h.socket.connects.size)
        // Every attempt starts with ensureFresh.
        assertEquals("ensureFresh", h.log[h.log.lastIndexOf("connect:token-0") - 1])
    }

    @Test
    fun `auth rejected close forces refresh even when token not locally expired`() = runManagerTest { h ->
        h.authState.setLoggedIn("user-1")
        h.manager.start()
        runCurrent()
        assertEquals(listOf("token-0"), h.socket.connects)

        h.socket.rejectAuth()
        runCurrent()

        assertEquals(1, h.refresher.forceCalls)
        assertEquals(listOf("token-0", "token-1"), h.socket.connects)
    }

    @Test
    fun `auth rejected refresh logs out and stops reconnecting`() = runManagerTest { h ->
        h.refresher.forceResult = RefreshResult.AUTH_REJECTED
        h.authState.setLoggedIn("user-1")
        h.manager.start()
        runCurrent()

        h.socket.rejectAuth()
        runCurrent()
        advanceTimeBy(fiveMinutes)

        assertEquals(1, h.refresher.forceCalls)
        assertEquals(1, h.socket.connects.size)
        assertEquals(AuthSession.LoggedOut, h.authState.session.value)
        assertEquals(RealtimeStatus.Idle, h.manager.status.value)
    }

    @Test
    fun `repeated auth rejects after successful refresh log out`() = runManagerTest { h ->
        h.authState.setLoggedIn("user-1")
        h.manager.start()
        runCurrent()

        repeat(RealtimeConnectionManager.MAX_CONSECUTIVE_AUTH_REJECTS) {
            h.socket.rejectAuth()
            runCurrent()
        }

        assertEquals(AuthSession.LoggedOut, h.authState.session.value)
        verify(exactly = 1) { h.tokens.storage.clear() }
        assertEquals(RealtimeConnectionManager.MAX_CONSECUTIVE_AUTH_REJECTS - 1, h.refresher.forceCalls)
        val connectsAtLogout = h.socket.connects.size
        assertEquals(RealtimeConnectionManager.MAX_CONSECUTIVE_AUTH_REJECTS, connectsAtLogout)

        advanceTimeBy(fiveMinutes)
        assertEquals(connectsAtLogout, h.socket.connects.size)
        assertEquals(RealtimeStatus.Idle, h.manager.status.value)
    }

    @Test
    fun `missing pong triggers reconnect`() = runManagerTest { h ->
        connected(h)
        val disconnectsBefore = h.socket.disconnects

        advanceTimeBy(RealtimeConnectionManager.PONG_TIMEOUT_MS)
        assertEquals(disconnectsBefore, h.socket.disconnects)
        assertTrue(h.socket.sent.contains(ClientEvent.Ping))

        advanceTimeBy(2) // now > lastPongAt + PONG_TIMEOUT_MS
        assertEquals(disconnectsBefore + 1, h.socket.disconnects)
        assertEquals(RealtimeStatus.Reconnecting, h.manager.status.value)

        advanceTimeBy(1_001) // first backoff step
        assertEquals(2, h.socket.connects.size)
    }

    @Test
    fun `pong keeps connection alive`() = runManagerTest { h ->
        connected(h)
        val disconnectsBefore = h.socket.disconnects

        repeat((fiveMinutes / RealtimeConnectionManager.PING_INTERVAL_MS).toInt()) {
            advanceTimeBy(RealtimeConnectionManager.PING_INTERVAL_MS)
            h.socket.pong()
            runCurrent()
        }

        assertEquals(disconnectsBefore, h.socket.disconnects)
        assertEquals(1, h.socket.connects.size)
        assertEquals(RealtimeStatus.Connected, h.manager.status.value)
        assertTrue(h.socket.sent.count { it == ClientEvent.Ping } >= 9)
    }

    @Test
    fun `network loss pauses and regain reconnects immediately`() = runManagerTest { h ->
        connected(h)

        h.connectivity.isConnected.value = false
        runCurrent()
        assertEquals(RealtimeStatus.Offline, h.manager.status.value)

        advanceTimeBy(fiveMinutes)
        assertEquals(1, h.socket.connects.size)

        h.connectivity.isConnected.value = true
        runCurrent()
        assertEquals(2, h.socket.connects.size)
    }

    @Test
    fun `network change while connected forces reconnect`() = runManagerTest { h ->
        connected(h)
        val disconnectsBefore = h.socket.disconnects

        h.connectivity.networkId.value = 2L
        runCurrent()

        assertEquals(disconnectsBefore + 1, h.socket.disconnects)
        assertEquals(2, h.socket.connects.size)
    }

    @Test
    fun `app foreground reconnects without waiting for backoff`() = runManagerTest { h ->
        connected(h)
        h.socket.drop()
        runCurrent()
        assertEquals(RealtimeStatus.Reconnecting, h.manager.status.value)

        h.manager.onAppForeground()
        runCurrent()
        assertEquals(2, h.socket.connects.size)
        h.socket.ready()
        runCurrent()

        // The pending backoff retry was cancelled: no extra connect later.
        h.socket.pong()
        advanceTimeBy(RealtimeConnectionManager.PING_INTERVAL_MS)
        assertEquals(2, h.socket.connects.size)
        assertEquals(RealtimeStatus.Connected, h.manager.status.value)
    }

    @Test
    fun `connect without Ready times out and reconnects`() = runManagerTest { h ->
        h.authState.setLoggedIn("user-1")
        h.manager.start()
        runCurrent()
        assertEquals(1, h.socket.connects.size)
        val disconnectsBefore = h.socket.disconnects

        advanceTimeBy(RealtimeConnectionManager.CONNECT_TIMEOUT_MS)
        assertEquals(disconnectsBefore, h.socket.disconnects)

        advanceTimeBy(2) // past CONNECT_TIMEOUT_MS
        assertEquals(disconnectsBefore + 1, h.socket.disconnects)
        assertEquals(RealtimeStatus.Reconnecting, h.manager.status.value)

        advanceTimeBy(1_001) // first backoff step
        assertEquals(2, h.socket.connects.size)

        // Ready on the new socket clears its timeout.
        h.socket.ready()
        runCurrent()
        h.socket.pong()
        advanceTimeBy(RealtimeConnectionManager.CONNECT_TIMEOUT_MS + 1)
        assertEquals(disconnectsBefore + 1, h.socket.disconnects)
        assertEquals(RealtimeStatus.Connected, h.manager.status.value)
    }

    @Test
    fun `foreground recovers when socket already disconnected unobserved`() = runManagerTest { h ->
        // The first connect fails before the collector sees Connecting, so the
        // manager never observes the Disconnected and still reports Connecting.
        h.manager.start()
        runCurrent() // drain the initial inputs so no queued socket state can reveal the failure
        h.socket.failNextConnectWith = ConnectionState.Disconnected
        h.authState.setLoggedIn("user-1")
        runCurrent()
        assertEquals(1, h.socket.connects.size)
        assertEquals(ConnectionState.Disconnected, h.socket.state.value)
        assertEquals(RealtimeStatus.Connecting, h.manager.status.value)

        h.manager.onAppForeground()
        runCurrent()

        assertEquals(2, h.socket.connects.size)
        assertEquals(ConnectionState.Connecting, h.socket.state.value)
    }

    @Test
    fun `unobserved disconnect is recovered by connect timeout`() = runManagerTest { h ->
        h.manager.start()
        runCurrent() // drain the initial inputs so no queued socket state can reveal the failure
        h.socket.failNextConnectWith = ConnectionState.Disconnected
        h.authState.setLoggedIn("user-1")
        runCurrent()
        assertEquals(1, h.socket.connects.size)

        advanceTimeBy(RealtimeConnectionManager.CONNECT_TIMEOUT_MS + 1 + 1_001)
        assertEquals(2, h.socket.connects.size)
    }

    @Test
    fun `logout disconnects socket`() = runManagerTest { h ->
        connected(h)
        val disconnectsBefore = h.socket.disconnects

        h.authState.setLoggedOut()
        runCurrent()

        assertEquals(disconnectsBefore + 1, h.socket.disconnects)
        assertEquals(RealtimeStatus.Idle, h.manager.status.value)

        advanceTimeBy(fiveMinutes)
        assertEquals(1, h.socket.connects.size)
    }

    @Test
    fun `status reflects reconnecting then connected`() = runManagerTest { h ->
        h.manager.start()
        runCurrent()

        h.manager.status.test {
            assertEquals(RealtimeStatus.Idle, awaitItem())

            h.authState.setLoggedIn("user-1")
            runCurrent()
            assertEquals(RealtimeStatus.Connecting, awaitItem())

            h.socket.ready()
            runCurrent()
            assertEquals(RealtimeStatus.Connected, awaitItem())

            h.socket.drop()
            runCurrent()
            assertEquals(RealtimeStatus.Reconnecting, awaitItem())

            advanceTimeBy(1_001)
            assertEquals(2, h.socket.connects.size)
            h.socket.ready()
            runCurrent()
            assertEquals(RealtimeStatus.Connected, awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }
}
