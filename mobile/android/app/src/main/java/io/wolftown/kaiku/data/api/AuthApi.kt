package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.User
import kotlinx.serialization.Serializable
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject

@Serializable
data class OidcProvider(
    val slug: String,
    val displayName: String,
    val iconHint: String? = null
)

interface AuthApi {
    suspend fun login(username: String, password: String, mfaCode: String? = null): AuthResponse
    suspend fun register(
        username: String,
        password: String,
        email: String? = null,
        displayName: String? = null
    ): AuthResponse
    suspend fun refresh(refreshToken: String): AuthResponse
    /**
     * `POST /auth/logout` on [serverUrl] (absolute) for one captured session,
     * using only the tokens passed in: the auth interceptor is skipped, so a
     * stored token (possibly another session's) is never attached, and
     * nothing is refreshed. Attempts, until one succeeds:
     * 1. bearer [accessToken] plus [refreshToken] in the body;
     * 2. bearer only (C25), e.g. when the body token was rotated away;
     * 3. [refreshToken] in the body without a bearer, e.g. when the captured
     *    access token has expired (if the server accepts it).
     * Without a [refreshToken] only the bearer request is sent. Best effort:
     * never throws on a non-2xx status; logs status codes, never tokens.
     */
    suspend fun logoutSession(serverUrl: String, accessToken: String?, refreshToken: String?)

    suspend fun getMe(): User
    /**
     * Calls /auth/me with an explicit bearer token. The request skips the auth
     * interceptor, so a stored token can't replace it. With [serverUrl] the call
     * goes to that origin instead of the configured server.
     */
    suspend fun authenticatedGetMe(accessToken: String, serverUrl: String? = null): User
    /**
     * C4: `POST /auth/me/password` on [serverUrl] (absolute) with an explicit
     * bearer token that the auth interceptor never replaces (the session is not
     * persisted while a forced change is pending). Throws [ApiException] on a
     * non-2xx response; its [ApiException.code] carries the server error code.
     */
    suspend fun changePassword(
        serverUrl: String,
        accessToken: String,
        currentPassword: String,
        newPassword: String
    )
    suspend fun getOidcProviders(): List<OidcProvider>
    suspend fun exchangeOidcCode(code: String, state: String, redirectUri: String, codeVerifier: String? = null): AuthResponse
    suspend fun redeemQrToken(serverUrl: String, token: String): AuthResponse
}

@Serializable
private data class LoginRequest(
    val username: String,
    val password: String,
    val mfaCode: String? = null
)

@Serializable
private data class RegisterRequest(
    val username: String,
    val password: String,
    val email: String? = null,
    val displayName: String? = null
)

@Serializable
private data class RefreshTokenRequest(
    val refreshToken: String
) {
    override fun toString(): String = "RefreshTokenRequest"
}

@Serializable
private data class OidcCallbackRequest(
    val code: String,
    val state: String,
    val redirectUri: String,
    val codeVerifier: String? = null
)

@Serializable
private data class ChangePasswordRequest(
    val currentPassword: String,
    val newPassword: String,
    val revokeOthers: Boolean
) {
    override fun toString(): String = "ChangePasswordRequest(revokeOthers=$revokeOthers)"
}

@Serializable
private data class QrRedeemRequest(
    val token: String
)

class AuthApiImpl @Inject constructor(
    private val httpClient: HttpClient
) : AuthApi {
    companion object {
        private val logger = Logger.getLogger("AuthApi")
    }

    override suspend fun login(
        username: String,
        password: String,
        mfaCode: String?
    ): AuthResponse {
        val response = httpClient.post("/auth/login") {
            setBody(LoginRequest(username, password, mfaCode))
        }

        if (response.status == HttpStatusCode.Forbidden) {
            val errorBody = response.body<ApiErrorResponse>()
            if (errorBody.error == "MFA_REQUIRED") {
                throw MfaRequiredApiException(errorBody.message ?: "MFA required")
            }
            throw ApiException(response.status, errorBody.message ?: "Forbidden", code = errorBody.error)
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Login failed", code = errorBody?.error)
        }

        return response.body()
    }

    override suspend fun register(
        username: String,
        password: String,
        email: String?,
        displayName: String?
    ): AuthResponse {
        val response = httpClient.post("/auth/register") {
            setBody(RegisterRequest(username, password, email, displayName))
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Registration failed")
        }

        return response.body()
    }

    override suspend fun refresh(refreshToken: String): AuthResponse {
        val response = httpClient.post("/auth/refresh") {
            setBody(RefreshTokenRequest(refreshToken))
        }

        if (!response.status.isSuccess()) {
            throw ApiException(response.status, "Token refresh failed")
        }

        return response.body()
    }

    override suspend fun logoutSession(serverUrl: String, accessToken: String?, refreshToken: String?) {
        val url = serverUrl.trimEnd('/') + "/auth/logout"
        val attempts = buildList {
            if (accessToken != null) add(accessToken to refreshToken)
            if (accessToken != null && refreshToken != null) add(accessToken to null)
            if (refreshToken != null) add(null to refreshToken)
        }
        for ((index, attempt) in attempts.withIndex()) {
            val (bearer, bodyToken) = attempt
            val response = httpClient.post(url) {
                attributes.put(KaikuHttpClient.SkipAuthInterceptor, true)
                if (bearer != null) {
                    headers { set(HttpHeaders.Authorization, "Bearer $bearer") }
                }
                if (bodyToken != null) setBody(RefreshTokenRequest(bodyToken))
            }
            if (response.status.isSuccess()) return
            logger.log(Level.WARNING, "Logout attempt ${index + 1} failed: ${response.status.value}")
        }
    }

    override suspend fun getMe(): User {
        val response = httpClient.get("/auth/me")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to get user")
        }

        return response.body()
    }

    override suspend fun authenticatedGetMe(accessToken: String, serverUrl: String?): User {
        val url = serverUrl?.let { it.trimEnd('/') + "/auth/me" } ?: "/auth/me"
        val response = httpClient.get(url) {
            attributes.put(KaikuHttpClient.SkipAuthInterceptor, true)
            headers {
                set(HttpHeaders.Authorization, "Bearer $accessToken")
            }
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to get user")
        }

        return response.body()
    }

    override suspend fun changePassword(
        serverUrl: String,
        accessToken: String,
        currentPassword: String,
        newPassword: String
    ) {
        val response = httpClient.post(serverUrl.trimEnd('/') + "/auth/me/password") {
            attributes.put(KaikuHttpClient.SkipAuthInterceptor, true)
            headers {
                set(HttpHeaders.Authorization, "Bearer $accessToken")
            }
            setBody(ChangePasswordRequest(currentPassword, newPassword, revokeOthers = false))
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Password change failed",
                code = errorBody?.error
            )
        }
    }

    override suspend fun getOidcProviders(): List<OidcProvider> {
        val response = httpClient.get("/auth/oidc/providers")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to load OIDC providers")
        }

        return response.body()
    }

    override suspend fun exchangeOidcCode(
        code: String,
        state: String,
        redirectUri: String,
        codeVerifier: String?
    ): AuthResponse {
        // Fallback: call the server's OIDC callback directly via GET.
        // The server exchanges the authorization code internally and returns tokens.
        val response = httpClient.get("/auth/oidc/callback") {
            url {
                parameters.append("code", code)
                parameters.append("state", state)
                if (codeVerifier != null) {
                    parameters.append("code_verifier", codeVerifier)
                }
            }
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "OIDC code exchange failed"
            )
        }

        return response.body()
    }

    override suspend fun redeemQrToken(serverUrl: String, token: String): AuthResponse {
        val url = serverUrl.trimEnd('/') + "/auth/qr/redeem"
        val response = httpClient.post(url) {
            setBody(QrRedeemRequest(token))
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "QR code expired or already used"
            )
        }

        return response.body()
    }
}

@Serializable
internal data class ApiErrorResponse(
    val error: String? = null,
    val message: String? = null
)

open class ApiException(
    val status: HttpStatusCode,
    override val message: String,
    /** Server error code (`error` field), e.g. `VALIDATION_ERROR`; null if absent. */
    val code: String? = null
) : Exception(message)

class MfaRequiredApiException(
    message: String
) : ApiException(HttpStatusCode.Forbidden, message)
