package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.MfaRequiredApiException
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.ui.auth.MfaRequiredException
import java.util.logging.Level
import java.util.logging.Logger
import kotlin.coroutines.cancellation.CancellationException
import javax.inject.Inject

class AuthRepository @Inject constructor(
    private val authApi: AuthApi,
    private val tokenStorage: TokenStorage,
    private val authState: AuthState
) {
    companion object {
        private val logger = Logger.getLogger("AuthRepository")
    }

    suspend fun login(
        username: String,
        password: String,
        mfaCode: String? = null
    ): Result<User> {
        return try {
            val authResponse = authApi.login(username, password, mfaCode)

            val user = authApi.authenticatedGetMe(authResponse.accessToken)

            tokenStorage.saveTokens(
                accessToken = authResponse.accessToken,
                refreshToken = authResponse.refreshToken,
                expiresIn = authResponse.expiresIn,
                userId = user.id
            )

            authState.setLoggedIn(user.id)
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
