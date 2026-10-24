package io.wolftown.kaiku.data.ws

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.Closeable
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Observes device network connectivity via Android's [ConnectivityManager].
 *
 * Exposes [isConnected] and the default network's [networkId] so that
 * [RealtimeConnectionManager] can pause reconnects while offline and force a
 * reconnect when the device switches networks (e.g. Wi-Fi to cellular).
 */
@Singleton
class ConnectivityMonitor @Inject constructor(
    @ApplicationContext private val context: Context
) : ConnectivitySignal, Closeable {
    private val connectivityManager =
        context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager

    private val _isConnected = MutableStateFlow(checkCurrentConnectivity())
    override val isConnected: StateFlow<Boolean> = _isConnected.asStateFlow()

    private val _networkId = MutableStateFlow(connectivityManager.activeNetwork?.networkHandle)
    override val networkId: StateFlow<Long?> = _networkId.asStateFlow()

    /** Tracks the system default network, whose handle identifies "the" network we are on. */
    private val defaultNetworkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            _networkId.value = network.networkHandle
        }

        override fun onLost(network: Network) {
            if (_networkId.value == network.networkHandle) {
                _networkId.value = null
            }
        }
    }

    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            _isConnected.value = true
        }

        override fun onLost(network: Network) {
            // Re-check: there may be another active network
            _isConnected.value = checkCurrentConnectivity()
        }

        override fun onCapabilitiesChanged(
            network: Network,
            networkCapabilities: NetworkCapabilities
        ) {
            // Only the default network decides; a secondary network changing its
            // capabilities must not mark the device offline (the realtime manager
            // drops the socket when offline).
            _isConnected.value = if (network == connectivityManager.activeNetwork) {
                networkCapabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                    networkCapabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
            } else {
                checkCurrentConnectivity()
            }
        }
    }

    init {
        val request = NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .addCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
            .build()
        connectivityManager.registerNetworkCallback(request, networkCallback)
        connectivityManager.registerDefaultNetworkCallback(defaultNetworkCallback)
    }

    private fun checkCurrentConnectivity(): Boolean {
        val network = connectivityManager.activeNetwork ?: return false
        val capabilities = connectivityManager.getNetworkCapabilities(network) ?: return false
        return capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            && capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
    }

    override fun close() {
        for (callback in listOf(networkCallback, defaultNetworkCallback)) {
            try {
                connectivityManager.unregisterNetworkCallback(callback)
            } catch (_: IllegalArgumentException) {
                // Already unregistered
            }
        }
        _isConnected.value = false
        _networkId.value = null
    }
}
