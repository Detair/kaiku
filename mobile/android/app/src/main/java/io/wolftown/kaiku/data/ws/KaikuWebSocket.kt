package io.wolftown.kaiku.data.ws

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * WebSocket transport to the Kaiku server.
 *
 * A pure transport: it opens one socket, authenticates with the first frame,
 * reports [connectionState] and parsed [events], and closes on request. It has
 * no timers and never reconnects on its own; [RealtimeConnectionManager] owns
 * reconnecting, token refresh and the heartbeat.
 *
 * Callbacks from a socket that has since been replaced or closed by
 * [disconnect] are ignored, so a late close never clobbers a newer socket.
 */
@Singleton
class KaikuWebSocket @Inject constructor(
    private val okHttpClient: OkHttpClient,
    private val json: Json
) : RealtimeSocket {
    companion object {
        private val logger = Logger.getLogger("KaikuWebSocket")

        /** WebSocket close code 1008 (Policy Violation): the server rejected authentication. */
        internal const val AUTH_REJECTED_CLOSE_CODE = 1008
        private const val NORMAL_CLOSURE = 1000
    }

    private val lock = Any()

    private val _events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
    override val events: SharedFlow<ServerEvent> = _events.asSharedFlow()

    private val _connectionState = MutableStateFlow(ConnectionState.Disconnected)
    override val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    /** The current socket; guarded by [lock]. */
    private var webSocket: WebSocket? = null

    /** Whether [webSocket] completed the upgrade (onOpen); guarded by [lock]. */
    private var socketOpen = false

    /**
     * Opens a WebSocket connection to [serverUrl] and authenticates with
     * [accessToken] in the first frame. Does nothing while a socket is
     * Connecting or Connected.
     *
     * [serverUrl] is the server's base HTTP(S) URL; it is converted to
     * `ws://` / `wss://` with `/ws` appended. The token never goes into the URL.
     */
    override fun connect(serverUrl: String, accessToken: String) {
        synchronized(lock) {
            val state = _connectionState.value
            if (state == ConnectionState.Connecting || state == ConnectionState.Connected) {
                logger.fine("connect ignored: state=$state")
                return
            }

            val request = try {
                Request.Builder().url(toWsUrl(serverUrl)).build()
            } catch (e: IllegalArgumentException) {
                logger.warning("connect failed: invalid server URL")
                _connectionState.value = ConnectionState.Disconnected
                return
            }

            _connectionState.value = ConnectionState.Connecting
            socketOpen = false
            webSocket = okHttpClient.newWebSocket(request, Listener(accessToken))
        }
    }

    override fun disconnect() {
        synchronized(lock) {
            val socket = webSocket
            val open = socketOpen
            webSocket = null
            socketOpen = false
            if (socket != null) {
                // A socket still upgrading is aborted; an open one gets a close handshake.
                if (!open || !socket.close(NORMAL_CLOSURE, null)) socket.cancel()
            }
            _connectionState.value = ConnectionState.Disconnected
        }
    }

    override fun send(event: ClientEvent): Boolean {
        val socket = synchronized(lock) { webSocket }
        val sent = socket?.send(json.encodeToString<ClientEvent>(event)) ?: false
        if (!sent) {
            logger.warning("send failed (not connected): ${event::class.simpleName}")
        }
        return sent
    }

    // -- Internal -------------------------------------------------------------

    private fun toWsUrl(serverUrl: String): String = serverUrl
        .replace("https://", "wss://")
        .replace("http://", "ws://")
        .trimEnd('/') + "/ws"

    /** Marks [socket] closed with [code] if it is still the current socket. */
    private fun handleClosed(socket: WebSocket, code: Int?) {
        synchronized(lock) {
            if (webSocket !== socket) return
            webSocket = null
            socketOpen = false
            _connectionState.value = if (code == AUTH_REJECTED_CLOSE_CODE) {
                ConnectionState.AuthRejected
            } else {
                ConnectionState.Disconnected
            }
        }
    }

    private fun eventTypeOf(text: String): String = try {
        json.parseToJsonElement(text).jsonObject["type"]?.jsonPrimitive?.contentOrNull ?: "unknown"
    } catch (_: Exception) {
        "unparseable"
    }

    private inner class Listener(private val accessToken: String) : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            synchronized(lock) {
                if (this@KaikuWebSocket.webSocket !== webSocket) return
                socketOpen = true
            }
            // Authenticate via the first frame (post-connect auth); stay Connecting until Ready.
            webSocket.send(json.encodeToString<ClientEvent>(ClientEvent.Authenticate(accessToken)))
            logger.info("socket open, awaiting ready")
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            val event = try {
                json.decodeFromString<ServerEvent>(text)
            } catch (e: Exception) {
                logger.warning("failed to parse server event: type=${eventTypeOf(text)} (${e.javaClass.simpleName})")
                return
            }
            synchronized(lock) {
                if (this@KaikuWebSocket.webSocket !== webSocket) return
                if (event is ServerEvent.Ready && _connectionState.value == ConnectionState.Connecting) {
                    logger.info("ready received, state=Connected")
                    _connectionState.value = ConnectionState.Connected
                }
                if (!_events.tryEmit(event)) {
                    logger.warning("event buffer full, dropped: ${event::class.simpleName}")
                }
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            logger.info("socket closing: code=$code")
            handleClosed(webSocket, code)
            webSocket.close(NORMAL_CLOSURE, null)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            logger.info("socket closed: code=$code")
            handleClosed(webSocket, code)
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            logger.warning("socket failure: ${t.javaClass.simpleName}")
            handleClosed(webSocket, null)
        }
    }
}
