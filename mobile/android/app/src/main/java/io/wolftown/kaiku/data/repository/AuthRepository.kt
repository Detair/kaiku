package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.MfaRequiredApiException
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.ui.auth.MfaRequiredException
import io.wolftown.kaiku.ui.auth.PasswordChangeRequiredException
import java.util.logging.Level
import java.util.logging.Logger
import kotlin.coroutines.cancellation.CancellationException
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
    private val authState: AuthState
) {
    companion object {
        private val logger = Logger.getLogger("AuthRepository")
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

            val user = authApi.authenticatedGetMe(authResponse.accessToken)

            val persisted = synchronized(pendingLock) {
                if (loginAttempt != attempt) {
                    false
                } else {
                    tokenStorage.saveTokens(
                        accessToken = authResponse.accessToken,
                        refreshToken = authResponse.refreshToken,
                        expiresIn = authResponse.expiresIn,
                        userId = user.id
                    )
                    authState.setLoggedIn(user.id)
                    true
                }
            }
            if (!persisted) return supersededResult()
            Result.success(user)
        } catch (_: MfaRequiredApiException) {
            Result.failure(MfaRequiredException())
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tokenStorage.clear()
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

            // Check and persist under the lock: a cancel can't slip in between.
            val persisted = synchronized(pendingLock) {
                if (pendingPasswordChange !== changed || !isSameServer(pending)) {
                    false
                } else {
                    tokenStorage.saveTokens(
                        accessToken = pending.accessToken,
                        refreshToken = pending.refreshToken,
                        expiresIn = pending.expiresIn,
                        userId = user.id
                    )
                    pendingPasswordChange = null
                    authState.setLoggedIn(user.id)
                    true
                }
            }
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
        return try {
            val authResponse = authApi.register(username, password, email, displayName)

            val user = authApi.authenticatedGetMe(authResponse.accessToken)

            tokenStorage.saveTokens(
                accessToken = authResponse.accessToken,
                refreshToken = authResponse.refreshToken,
                expiresIn = authResponse.expiresIn,
                userId = user.id
            )

            authState.setLoggedIn(user.id)
            Result.success(user)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tokenStorage.clear()
            Result.failure(e)
        }
    }

    suspend fun logout() {
        invalidateLogin()
        try {
            authApi.logout()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Server-side logout failed (best-effort)", e)
        }
        tokenStorage.clear()
        authState.setLoggedOut()
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
        return try {
            val user = authApi.authenticatedGetMe(accessToken)

            tokenStorage.saveTokens(
                accessToken = accessToken,
                refreshToken = refreshToken,
                expiresIn = expiresIn,
                userId = user.id
            )

            authState.setLoggedIn(user.id)
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
        return try {
            val redirectUri = "kaiku://auth/callback"
            val authResponse = authApi.exchangeOidcCode(code, state, redirectUri, codeVerifier)

            val user = authApi.authenticatedGetMe(authResponse.accessToken)

            tokenStorage.saveTokens(
                accessToken = authResponse.accessToken,
                refreshToken = authResponse.refreshToken,
                expiresIn = authResponse.expiresIn,
                userId = user.id
            )

            authState.setLoggedIn(user.id)
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
        return try {
            val authResponse = authApi.redeemQrToken(serverUrl, token)

            // Save server URL before getMe — KaikuHttpClient needs it for the base URL
            tokenStorage.saveServerUrl(serverUrl)

            val user = authApi.authenticatedGetMe(authResponse.accessToken)

            tokenStorage.saveTokens(
                accessToken = authResponse.accessToken,
                refreshToken = authResponse.refreshToken,
                expiresIn = authResponse.expiresIn,
                userId = user.id
            )

            authState.setLoggedIn(user.id)
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
