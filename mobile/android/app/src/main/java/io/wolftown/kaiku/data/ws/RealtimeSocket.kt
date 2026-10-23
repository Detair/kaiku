package io.wolftown.kaiku.data.ws

import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

/** Lifecycle of a single realtime socket. */
enum class ConnectionState {
    Connected,
    Connecting,
    Disconnected,

    /** The server closed the socket with 1008 (authentication rejected). */
    AuthRejected
}

/**
 * A realtime transport: opens one socket, reports its state and events, and
 * nothing else. Reconnecting, refreshing and the heartbeat are owned by
 * [RealtimeConnectionManager].
 */
interface RealtimeSocket {
    val connectionState: StateFlow<ConnectionState>
    val events: SharedFlow<ServerEvent>

    /** Opens a socket unless one is already Connecting or Connected. */
    fun connect(serverUrl: String, accessToken: String)

    fun disconnect()

    fun send(event: ClientEvent): Boolean
}

/** Network availability as seen by the realtime layer. */
interface ConnectivitySignal {
    val isConnected: StateFlow<Boolean>

    /** Handle of the current default network (`Network.getNetworkHandle()`), or null when there is none. */
    val networkId: StateFlow<Long?>
}

/** Wall-clock source, injectable so tests can use virtual time. */
fun interface WallClock {
    fun nowMillis(): Long
}
