package io.wolftown.kaiku.data.local

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import java.io.Closeable
import javax.inject.Inject
import javax.inject.Singleton

sealed class AuthSession {
    data object LoggedOut : AuthSession()
    data class LoggedIn(val userId: String) : AuthSession()
}

@Singleton
class AuthState @Inject constructor() : Closeable {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private val _session = MutableStateFlow<AuthSession>(AuthSession.LoggedOut)
    val session: StateFlow<AuthSession> = _session.asStateFlow()

    val isLoggedIn: StateFlow<Boolean> = _session.map { it is AuthSession.LoggedIn }
        .stateIn(scope, SharingStarted.Eagerly, false)

    val currentUserId: StateFlow<String?> = _session.map { (it as? AuthSession.LoggedIn)?.userId }
        .stateIn(scope, SharingStarted.Eagerly, null)

    fun setLoggedIn(userId: String) {
        require(userId.isNotBlank()) { "userId must not be blank" }
        _session.value = AuthSession.LoggedIn(userId)
    }

    fun setLoggedOut() {
        _session.value = AuthSession.LoggedOut
    }

    /**
     * Restores auth state from persisted tokens on app start.
     *
     * If valid tokens exist (non-expired, or expired with a refresh token
     * available), the user is considered logged in — the HTTP interceptor
     * will refresh the token transparently on the first API call.
     */
    fun initialize(tokenStorage: TokenStorage) {
        val token = tokenStorage.getAccessToken()
        val userId = tokenStorage.getUserId()
        val refreshToken = tokenStorage.getRefreshToken()

        if (token != null && !userId.isNullOrBlank()) {
            // Valid token or expired-but-refreshable — let the HTTP interceptor handle refresh
            if (!tokenStorage.isAccessTokenExpired() || refreshToken != null) {
                setLoggedIn(userId)
                return
            }
        }

        setLoggedOut()
    }

    override fun close() {
        scope.cancel()
    }
}
