package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.engine.*
import io.ktor.client.engine.okhttp.*
import io.ktor.client.plugins.*
import io.ktor.client.plugins.contentnegotiation.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.ktor.http.content.*
import io.ktor.serialization.kotlinx.json.*
import io.ktor.util.*
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.AuthResponse
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import okhttp3.ConnectionSpec
import java.util.logging.Level
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
 */
class KaikuHttpClient @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val authState: AuthState,
    private val tls13Spec: ConnectionSpec
) {
    private enum class RefreshResult { SUCCESS, AUTH_REJECTED, NETWORK_ERROR }

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

            // 401 received -- attempt token refresh
            val refreshToken = tokenStorage.getRefreshToken()
            if (refreshToken == null) {
                authState.setLoggedOut()
                return@intercept originalCall
            }

            val tokenUsedInRequest =
                request.headers[HttpHeaders.Authorization]?.removePrefix("Bearer ")

            val refreshResult = refreshMutex.withLock {
                // Double-check: another coroutine may have already refreshed
                val currentToken = tokenStorage.getAccessToken()
                if (currentToken != null && currentToken != tokenUsedInRequest) {
                    // Token was already refreshed by another coroutine
                    RefreshResult.SUCCESS
                } else {
                    performRefresh(refreshToken)
                }
            }

            if (refreshResult != RefreshResult.SUCCESS) {
                if (refreshResult == RefreshResult.AUTH_REJECTED) {
                    authState.setLoggedOut()
                }
                return@intercept originalCall
            }

            // Retry original request with new token
            val newToken = tokenStorage.getAccessToken()
            request.headers[HttpHeaders.Authorization] = "Bearer $newToken"
            execute(request)
        }

        return client
    }

    /**
     * Performs the token refresh request.
     *
     * Returns [RefreshResult.SUCCESS] when tokens were saved,
     * [RefreshResult.AUTH_REJECTED] on 401/403 (invalid refresh token), or
     * [RefreshResult.NETWORK_ERROR] for transient failures that should not log the user out.
     */
    private suspend fun Sender.performRefresh(refreshToken: String): RefreshResult {
        return try {
            val body = KaikuJson.encodeToString(RefreshRequest(refreshToken))
            val refreshRequest = HttpRequestBuilder().apply {
                method = HttpMethod.Post
                url.encodedPath = "/auth/refresh"
                contentType(ContentType.Application.Json)
                setBody(TextContent(body, ContentType.Application.Json))
                // Mark this request to skip the auth interceptor
                attributes.put(SkipAuthInterceptor, true)
            }
            val refreshCall = execute(refreshRequest)
            val status = refreshCall.response.status

            if (status == HttpStatusCode.Unauthorized || status == HttpStatusCode.Forbidden) {
                return RefreshResult.AUTH_REJECTED
            }

            if (!status.isSuccess()) {
                return RefreshResult.NETWORK_ERROR
            }

            val authResponse = refreshCall.response.body<AuthResponse>()

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
            logger.log(Level.WARNING, "Token refresh failed", e)
            RefreshResult.NETWORK_ERROR
        }
    }
}
