package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.LoginAttempt
import io.wolftown.kaiku.domain.model.LoginStep
import kotlinx.serialization.Serializable
import javax.inject.Inject

/**
 * C27 sign-in attempts (SM1) for a phone that is not a key. The app is native:
 * it sends `client_challenge` = S256(verifier) at create and the verifier in
 * every later call. All routes are pre-session (no bearer).
 */
interface LoginAttemptApi {
    suspend fun createQr(clientChallenge: String): LoginAttempt
    suspend fun login(username: String, password: String, clientChallenge: String): LoginStep
    suspend fun poll(id: String, verifier: String, waitSeconds: Int): LoginAttempt
    suspend fun collect(id: String, verifier: String): AuthResponse
    suspend fun totp(id: String, verifier: String, code: String): AuthResponse
    suspend fun cancel(id: String, verifier: String)
    suspend fun exchangeOidc(code: String, verifier: String): LoginStep
}

@Serializable
private data class CreateAttemptRequest(val clientChallenge: String)

@Serializable
private data class PasswordLoginRequest(val username: String, val password: String, val clientChallenge: String) {
    override fun toString() = "PasswordLoginRequest"
}

@Serializable
private data class PollRequest(val verifier: String, val wait: Int) {
    override fun toString() = "PollRequest(wait=$wait)"
}

@Serializable
private data class VerifierRequest(val verifier: String) {
    override fun toString() = "VerifierRequest"
}

@Serializable
private data class TotpRequest(val verifier: String, val code: String) {
    override fun toString() = "TotpRequest"
}

@Serializable
private data class OidcExchangeRequest(val code: String, val verifier: String) {
    override fun toString() = "OidcExchangeRequest"
}

class LoginAttemptApiImpl @Inject constructor(private val httpClient: HttpClient) : LoginAttemptApi {

    override suspend fun createQr(clientChallenge: String): LoginAttempt {
        val r = httpClient.post("/auth/login-attempts") { publicRoute(); setBody(CreateAttemptRequest(clientChallenge)) }
        r.ensureSuccess("create attempt failed")
        return r.body()
    }

    override suspend fun login(username: String, password: String, clientChallenge: String): LoginStep {
        val r = httpClient.post("/auth/login") { publicRoute(); setBody(PasswordLoginRequest(username, password, clientChallenge)) }
        r.ensureSuccess("login failed")
        return r.body()
    }

    override suspend fun poll(id: String, verifier: String, waitSeconds: Int): LoginAttempt {
        val r = httpClient.post("/auth/login-attempts/$id/poll") {
            publicRoute()
            setBody(PollRequest(verifier, waitSeconds.coerceIn(0, 25)))
        }
        r.ensureSuccess("poll failed")
        return r.body()
    }

    override suspend fun collect(id: String, verifier: String): AuthResponse {
        val r = httpClient.post("/auth/login-attempts/$id/collect") { publicRoute(); setBody(VerifierRequest(verifier)) }
        r.ensureSuccess("collect failed")
        return r.body()
    }

    override suspend fun totp(id: String, verifier: String, code: String): AuthResponse {
        val r = httpClient.post("/auth/login-attempts/$id/totp") { publicRoute(); setBody(TotpRequest(verifier, code)) }
        r.ensureSuccess("totp failed")
        return r.body()
    }

    override suspend fun cancel(id: String, verifier: String) {
        httpClient.post("/auth/login-attempts/$id/cancel") { publicRoute(); setBody(VerifierRequest(verifier)) }
            .ensureSuccess("cancel failed")
    }

    override suspend fun exchangeOidc(code: String, verifier: String): LoginStep {
        val r = httpClient.post("/auth/oidc/desktop/exchange") { publicRoute(); setBody(OidcExchangeRequest(code, verifier)) }
        r.ensureSuccess("oidc exchange failed")
        return r.body()
    }
}
