package io.wolftown.kaiku.data.local

import io.wolftown.kaiku.di.AuthCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.getAndUpdate
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import java.io.Closeable
import java.util.concurrent.atomic.AtomicLong
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

sealed class AuthSession {
    data object LoggedOut : AuthSession()

    /**
     * A signed-in session. [sessionId] is unique per process (every
     * [AuthState.setLoggedIn] takes the next one), so two logins of the same
     * user are never equal. Session teardown and the connection manager
     * identify sessions by it.
     */
    data class LoggedIn(val userId: String, val sessionId: Long) : AuthSession()
}

@Singleton
class AuthState @Inject constructor(
    @AuthCoroutineScope private val scope: CoroutineScope,
) : Closeable {

    private val _session = MutableStateFlow<AuthSession>(AuthSession.LoggedOut)
    val session: StateFlow<AuthSession> = _session.asStateFlow()

    private val nextSessionId = AtomicLong(0)

    private val _endedSessions = MutableSharedFlow<AuthSession.LoggedIn>(
        extraBufferCapacity = ENDED_SESSIONS_BUFFER,
    )

    /**
     * Every session that ended (replaced by LoggedOut or by another login),
     * emitted by the transition itself. Unlike [session] it never conflates,
     * so an observer sees each ended session even if several transitions
     * happen before it resumes. Replay 0: only for observers started before
     * the first login (session teardown, started in `Application.onCreate`).
     */
    val endedSessions: SharedFlow<AuthSession.LoggedIn> = _endedSessions.asSharedFlow()

    val isLoggedIn: StateFlow<Boolean> = _session.map { it is AuthSession.LoggedIn }
        .stateIn(scope, SharingStarted.Eagerly, false)

    val currentUserId: StateFlow<String?> = _session.map { (it as? AuthSession.LoggedIn)?.userId }
        .stateIn(scope, SharingStarted.Eagerly, null)

    /** Starts a new session (new [AuthSession.LoggedIn.sessionId]); a running one ends. */
    fun setLoggedIn(userId: String) {
        require(userId.isNotBlank()) { "userId must not be blank" }
        val next = AuthSession.LoggedIn(userId, nextSessionId.incrementAndGet())
        emitEnded(_session.getAndUpdate { next })
    }

    fun setLoggedOut() {
        emitEnded(_session.getAndUpdate { AuthSession.LoggedOut })
    }

    /**
     * Logs out only if [sessionId] is still the current session, so ending an
     * old session never logs out a newer login. Returns whether it did.
     */
    fun setLoggedOutIf(sessionId: Long): Boolean {
        while (true) {
            val current = _session.value
            if (current !is AuthSession.LoggedIn || current.sessionId != sessionId) return false
            if (_session.compareAndSet(current, AuthSession.LoggedOut)) {
                emitEnded(current)
                return true
            }
        }
    }

    /** Id of the session that ended last, or null; see [endedSessions]. */
    @Volatile
    var lastEndedSessionId: Long? = null
        private set

    private fun emitEnded(previous: AuthSession) {
        if (previous is AuthSession.LoggedIn) lastEndedSessionId = previous.sessionId
        if (previous is AuthSession.LoggedIn && !_endedSessions.tryEmit(previous)) {
            // Only if ENDED_SESSIONS_BUFFER transitions pile up unobserved.
            logger.warning("ended session not delivered to observers")
        }
    }

    /**
     * Restores auth state from persisted tokens on app start.
     *
     * No-op while a session is running: MainActivity calls this in every
     * onCreate (rotation, dark mode, split screen), and a recreation must
     * neither start a new session identity nor end the running one, e.g.
     * while a logout has cleared the tokens but not yet ended its session.
     *
     * If valid tokens exist (non-expired, or expired with a refresh token
     * available), the user is considered logged in — the HTTP interceptor
     * will refresh the token transparently on the first API call.
     */
    fun initialize(tokenStorage: TokenStorage) {
        if (_session.value is AuthSession.LoggedIn) return
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

    private companion object {
        val logger: Logger = Logger.getLogger("AuthState")
        const val ENDED_SESSIONS_BUFFER = 64
    }

    override fun close() {
        // Scope is now owned by Hilt's SingletonComponent (see AuthCoroutineScopeModule)
        // — do not cancel here. Same rationale as WebRtcManager.dispose() after #538:
        // SupervisorJob, potential future shared consumers, app-lifetime ownership.
    }
}
