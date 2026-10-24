package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.engine.*
import io.ktor.client.engine.okhttp.*
import io.ktor.client.plugins.*
import io.ktor.client.plugins.contentnegotiation.*
import io.ktor.client.request.*
import io.ktor.client.statement.*
import io.ktor.http.*
import io.ktor.serialization.kotlinx.json.*
import io.ktor.util.*
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.AuthResponse
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.Serializable
import okhttp3.ConnectionSpec
import java.util.logging.Logger
import javax.inject.Inject

/** Request body for the token refresh endpoint. */
@Serializable
internal data class RefreshRequest(val refreshToken: String)

/**
 * Configured Ktor [HttpClient] wrapper for all Kaiku API calls.
 *
 * Provides:
 * - Content negotiation with [KaikuJson] (snake_case)
 * - Base URL from [TokenStorage.getServerUrl]
 * - Automatic Bearer token injection
 * - Transparent 401 -> refresh -> retry flow
 * - Mutex-guarded refresh to prevent concurrent refresh storms
 * - [TokenRefresher] for proactive refreshes (shares the same mutex)
 * - C4 fallback: a 403 `PASSWORD_CHANGE_REQUIRED` (or a refresh response with
 *   `password_change_required`) ends the stored session, so the user signs in
 *   again and gets the change-password screen
 *
 * The refresh request itself is issued through [activeClient] (with
 * [SkipAuthInterceptor] set), so it uses the configured server URL and JSON
 * content type from `defaultRequest`.
 */
class KaikuHttpClient @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val authState: AuthState,
    private val tls13Spec: ConnectionSpec
) : TokenRefresher {

    internal companion object {
        private val logger = Logger.getLogger("KaikuHttpClient")

        /** C4 error code for a session that must change its password first. */
        internal const val PASSWORD_CHANGE_REQUIRED = "PASSWORD_CHANGE_REQUIRED"

        /** Attribute key to mark requests that should skip the auth interceptor. */
        private val SkipAuthInterceptor = AttributeKey<Boolean>("SkipAuthInterceptor")

        /**
         * Creates a [KaikuHttpClient] with a custom engine (for testing with MockEngine).
         */
        fun forTesting(
            tokenStorage: TokenStorage,
            authState: AuthState,
            engine: HttpClientEngine
        ): KaikuHttpClient {
            // ConnectionSpec is unused for MockEngine; provide MODERN_TLS as placeholder.
            return KaikuHttpClient(tokenStorage, authState, ConnectionSpec.MODERN_TLS).apply {
                testClient = createConfiguredClient(engine)
            }
        }
    }

    private val refreshMutex = Mutex()

    val httpClient: HttpClient = createConfiguredClient(OkHttp.create {
        config {
            connectionSpecs(listOf(tls13Spec))
        }
    })

    @Volatile
    private var testClient: HttpClient? = null

    /** Returns the active [HttpClient] (test override or production OkHttp). */
    internal fun activeClient(): HttpClient = testClient ?: httpClient

    private fun createConfiguredClient(engine: HttpClientEngine): HttpClient {
        val client = HttpClient(engine) {
            install(ContentNegotiation) {
                json(KaikuJson)
            }

            defaultRequest {
                val serverUrl = tokenStorage.getServerUrl()
                if (serverUrl != null) {
                    url(serverUrl)
                }
                contentType(ContentType.Application.Json)
            }
        }

        client.plugin(HttpSend).intercept { request ->
            // Skip auth logic for internal requests (e.g. token refresh)
            if (request.attributes.getOrNull(SkipAuthInterceptor) == true) {
                return@intercept execute(request)
            }

            // Capture the session this request belongs to. Read the generation
            // before the token: a login landing in between makes the generation
            // look stale (safe: no retry), never the other way round.
            val generationAtRequest = tokenStorage.getSessionGeneration()
            val serverUrlAtRequest = tokenStorage.getServerUrl()

            // Attach Bearer token if available
            tokenStorage.getAccessToken()?.let { token ->
                request.headers[HttpHeaders.Authorization] = "Bearer $token"
            }

            val originalCall = execute(request)

            if (originalCall.response.status == HttpStatusCode.Forbidden &&
                isPasswordChangeRequired(originalCall.response)
            ) {
                endSessionForPasswordChange(generationAtRequest, serverUrlAtRequest)
                return@intercept originalCall
            }

            if (originalCall.response.status != HttpStatusCode.Unauthorized) {
                return@intercept originalCall
            }

            val tokenUsedInRequest =
                request.headers[HttpHeaders.Authorization]?.removePrefix("Bearer ")

            val retry = refreshMutex.withLock {
                if (!isSameSession(generationAtRequest, serverUrlAtRequest)) {
                    // Logged out, logged in again or server changed since this
                    // request was sent: never replay it with another session's token.
                    false
                } else {
                    val currentToken = tokenStorage.getAccessToken()
                    if (currentToken != null && currentToken != tokenUsedInRequest) {
                        // Same session, token already rotated by another caller
                        true
                    } else {
                        refreshLocked() == Outcome.ROTATED
                    }
                }
            }

            if (!retry) {
                return@intercept originalCall
            }

            // Retry original request with the rotated token, but only if the
            // session is still the one the request started from (the retry
            // reuses the original, already resolved URL).
            val newToken = tokenStorage.getAccessToken()
            if (newToken == null || !isSameSession(generationAtRequest, serverUrlAtRequest)) {
                return@intercept originalCall
            }
            request.headers[HttpHeaders.Authorization] = "Bearer $newToken"
            execute(request)
        }

        return client
    }

    override suspend fun ensureFresh(minValidityMs: Long): RefreshResult {
        if (isValidFor(minValidityMs)) return RefreshResult.SUCCESS
        return refreshMutex.withLock {
            // Double-check: another caller may have refreshed while we waited
            if (isValidFor(minValidityMs)) RefreshResult.SUCCESS else refreshLocked().result
        }
    }

    override suspend fun forceRefresh(): RefreshResult {
        val tokenBefore = tokenStorage.getAccessToken()
        return refreshMutex.withLock {
            // Double-check: another caller may have rotated the token while we waited
            val currentToken = tokenStorage.getAccessToken()
            if (currentToken != null && currentToken != tokenBefore) {
                RefreshResult.SUCCESS
            } else {
                refreshLocked().result
            }
        }
    }

    /**
     * Internal refresh outcome. Distinguishes a rotation of the session the
     * refresh started from ([ROTATED], the only outcome that allows replaying a
     * request) from finding a different session ([SESSION_CHANGED]) or a session
     * ended by someone else ([SESSION_ENDED]).
     */
    private enum class Outcome(val result: RefreshResult) {
        ROTATED(RefreshResult.SUCCESS),
        SESSION_CHANGED(RefreshResult.SUCCESS),
        SESSION_ENDED(RefreshResult.NO_SESSION),
        AUTH_REJECTED(RefreshResult.AUTH_REJECTED),
        NETWORK_ERROR(RefreshResult.NETWORK_ERROR),
        NO_SESSION(RefreshResult.NO_SESSION),
    }

    /**
     * True if a 403 body carries the C4 code `PASSWORD_CHANGE_REQUIRED`. Ktor 3
     * keeps the response body in memory, so the caller can still read it.
     */
    private suspend fun isPasswordChangeRequired(response: HttpResponse): Boolean =
        try {
            KaikuJson.decodeFromString<ApiErrorResponse>(response.bodyAsText()).error ==
                PASSWORD_CHANGE_REQUIRED
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (_: Exception) {
            false
        }

    /**
     * The server says the stored session can't be used until the password is
     * changed (it changed server-side after login). Clear it and log out; the
     * user signs in again and is sent to the change-password screen. Nothing is
     * done when no session is stored (the in-memory pending-change flow) or the
     * request belonged to an older session.
     */
    private suspend fun endSessionForPasswordChange(generation: Long, serverUrl: String?) {
        refreshMutex.withLock {
            if (!isSameSession(generation, serverUrl)) return@withLock
            if (tokenStorage.getAccessToken() == null && tokenStorage.getRefreshToken() == null) {
                return@withLock
            }
            logger.warning("Server requires a password change, ending the session")
            tokenStorage.clear()
            authState.setLoggedOut()
        }
    }

    private fun isSameSession(generation: Long, serverUrl: String?): Boolean =
        tokenStorage.getSessionGeneration() == generation &&
            tokenStorage.getServerUrl() == serverUrl

    private fun isValidFor(minValidityMs: Long): Boolean {
        if (tokenStorage.getAccessToken() == null) return false
        val remaining = tokenStorage.getAccessTokenExpiresAtMillis() - System.currentTimeMillis()
        return remaining >= minValidityMs
    }

    /**
     * Classifies storage after the refresh token turned out to differ from the
     * one a refresh started with: empty means someone else ended the session,
     * anything else is a newer session that must be left untouched.
     */
    private fun changedSession(): Outcome =
        if (tokenStorage.getRefreshToken() == null && tokenStorage.getAccessToken() == null) {
            Outcome.SESSION_ENDED
        } else {
            Outcome.SESSION_CHANGED
        }

    /**
     * Ends the session the refresh started from, but only if it is still the
     * stored one (atomic in [TokenStorage]). Clears before logging out so no
     * LoggedOut observer can read live tokens.
     */
    private fun endSessionIfStill(sentRefreshToken: String?, onEnded: Outcome): Outcome {
        if (!tokenStorage.clearIfRefreshTokenIs(sentRefreshToken)) return changedSession()
        authState.setLoggedOut()
        return onEnded
    }

    /**
     * Refreshes the tokens. Must be called while holding [refreshMutex].
     *
     * Ends the session (clear stored tokens, then logout) on
     * [Outcome.AUTH_REJECTED] and [Outcome.NO_SESSION], only if the stored
     * refresh token is still the one the refresh started from.
     */
    private suspend fun refreshLocked(): Outcome {
        val refreshToken = tokenStorage.getRefreshToken()
            ?: return endSessionIfStill(null, Outcome.NO_SESSION)
        return performRefresh(refreshToken)
    }

    /**
     * Performs the token refresh request through the configured client.
     *
     * Status mapping: 2xx → tokens saved atomically if the stored refresh token
     * is still [refreshToken] ([Outcome.ROTATED]); 401/403 → session ended if
     * still current ([Outcome.AUTH_REJECTED]); 400, 5xx, other non-2xx and
     * exceptions → [Outcome.NETWORK_ERROR] (session kept). If the stored refresh
     * token changed while the request was in flight (logout or new login), the
     * response is discarded and nothing is written or cleared.
     */
    private suspend fun performRefresh(refreshToken: String): Outcome {
        return try {
            val response = activeClient().post("/auth/refresh") {
                // Skip the auth interceptor: no Bearer header, no 401 recursion
                attributes.put(SkipAuthInterceptor, true)
                contentType(ContentType.Application.Json)
                setBody(RefreshRequest(refreshToken))
            }
            val status = response.status

            if (status == HttpStatusCode.Unauthorized || status == HttpStatusCode.Forbidden) {
                // Only the session that sent this refresh token may be ended.
                return endSessionIfStill(refreshToken, Outcome.AUTH_REJECTED)
            }

            if (!status.isSuccess()) {
                logger.warning("Token refresh failed with HTTP ${status.value}")
                return Outcome.NETWORK_ERROR
            }

            val authResponse = response.body<AuthResponse>()

            if (authResponse.passwordChangeRequired) {
                // C4: the session can't be used until the password is changed.
                // Signing in again leads to the change-password screen.
                logger.warning("Token refresh reports a pending password change, ending the session")
                return endSessionIfStill(refreshToken, Outcome.AUTH_REJECTED)
            }

            // Use existing userId since the refresh response does not include it
            val userId = tokenStorage.getUserId()
                ?: return if (tokenStorage.getRefreshToken() == refreshToken) {
                    Outcome.NETWORK_ERROR
                } else {
                    changedSession()
                }

            val saved = tokenStorage.saveTokensIfRefreshTokenIs(
                expectedRefreshToken = refreshToken,
                accessToken = authResponse.accessToken,
                refreshToken = authResponse.refreshToken ?: refreshToken,
                expiresIn = authResponse.expiresIn,
                userId = userId
            )
            if (saved) Outcome.ROTATED else changedSession()
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (e: Exception) {
            // Log the exception type only: decoding errors can echo the response
            // body, which contains tokens.
            logger.warning("Token refresh failed: ${e.javaClass.simpleName}")
            Outcome.NETWORK_ERROR
        }
    }
}
