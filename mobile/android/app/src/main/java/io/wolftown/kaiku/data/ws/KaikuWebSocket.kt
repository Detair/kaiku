package io.wolftown.kaiku.data.ws

import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

enum class ConnectionState {
    Connected,
    Connecting,
    Disconnected,
    TokenExpired
}

/**
 * Manages the WebSocket connection to the Kaiku server.
 *
 * Provides:
 * - [events] SharedFlow of parsed [ServerEvent]s
 * - [connectionState] for observing connection lifecycle
 * - Automatic application-level heartbeat (30s interval)
 * - Exponential backoff reconnection (1s -> 30s cap)
 * - Network-aware reconnection via [ConnectivityMonitor]
 */
@Singleton
class KaikuWebSocket @Inject constructor(
    private val okHttpClient: OkHttpClient,
    private val tokenStorage: TokenStorage,
    private val json: Json
) {
    companion object {
        private val logger = Logger.getLogger("KaikuWebSocket")
        internal const val PING_INTERVAL_MS = 30_000L
        internal const val INITIAL_RECONNECT_DELAY_MS = 1_000L
        internal const val MAX_RECONNECT_DELAY_MS = 30_000L
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var connectivityJob: Job? = null

    private val _events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
    val events: SharedFlow<ServerEvent> = _events.asSharedFlow()

    private val _connectionState = MutableStateFlow(ConnectionState.Disconnected)
    val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    private var webSocket: WebSocket? = null
    private var pingJob: Job? = null
    private var reconnectJob: Job? = null
    private var serverUrl: String? = null
    private var reconnectDelay = INITIAL_RECONNECT_DELAY_MS
    private var shouldReconnect = false

    /** Optional connectivity monitor — set via [setConnectivityMonitor]. */
    private var connectivityMonitor: ConnectivityMonitor? = null

    fun setConnectivityMonitor(monitor: ConnectivityMonitor) {
        connectivityJob?.cancel()
        connectivityMonitor = monitor
        connectivityJob = scope.launch {
            monitor.isConnected.collect { connected ->
                if (connected && shouldReconnect && _connectionState.value == ConnectionState.Disconnected) {
                    reconnectDelay = INITIAL_RECONNECT_DELAY_MS
                    scheduleReconnect()
                }
            }
        }
    }

    /**
     * Opens a WebSocket connection to the given server URL.
     *
     * The URL should be the server's base HTTP(S) URL; it will be converted
     * to the appropriate `ws://` or `wss://` scheme with `/ws` path appended.
     */
    fun connect(serverUrl: String) {
        this.serverUrl = serverUrl
        shouldReconnect = true
        reconnectDelay = INITIAL_RECONNECT_DELAY_MS
        reconnectJob?.cancel()
        doConnect()
    }

    fun disconnect() {
        shouldReconnect = false
        reconnectJob?.cancel()
        reconnectJob = null
        pingJob?.cancel()
        pingJob = null
        connectivityJob?.cancel()
        connectivityJob = null
        webSocket?.close(1000, "Client disconnect")
        webSocket = null
        _connectionState.value = ConnectionState.Disconnected
    }

    fun send(event: ClientEvent): Boolean {
        val text = json.encodeToString<ClientEvent>(event)
        val sent = webSocket?.send(text) ?: false
        if (!sent) {
            logger.warning("Failed to send event (not connected): ${event::class.simpleName}")
        }
        return sent
    }

    // -- Internal -------------------------------------------------------------

    private fun doConnect() {
        val url = serverUrl ?: run {
            logger.warning("No server URL configured, stopping reconnect")
            shouldReconnect = false
            return
        }
        val token = tokenStorage.getAccessToken() ?: run {
            logger.warning("No access token available, stopping reconnect")
            shouldReconnect = false
            _connectionState.value = ConnectionState.Disconnected
            return
        }

        if (tokenStorage.isAccessTokenExpired()) {
            logger.info("Access token expired, emitting TokenExpired state")
            _connectionState.value = ConnectionState.TokenExpired
            shouldReconnect = false
            return
        }

        _connectionState.value = ConnectionState.Connecting

        val wsUrl = url
            .replace("https://", "wss://")
            .replace("http://", "ws://")
            .trimEnd('/') + "/ws"

        val request = Request.Builder()
            .url(wsUrl)
            .build()

        webSocket = okHttpClient.newWebSocket(request, createListener(token))
    }

    private fun createListener(token: String) = object : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            // Authenticate via first frame (post-connect auth)
            send(ClientEvent.Authenticate(token))

            logger.info("WebSocket connected")
            _connectionState.value = ConnectionState.Connected
            reconnectDelay = INITIAL_RECONNECT_DELAY_MS
            startPingLoop()
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            try {
                val event = json.decodeFromString<ServerEvent>(text)
                val emitted = _events.tryEmit(event)
                if (!emitted) {
                    logger.warning("Event buffer full, dropped: ${event::class.simpleName}")
                }
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to parse server event: $text", e)
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            logger.info("WebSocket closing: code=$code reason=$reason")
            webSocket.close(code, reason)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            logger.info("WebSocket closed: code=$code reason=$reason")
            handleDisconnect()
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            logger.log(Level.WARNING, "WebSocket failure: ${t.message}", t)
            handleDisconnect()
        }
    }

    private fun handleDisconnect() {
        pingJob?.cancel()
        pingJob = null
        webSocket = null
        _connectionState.value = ConnectionState.Disconnected

        if (shouldReconnect) {
            scheduleReconnect()
        }
    }

    private fun startPingLoop() {
        pingJob?.cancel()
        pingJob = scope.launch {
            while (true) {
                delay(PING_INTERVAL_MS)
                send(ClientEvent.Ping)
            }
        }
    }

    private fun scheduleReconnect() {
        reconnectJob?.cancel()
        reconnectJob = scope.launch {
            // If offline, don't attempt to reconnect — the connectivity monitor
            // will trigger reconnection when network returns.
            val monitor = connectivityMonitor
            if (monitor != null && !monitor.isConnected.value) {
                logger.info("Network unavailable, deferring reconnect")
                return@launch
            }

            logger.info("Scheduling reconnect in ${reconnectDelay}ms")
            delay(reconnectDelay)
            reconnectDelay = (reconnectDelay * 2).coerceAtMost(MAX_RECONNECT_DELAY_MS)
            doConnect()
        }
    }
}
