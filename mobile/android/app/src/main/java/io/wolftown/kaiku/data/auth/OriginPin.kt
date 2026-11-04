package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.KaikuPayloads
import javax.inject.Inject

/**
 * The origin this app is tied to (B.1; coordinator call 6): a key ties it to
 * the key's server, a session to the session's server; a saved server URL
 * alone does not. Null means unconfigured: only then may a setup code bring a
 * new server.
 */
class OriginPin @Inject constructor(
    private val keyDevices: KeyDeviceRepository,
    private val authState: AuthState,
    private val tokenStorage: TokenStorage,
) {
    fun pinned(): String? =
        keyDevices.pinnedOrigin()?.let(KaikuPayloads::canonicalOrigin)
            ?: if (authState.isLoggedIn.value) tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin) else null

    /**
     * The origin every device signature must name (C26 challenge check): the
     * key's server whenever a key exists, whatever server is saved (a reset or
     * changed server URL never moves a key); otherwise the configured server
     * (the setup of this phone talks to the server the person confirmed).
     */
    fun signingOrigin(): String? =
        keyDevices.pinnedOrigin()?.let(KaikuPayloads::canonicalOrigin)
            ?: tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin)
}
