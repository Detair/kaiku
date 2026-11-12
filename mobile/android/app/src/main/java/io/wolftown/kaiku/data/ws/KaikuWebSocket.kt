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
        private const val HTTP_UNAUTHORIZED = 401
        private const val HTTP_FORBIDDEN = 403
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

    /** Counts authenticated sockets: bumped with each socket's Ready. Guarded by [lock]. */
    private var authenticatedSockets = 0L

    private val _authenticatedSocket = MutableStateFlow<Long?>(null)

    /**
     * The id of the socket that is authenticated now (its Ready was received),
     * or null while there is none. Every authenticated socket gets a new id
     * and ids never repeat, so "is the socket I used still there?" is the
     * plain comparison with the current value, and it never turns true again
     * once it was false.
     *
     * Changes together with [connectionState], under the same lock: non-null
     * exactly while that is Connected.
     */
    val authenticatedSocket: StateFlow<Long?> = _authenticatedSocket.asStateFlow()

    /** Sets [connectionState] and, with it, [authenticatedSocket]. Call with [lock] held. */
    private fun setState(state: ConnectionState) {
        _authenticatedSocket.value = if (state == ConnectionState.Connected) ++authenticatedSockets else null
        _connectionState.value = state
    }

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
                setState(ConnectionState.Disconnected)
                return
            }

            setState(ConnectionState.Connecting)
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
            setState(ConnectionState.Disconnected)
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

    /**
     * Sends [event] on the authenticated socket [socketId] and on no other.
     * Returns true if the frame went out on that socket, false (nothing was
     * sent) if it is not the authenticated socket any more. The check and the
     * send are one step under the socket lock, so the frame can never end up
     * on a newer socket, or on one that is still authenticating.
     *
     * Voice signalling sends every frame this way, with the id it read from
     * [authenticatedSocket]. Nothing voice-related goes out while a socket is
     * still Connecting (such a frame is processed if it arrives after the
     * Authenticate frame and dropped otherwise, so its effect would be
     * unknown), and the caller knows which socket carried each frame.
     *
     * A socket that refuses the frame although it is still the authenticated
     * one is closing; its close is reported next.
     */
    fun sendOn(socketId: Long, event: ClientEvent): Boolean {
        val text = json.encodeToString<ClientEvent>(event)
        val sent = synchronized(lock) {
            if (_authenticatedSocket.value != socketId) return false
            webSocket?.send(text) ?: false
        }
        if (!sent) {
            logger.warning("send failed (socket closing): ${event::class.simpleName}")
        }
        return sent
    }

    // -- Internal -------------------------------------------------------------

    private fun toWsUrl(serverUrl: String): String = serverUrl
        .replace("https://", "wss://")
        .replace("http://", "ws://")
        .trimEnd('/') + "/ws"

    /**
     * Marks [socket] closed with [code] if it is still the current socket.
     * [AUTH_REJECTED_CLOSE_CODE] (close 1008, or a handshake 401/403) maps to AuthRejected.
     */
    private fun handleClosed(socket: WebSocket, code: Int?) {
        synchronized(lock) {
            if (webSocket !== socket) return
            webSocket = null
            socketOpen = false
            setState(
                if (code == AUTH_REJECTED_CLOSE_CODE) ConnectionState.AuthRejected else ConnectionState.Disconnected
            )
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
                    setState(ConnectionState.Connected)
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
            // Contract C11: a 401/403 answer to the upgrade request is an auth rejection,
            // same as close 1008. Only the status code is read (OkHttp closes the response).
            val httpCode = response?.code
            logger.warning("socket failure: ${t.javaClass.simpleName}" + (httpCode?.let { " http=$it" } ?: ""))
            val authRejected = httpCode == HTTP_UNAUTHORIZED || httpCode == HTTP_FORBIDDEN
            handleClosed(webSocket, if (authRejected) AUTH_REJECTED_CLOSE_CODE else null)
        }
    }
}
