package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.MfaRequiredApiException
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.session.SessionTeardownRunner
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.ui.auth.MfaRequiredException
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
     * Set by [login] when the server requires a password change; see
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
     * another account's QR code from Settings, or a stray OIDC deep link):
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
     * and sets LoggedIn. Two sign-ins racing while logged out (a QR redeem
     * and an OIDC deep link) can therefore never both install. Returns
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
     * Logs in. If the server reports `password_change_required`, nothing is
     * persisted and [getMe][AuthApi.authenticatedGetMe] is skipped: the session
     * is kept in memory and [PasswordChangeRequiredException] is returned, and
     * the typed [password] later becomes `current_password` in
     * [completePasswordChange].
     */
    suspend fun login(
        username: String,
        password: String,
        mfaCode: String? = null
    ): Result<User> {
        signedInRefusal()?.let { return it }
        val attempt = invalidateLogin()
        // The server this login goes to; a pending change is bound to it.
        val serverUrl = tokenStorage.getServerUrl()
        return try {
            val authResponse = authApi.login(username, password, mfaCode)

            if (authResponse.passwordChangeRequired) {
                if (serverUrl == null) return supersededResult()
                val pending = PendingPasswordChange(
                    serverUrl = serverUrl,
                    accessToken = authResponse.accessToken,
                    refreshToken = authResponse.refreshToken,
                    expiresIn = authResponse.expiresIn,
                    currentPassword = password
                )
                val installed = synchronized(pendingLock) {
                    if (loginAttempt == attempt && tokenStorage.getServerUrl() == serverUrl) {
                        pendingPasswordChange = pending
                        true
                    } else {
                        false
                    }
                }
                if (!installed) return supersededResult()
                return Result.failure(PasswordChangeRequiredException())
            }

            // Server switched while the login was in flight: don't send server A's
            // token anywhere else, and leave the new server's state alone.
            if (tokenStorage.getServerUrl() != serverUrl) return supersededResult()
            val user = authApi.authenticatedGetMe(authResponse.accessToken, serverUrl)

            // The previous session's teardown finishes before this one starts.
            sessionTeardown.awaitIdle()

            // Persist only for the latest attempt and the server it went to: a
            // server change mid-login must not store server A's tokens under B.
            val persisted = installSession(
                authResponse.accessToken, authResponse.refreshToken, authResponse.expiresIn, user.id,
                precondition = { loginAttempt == attempt && tokenStorage.getServerUrl() == serverUrl },
            )
            if (!persisted) return supersededResult()
            Result.success(user)
        } catch (_: MfaRequiredApiException) {
            Result.failure(MfaRequiredException())
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Only the latest attempt may clear: a stale login failing late must
            // not wipe the session a newer login stored meanwhile.
            synchronized(pendingLock) {
                if (loginAttempt == attempt && tokenStorage.getServerUrl() == serverUrl) tokenStorage.clear()
            }
            Result.failure(e)
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

    suspend fun register(
        username: String,
        password: String,
        email: String?,
        displayName: String?
    ): Result<User> {
        signedInRefusal()?.let { return it }
        // Fetch the profile from the server that issued the token.
        val serverUrl = tokenStorage.getServerUrl()
        return try {
            val authResponse = authApi.register(username, password, email, displayName)

            val user = authApi.authenticatedGetMe(authResponse.accessToken, serverUrl)

            // The previous session's teardown finishes before this one starts.
            sessionTeardown.awaitIdle()
            if (!installSession(authResponse.accessToken, authResponse.refreshToken, authResponse.expiresIn, user.id)) {
                return Result.failure(AlreadySignedInException())
            }
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tokenStorage.clear()
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
        unregisterPush(session?.sessionId)

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

    /**
     * Completes the OIDC login by storing the tokens received via deep link redirect.
     *
     * In the mobile OIDC flow, the server redirects to `kaiku://auth/callback` with
     * tokens in query parameters (access_token, refresh_token, expires_in).
     * This method saves those tokens and fetches the user profile.
     */
    suspend fun completeOidcLogin(
        accessToken: String,
        refreshToken: String?,
        expiresIn: Int
    ): Result<User> {
        signedInRefusal()?.let { return it }
        return try {
            val user = authApi.authenticatedGetMe(accessToken)

            // The previous session's teardown finishes before this one starts.
            sessionTeardown.awaitIdle()
            if (!installSession(accessToken, refreshToken, expiresIn, user.id)) {
                return Result.failure(AlreadySignedInException())
            }
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tokenStorage.clear()
            Result.failure(e)
        }
    }

    /**
     * Exchanges an OIDC authorization code for tokens via the server's callback endpoint.
     *
     * This is a fallback path if the server does not redirect with tokens directly.
     */
    suspend fun exchangeOidcCode(code: String, state: String, codeVerifier: String? = null): Result<User> {
        signedInRefusal()?.let { return it }
        // Fetch the profile from the server that issued the token.
        val serverUrl = tokenStorage.getServerUrl()
        return try {
            val redirectUri = "kaiku://auth/callback"
            val authResponse = authApi.exchangeOidcCode(code, state, redirectUri, codeVerifier)

            val user = authApi.authenticatedGetMe(authResponse.accessToken, serverUrl)

            // The previous session's teardown finishes before this one starts.
            sessionTeardown.awaitIdle()
            if (!installSession(authResponse.accessToken, authResponse.refreshToken, authResponse.expiresIn, user.id)) {
                return Result.failure(AlreadySignedInException())
            }
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tokenStorage.clear()
            Result.failure(e)
        }
    }

    /**
     * Redeems a QR login token scanned from the desktop client.
     *
     * Exchanges the token for auth credentials via an absolute URL, then
     * saves the server URL (needed by KaikuHttpClient for base URL) before
     * calling getMe. Tokens are only persisted after getMe succeeds.
     */
    suspend fun redeemQrToken(serverUrl: String, token: String): Result<User> {
        signedInRefusal()?.let { return it }
        return try {
            val authResponse = authApi.redeemQrToken(serverUrl, token)

            // Save server URL before getMe — KaikuHttpClient needs it for the base URL
            tokenStorage.saveServerUrl(serverUrl)

            val user = authApi.authenticatedGetMe(authResponse.accessToken, serverUrl)

            // The previous session's teardown finishes before this one starts.
            sessionTeardown.awaitIdle()
            if (!installSession(authResponse.accessToken, authResponse.refreshToken, authResponse.expiresIn, user.id)) {
                return Result.failure(AlreadySignedInException())
            }
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tokenStorage.clear()
            Result.failure(e)
        }
    }

    fun isLoggedIn(): Boolean {
        return authState.isLoggedIn.value
    }
}

/** A sign-in was refused because a session is running; sign out first. */
class AlreadySignedInException : IllegalStateException("A session is running; sign out first")
