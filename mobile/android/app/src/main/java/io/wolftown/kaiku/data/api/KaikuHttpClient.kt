package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.engine.*
import io.ktor.client.engine.okhttp.*
import io.ktor.client.plugins.*
import io.ktor.client.plugins.contentnegotiation.*
import io.ktor.client.request.*
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

            // Attach Bearer token if available
            tokenStorage.getAccessToken()?.let { token ->
                request.headers[HttpHeaders.Authorization] = "Bearer $token"
            }

            val originalCall = execute(request)

            if (originalCall.response.status != HttpStatusCode.Unauthorized) {
                return@intercept originalCall
            }

            // 401 received -- attempt token refresh (skipped if another caller
            // already rotated the token this request was sent with)
            val tokenUsedInRequest =
                request.headers[HttpHeaders.Authorization]?.removePrefix("Bearer ")

            val refreshResult = refreshMutex.withLock {
                val currentToken = tokenStorage.getAccessToken()
                if (currentToken != null && currentToken != tokenUsedInRequest) {
                    RefreshResult.SUCCESS
                } else {
                    refreshLocked()
                }
            }

            if (refreshResult != RefreshResult.SUCCESS) {
                return@intercept originalCall
            }

            // Retry original request with new token
            val newToken = tokenStorage.getAccessToken()
            request.headers[HttpHeaders.Authorization] = "Bearer $newToken"
            execute(request)
        }

        return client
    }

    override suspend fun ensureFresh(minValidityMs: Long): RefreshResult {
        if (isValidFor(minValidityMs)) return RefreshResult.SUCCESS
        return refreshMutex.withLock {
            // Double-check: another caller may have refreshed while we waited
            if (isValidFor(minValidityMs)) RefreshResult.SUCCESS else refreshLocked()
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
                refreshLocked()
            }
        }
    }

    private fun isValidFor(minValidityMs: Long): Boolean {
        if (tokenStorage.getAccessToken() == null) return false
        val remaining = tokenStorage.getAccessTokenExpiresAtMillis() - System.currentTimeMillis()
        return remaining >= minValidityMs
    }

    /**
     * Refreshes the tokens. Must be called while holding [refreshMutex].
     *
     * Ends the session (logout + clear stored tokens) on
     * [RefreshResult.AUTH_REJECTED] and [RefreshResult.NO_SESSION].
     */
    private suspend fun refreshLocked(): RefreshResult {
        val refreshToken = tokenStorage.getRefreshToken()
        val result = if (refreshToken == null) {
            RefreshResult.NO_SESSION
        } else {
            performRefresh(refreshToken)
        }
        if (result == RefreshResult.AUTH_REJECTED || result == RefreshResult.NO_SESSION) {
            // Clear first so no LoggedOut observer can read live tokens.
            tokenStorage.clear()
            authState.setLoggedOut()
        }
        return result
    }

    /**
     * Performs the token refresh request through the configured client.
     *
     * Returns [RefreshResult.SUCCESS] when tokens were saved,
     * [RefreshResult.AUTH_REJECTED] on 401/403 (invalid refresh token), or
     * [RefreshResult.NETWORK_ERROR] for transient failures (I/O, 5xx, other
     * non-success statuses including 400) that should not log the user out.
     *
     * If the stored refresh token changed while the request was in flight
     * (logout cleared it, or a new login replaced it), the response is
     * discarded so a logged-out session is never resurrected.
     */
    private suspend fun performRefresh(refreshToken: String): RefreshResult {
        return try {
            val response = activeClient().post("/auth/refresh") {
                // Skip the auth interceptor: no Bearer header, no 401 recursion
                attributes.put(SkipAuthInterceptor, true)
                contentType(ContentType.Application.Json)
                setBody(RefreshRequest(refreshToken))
            }
            val status = response.status

            if (status == HttpStatusCode.Unauthorized || status == HttpStatusCode.Forbidden) {
                return RefreshResult.AUTH_REJECTED
            }

            if (!status.isSuccess()) {
                logger.warning("Token refresh failed with HTTP ${status.value}")
                return RefreshResult.NETWORK_ERROR
            }

            val authResponse = response.body<AuthResponse>()

            // Re-check: logout (clear) or a new login may have happened while
            // the request was in flight. Never overwrite either.
            when (tokenStorage.getRefreshToken()) {
                null -> return RefreshResult.NO_SESSION
                refreshToken -> Unit
                else -> return RefreshResult.SUCCESS
            }

            // Use existing userId since the refresh response does not include it
            val userId = tokenStorage.getUserId() ?: return RefreshResult.NETWORK_ERROR

            tokenStorage.saveTokens(
                accessToken = authResponse.accessToken,
                refreshToken = authResponse.refreshToken ?: refreshToken,
                expiresIn = authResponse.expiresIn,
                userId = userId
            )
            RefreshResult.SUCCESS
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (e: Exception) {
            // Log the exception type only: decoding errors can echo the response
            // body, which contains tokens.
            logger.warning("Token refresh failed: ${e.javaClass.simpleName}")
            RefreshResult.NETWORK_ERROR
        }
    }
}
