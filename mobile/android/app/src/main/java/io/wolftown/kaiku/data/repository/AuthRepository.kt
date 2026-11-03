package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDevicePolicy
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.session.SessionTeardownRunner
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.RegistrationTicket
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.ui.auth.PasswordChangeRequiredException
import java.util.logging.Level
import java.util.logging.Logger
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Session handed out by a login that still requires a password change (C4).
 * Held in memory only, never persisted, and never logged: [toString] hides
 * the tokens and the temporary password.
 */
internal data class PendingPasswordChange(
    /** Server that issued the session; the change goes only there. */
    val serverUrl: String,
    val accessToken: String,
    val refreshToken: String?,
    val expiresIn: Int,
    val currentPassword: String
) {
    override fun toString(): String = "PendingPasswordChange(expiresIn=$expiresIn)"
}

// Singleton: the pending password change must survive from LoginViewModel to
// ChangePasswordViewModel, which would otherwise get separate instances.
@Singleton
class AuthRepository @Inject constructor(
    private val authApi: AuthApi,
    private val tokenStorage: TokenStorage,
    private val authState: AuthState,
    private val pushRegistrar: PushRegistrar,
    private val sessionTeardown: SessionTeardownRunner
) {
    companion object {
        private val logger = Logger.getLogger("AuthRepository")

        /** Upper bound for the best-effort server logout. */
        const val SERVER_LOGOUT_TIMEOUT_MS = 10_000L

        /** Logout never waits longer than this for the push device DELETE. */
        const val PUSH_UNREGISTER_TIMEOUT_MS = 3_000L
    }

    /**
     * L2 (C26 amendment 3): on a key device a plain sign-out keeps the push
     * subscription, so a lapsed key device still gets call wakes and notices.
     */
    @set:Inject
    var keyDevicePolicy: KeyDevicePolicy = KeyDevicePolicy.NONE

    /**
     * Set by [completeAttempt] when the server requires a password change; see
     * [completePasswordChange]. Guarded by [pendingLock], so a cancel and the
     * final persist step can't interleave.
     */
    private val pendingLock = Any()
    @Volatile
    private var pendingPasswordChange: PendingPasswordChange? = null

    /**
     * Bumped by every login, cancel and logout (under [pendingLock]). A login
     * response only installs or persists anything if its attempt is still the
     * latest, so a late response can't undo a cancel, a logout or a newer login.
     */
    private var loginAttempt = 0L

    /** Swaps the pending state only if it is still [expected] (same instance). */
    private fun replacePendingIf(expected: PendingPasswordChange, next: PendingPasswordChange?): Boolean =
        synchronized(pendingLock) {
            if (pendingPasswordChange === expected) {
                pendingPasswordChange = next
                true
            } else {
                false
            }
        }

    /** Drops the pending state and invalidates any login still in flight. */
    private fun invalidateLogin(): Long = synchronized(pendingLock) {
        pendingPasswordChange = null
        ++loginAttempt
    }

    private fun supersededResult(): Result<User> =
        Result.failure(IllegalStateException("Login was cancelled or superseded"))

    /** True while a login is waiting for the user to choose a new password. */
    fun hasPendingPasswordChange(): Boolean = pendingPasswordChange != null

    /**
     * Every sign-in path refuses to replace a running session (e.g. scanning
     * another account's sign-in from a stale screen, or a stray OIDC callback):
     * the user signs out first, so the old session is always torn down
     * completely before another one starts, and its token is never sent
     * along with the new sign-in. Checked at the start and again right
     * before the new session is stored.
     */
    private fun signedInRefusal(): Result<User>? =
        if (authState.session.value is AuthSession.LoggedIn) Result.failure(AlreadySignedInException()) else null

    /**
     * The one place a new session is stored and started, used by every
     * sign-in path: under [pendingLock], refuses while a session runs or if
     * [precondition] fails, otherwise saves the tokens, runs [onInstalled]
     * and sets LoggedIn. Two sign-ins racing while logged out (a passkey
     * and a collected attempt) can therefore never both install. Returns
     * whether it installed.
     */
    private fun installSession(
        accessToken: String,
        refreshToken: String?,
        expiresIn: Int,
        userId: String,
        precondition: () -> Boolean = { true },
        onInstalled: () -> Unit = {},
    ): Boolean = synchronized(pendingLock) {
        if (authState.session.value is AuthSession.LoggedIn || !precondition()) return false
        tokenStorage.saveTokens(
            accessToken = accessToken,
            refreshToken = refreshToken,
            expiresIn = expiresIn,
            userId = userId
        )
        onInstalled()
        authState.setLoggedIn(userId)
        true
    }

    /**
     * Completes a C27 sign-in attempt (SM1): [collect] is the attempt's `collect`
     * or `totp` call, which mints the probation session on [serverUrl], the
     * server the attempt was started on. Every rule of the single install point
     * applies: refused while a session runs, and only the latest sign-in, still
     * on [serverUrl], stores anything (a cancel, logout or newer sign-in while
     * [collect] is in flight wins). If the server reports
     * `password_change_required`, nothing is persisted: the session is kept in
     * memory and [PasswordChangeRequiredException] is returned, and the
     * [typedPassword] of a password attempt later becomes `current_password` in
     * [completePasswordChange] (C4; without one the session is installed as is).
     */
    suspend fun completeAttempt(
        serverUrl: String,
        typedPassword: String?,
        collect: suspend () -> AuthResponse,
    ): Result<User> {
        signedInRefusal()?.let { return it }
        val attempt = invalidateLogin()
        if (tokenStorage.getServerUrl() != serverUrl) return supersededResult()
        return try {
            val authResponse = collect()

            if (authResponse.passwordChangeRequired && typedPassword != null) {
                val pending = PendingPasswordChange(
                    serverUrl = serverUrl,
                    accessToken = authResponse.accessToken,
                    refreshToken = authResponse.refreshToken,
                    expiresIn = authResponse.expiresIn,
                    currentPassword = typedPassword
                )
                val installed = synchronized(pendingLock) {
                    if (loginAttempt == attempt && tokenStorage.getServerUrl() == serverUrl) {
                        pendingPasswordChange = pending
                        true
                    } else {
                        false
                    }
                }
                if (!installed) return superseded(serverUrl, authResponse)
                return Result.failure(PasswordChangeRequiredException())
            }

            // Server switched while the attempt was collected: don't send server A's
            // token anywhere else (the minted session is ended on A), and leave B alone.
            if (tokenStorage.getServerUrl() != serverUrl) return superseded(serverUrl, authResponse)
            val user = authApi.authenticatedGetMe(authResponse.accessToken, serverUrl)

            // The previous session's teardown finishes before this one starts.
            sessionTeardown.awaitIdle()

            // Persist only for the latest attempt and the server it went to: a
            // server change mid-attempt must not store server A's tokens under B.
            val persisted = installSession(
                authResponse.accessToken, authResponse.refreshToken, authResponse.expiresIn, user.id,
                precondition = { loginAttempt == attempt && tokenStorage.getServerUrl() == serverUrl },
            )
            if (!persisted) return superseded(serverUrl, authResponse)
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Only the latest attempt may clear, and never under a running session: a stale
            // sign-in failing late must not wipe the session a newer one stored meanwhile.
            synchronized(pendingLock) {
                if (loginAttempt == attempt && tokenStorage.getServerUrl() == serverUrl &&
                    authState.session.value !is AuthSession.LoggedIn
                ) {
                    tokenStorage.clear()
                }
            }
            Result.failure(e)
        }
    }

    /** A collected session that is not installed is ended on its server, so none is left live. */
    private suspend fun superseded(serverUrl: String, auth: AuthResponse): Result<User> {
        endUnused(serverUrl, auth)
        return supersededResult()
    }

    /** False while a session runs: a passkey sign-in then starts nothing. */
    fun canStartSignIn(): Boolean = signedInRefusal() == null

    /**
     * Installs a session from a sign-in that needed no password (a passkey,
     * C29): `GET /auth/me` with the new token, then the same single install
     * point as [completeAttempt], only for the latest attempt and the server it went to.
     * A result that is not installed (a session runs, the server changed, or
     * another sign-in came first) ends its session on [serverUrl], so a
     * second result never leaves an unused session live.
     */
    suspend fun installNativeSignIn(authResponse: AuthResponse, serverUrl: String?): Result<User> {
        signedInRefusal()?.let { refused ->
            serverUrl?.let { endUnused(it, authResponse) }
            return refused
        }
        val attempt = invalidateLogin()
        if (serverUrl == null) return supersededResult()
        if (tokenStorage.getServerUrl() != serverUrl) {
            endUnused(serverUrl, authResponse)
            return supersededResult()
        }
        return try {
            val user = authApi.authenticatedGetMe(authResponse.accessToken, serverUrl)
            sessionTeardown.awaitIdle()
            val persisted = installSession(
                authResponse.accessToken, authResponse.refreshToken, authResponse.expiresIn, user.id,
                precondition = { loginAttempt == attempt && tokenStorage.getServerUrl() == serverUrl },
            )
            if (!persisted) {
                endUnused(serverUrl, authResponse)
                supersededResult()
            } else {
                Result.success(user)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Ends a minted session that was not installed, on the server that
     * minted it, with its own tokens only; best effort, bounded, never
     * cancelled halfway. The one helper for every sign-in that mints a
     * session: a refused or superseded passkey result ([installNativeSignIn]),
     * a collected attempt ([completeAttempt]) and a device-key sign-in
     * ([installDeviceSession]).
     */
    private suspend fun endUnused(serverUrl: String, auth: AuthResponse) {
        withContext(NonCancellable) {
            try {
                withTimeoutOrNull(SERVER_LOGOUT_TIMEOUT_MS) {
                    authApi.logoutSession(serverUrl, auth.accessToken, auth.refreshToken)
                }
            } catch (e: Exception) {
                logger.warning("Unused session not ended (best-effort): ${e.javaClass.simpleName}")
            }
        }
    }

    /**
     * Sets the user's own password for the pending login, then completes it:
     * `POST /auth/me/password`, `GET /auth/me`, persist the tokens, log in.
     * The pending state is cleared on success. Fails with
     * [IllegalStateException] if no password change is pending, or if it was
     * cancelled or replaced while a request was in flight: then nothing is
     * persisted and the user is not logged in. The requests go only to the
     * server that issued the pending session; if the configured server changed
     * meanwhile, the pending change is dropped and nothing is sent.
     */
    suspend fun completePasswordChange(newPassword: String): Result<User> {
        signedInRefusal()?.let { return it }
        val pending = pendingPasswordChange
            ?: return Result.failure(IllegalStateException("No password change pending"))
        if (!isSameServer(pending)) {
            replacePendingIf(pending, null)
            return cancelledResult()
        }
        return try {
            authApi.changePassword(pending.serverUrl, pending.accessToken, pending.currentPassword, newPassword)
            // The new password is now the current one: a retry after a failed
            // getMe must not send the old temporary password again.
            val changed = pending.copy(currentPassword = newPassword)
            if (!replacePendingIf(pending, changed)) return cancelledResult()
            if (!isSameServer(pending)) {
                replacePendingIf(changed, null)
                return cancelledResult()
            }

            val user = authApi.authenticatedGetMe(pending.accessToken, pending.serverUrl)

            // The previous session's teardown finishes before this one starts.
            sessionTeardown.awaitIdle()

            // Check and persist under the lock: a cancel can't slip in between.
            val persisted = installSession(
                pending.accessToken, pending.refreshToken, pending.expiresIn, user.id,
                precondition = { pendingPasswordChange === changed && isSameServer(pending) },
                onInstalled = { pendingPasswordChange = null },
            )
            if (!persisted) return cancelledResult()
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    private fun isSameServer(pending: PendingPasswordChange): Boolean =
        tokenStorage.getServerUrl() == pending.serverUrl

    private fun cancelledResult(): Result<User> =
        Result.failure(IllegalStateException("Password change was cancelled"))

    /** Drops the pending password change (Back on the change-password screen). */
    fun cancelPasswordChange() {
        invalidateLogin()
    }

    /**
     * F8: registration makes an account without a session; the answer is a
     * 30-minute ticket that binds this phone as the account's key (the setup
     * flow claims with it and then signs in with the key).
     */
    suspend fun register(
        username: String,
        password: String,
        email: String?,
        displayName: String?
    ): Result<RegistrationTicket> {
        signedInRefusal()?.let { return Result.failure(it.exceptionOrNull()!!) }
        return try {
            Result.success(authApi.register(username, password, email, displayName))
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Signs out the session that is current when it is called (the captured
     * session), on this device and on the server, in this order:
     *
     * 1. push unregister FIRST, while the session token still works
     *    ([PushRegistrar.unregisterForLogout], C19: the device row is deleted;
     *    bounded by [PUSH_UNREGISTER_TIMEOUT_MS]). The DELETE is bound to the
     *    captured session id, so it never goes out with another session's
     *    token (a new sign-in is refused while this session runs)
     * 2. session teardown ([SessionTeardownRunner]) for the captured session:
     *    leave voice, close the realtime connection, clear caches; shared with
     *    the LoggedOut observer, so it runs once
     * 3. server logout, best effort and bounded by [SERVER_LOGOUT_TIMEOUT_MS],
     *    with the captured session's own tokens only ([AuthApi.logoutSession]:
     *    explicit bearer, no interceptor, no refresh, body/bearer-only/body-only
     *    attempts). If a refresh rotated the session's tokens during the
     *    teardown, the rotated ones are used, read under a generation check so
     *    a newer login's tokens can never be picked up
     * 4. clear the stored tokens only if they still belong to the captured
     *    session (session generation), and set LoggedOut only if the captured
     *    session is still current
     *
     * Runs to the end even if the caller is cancelled (e.g. its ViewModel is
     * cleared); tokens are never logged.
     */
    suspend fun logout() {
        withContext(NonCancellable) { logoutCaptured() }
    }

    private suspend fun logoutCaptured() {
        invalidateLogin()
        val session = authState.session.value as? AuthSession.LoggedIn
        val generation = tokenStorage.getSessionGeneration()
        val serverUrl = tokenStorage.getServerUrl()
        val capturedAccess = tokenStorage.getAccessToken()
        val capturedRefresh = tokenStorage.getRefreshToken()

        // First step, while the captured session's tokens still authenticate the DELETE.
        // A key device keeps its subscription (C19 amendment 3).
        if (!keyDevicePolicy.keepsPushOnSignOut()) unregisterPush(session?.sessionId)

        sessionTeardown.tearDown(session?.sessionId).join()

        // The captured session's tokens, rotated if a refresh ran meanwhile.
        // Read between two generation checks: a login landing in between
        // changes the generation, and then the captured tokens are kept.
        var access = capturedAccess
        var refresh = capturedRefresh
        if (tokenStorage.getSessionGeneration() == generation) {
            val rotatedAccess = tokenStorage.getAccessToken()
            val rotatedRefresh = tokenStorage.getRefreshToken()
            if (tokenStorage.getSessionGeneration() == generation) {
                access = rotatedAccess ?: capturedAccess
                refresh = rotatedRefresh ?: capturedRefresh
            }
        }

        try {
            val finished = withTimeoutOrNull(SERVER_LOGOUT_TIMEOUT_MS) {
                if (serverUrl != null && (access != null || refresh != null)) {
                    authApi.logoutSession(serverUrl, access, refresh)
                }
                true
            }
            if (finished == null) logger.warning("Server-side logout timed out (best-effort)")
        } catch (e: Exception) {
            // Includes cancellation from the request: the local sign-out below must still run.
            logger.log(Level.WARNING, "Server-side logout failed (best-effort): ${e::class.simpleName}")
        }

        tokenStorage.clearIfGeneration(generation)
        if (session != null) authState.setLoggedOutIf(session.sessionId)
    }

    /**
     * Removes this device's push registration for the ending session
     * [sessionId]; bounded and best effort, never blocks logout.
     */
    private suspend fun unregisterPush(sessionId: Long?) {
        try {
            withTimeoutOrNull(PUSH_UNREGISTER_TIMEOUT_MS) { pushRegistrar.unregisterForLogout(sessionId) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.warning("Push unregister failed (best-effort): ${e.javaClass.simpleName}")
        }
    }

    suspend fun getCurrentUser(): Result<User> {
        return try {
            val user = authApi.getMe()
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /** A session runs and its tokens are stored. */
    fun hasLiveSession(): Boolean =
        authState.session.value is AuthSession.LoggedIn && tokenStorage.getAccessToken() != null

    /**
     * Starts a device-key sign-in (C26) for the key bound to [keyOrigin]: records
     * the sign-in generation, the server and the running session, which
     * [installDeviceSession] checks again before it installs anything. The key
     * ties the app to its server (B.1, coordinator call 6): without a running
     * session the app's server becomes the key's (a reset session store loses the
     * saved server, the key's metadata survives); with a session on another
     * server the sign-in is refused (null).
     */
    fun beginDeviceSignIn(keyOrigin: String): DeviceSignInStart? = synchronized(pendingLock) {
        val session = authState.session.value
        if (tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin) != keyOrigin) {
            if (session is AuthSession.LoggedIn) return null
            tokenStorage.saveServerUrl(keyOrigin)
        }
        val serverUrl = tokenStorage.getServerUrl() ?: return null
        // A sign-in of its own: an older sign-in still in flight can no longer install or
        // clear anything, and two overlapping device sign-ins never both install.
        pendingPasswordChange = null
        DeviceSignInStart(++loginAttempt, serverUrl, session)
    }

    /**
     * Installs the full session of a device-key sign-in for [expectedUserId];
     * the profile must be that user's. It installs only if nothing changed since
     * [start]: no logout or other sign-in, the same server, the same running
     * session (none, or for the B.4 renewal the same one, which it replaces and
     * ends on the server with its bearer only). A session it does not install is
     * ended on the server, so a late answer never leaves a live session behind.
     */
    suspend fun installDeviceSession(auth: AuthResponse, expectedUserId: String, start: DeviceSignInStart): Result<User> {
        val serverUrl = start.serverUrl
        return try {
            val user = authApi.authenticatedGetMe(auth.accessToken, serverUrl)
            if (user.id != expectedUserId) {
                endUnused(serverUrl, auth)
                return Result.failure(IllegalStateException("device session for another account"))
            }
            val running = start.session as? AuthSession.LoggedIn
            if (running != null && running.userId != user.id) {
                endUnused(serverUrl, auth)
                return Result.failure(AlreadySignedInException())
            }
            // The previous session's teardown finishes before a new one starts.
            if (running == null) sessionTeardown.awaitIdle()
            var replacedAccess: String? = null
            val installed = synchronized(pendingLock) {
                val unchanged = loginAttempt == start.attempt &&
                    tokenStorage.getServerUrl() == serverUrl &&
                    authState.session.value == start.session
                if (unchanged) {
                    if (running != null) replacedAccess = tokenStorage.getAccessToken()
                    tokenStorage.saveTokens(auth.accessToken, auth.refreshToken, auth.expiresIn, user.id)
                    authState.setLoggedIn(user.id)
                }
                unchanged
            }
            if (!installed) {
                endUnused(serverUrl, auth)
                return Result.failure(IllegalStateException("Device sign-in was cancelled or superseded"))
            }
            // The new session is installed: ending the replaced one runs to the end even if the
            // caller is cancelled now, and never turns the installed sign-in into a failure.
            replacedAccess?.let { old ->
                withContext(NonCancellable) {
                    try {
                        withTimeoutOrNull(SERVER_LOGOUT_TIMEOUT_MS) { authApi.logoutSession(serverUrl, old, null) }
                    } catch (e: Exception) {
                        logger.warning("Old session not ended (best-effort): ${e.javaClass.simpleName}")
                    }
                }
            }
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    fun isLoggedIn(): Boolean {
        return authState.isLoggedIn.value
    }
}

/**
 * What a device-key sign-in started from ([AuthRepository.beginDeviceSignIn]):
 * the sign-in generation, the server it talks to and the session that ran.
 */
class DeviceSignInStart internal constructor(
    internal val attempt: Long,
    val serverUrl: String,
    internal val session: AuthSession,
) {
    override fun toString(): String = "DeviceSignInStart(serverUrl=$serverUrl)"
}

/** A sign-in was refused because a session is running; sign out first. */
class AlreadySignedInException : IllegalStateException("A session is running; sign out first")
