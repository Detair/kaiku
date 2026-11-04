package io.wolftown.kaiku.data.api

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.domain.model.AttemptState
import io.wolftown.kaiku.domain.model.LoginStepStatus
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class LoginAttemptApiTest {

    private val rec = ApiRecorder(bearer = null)
    private val api = LoginAttemptApiImpl(rec.client)
    private fun res(name: String) = javaClass.classLoader!!.getResource("contract/$name")!!.readText()
    private fun bodyKeys() = Json.parseToJsonElement(rec.last.body).jsonObject.keys

    @Test fun `create qr sends the client challenge`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = res("login_attempt_open.json")
        val a = api.createQr("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
        assertEquals("/auth/login-attempts", rec.last.path)
        assertEquals(setOf("client_challenge"), bodyKeys())
        assertEquals(AttemptState.OPEN, a.state)
    }

    @Test fun `login never sends mfa_code`() = runTest {
        rec.status = HttpStatusCode.Accepted; rec.body = res("login_step.json")
        val step = api.login("alice", "pw", "challenge")
        assertEquals("/auth/login", rec.last.path)
        assertEquals(setOf("username", "password", "client_challenge"), bodyKeys())
        assertFalse(rec.last.body.contains("mfa"))
        assertEquals(LoginStepStatus.CONFIRMATION_REQUIRED, step.status)
        assertEquals(true, step.attempt.totpAvailable)
    }

    @Test fun `attempt calls carry the verifier`() = runTest {
        rec.body = res("login_attempt_open.json")
        api.poll("a1", "verifier-secret", 25)
        assertEquals("/auth/login-attempts/a1/poll", rec.last.path)
        assertEquals(setOf("verifier", "wait"), bodyKeys())
        rec.body = res("auth_response_full.json")
        api.collect("a1", "verifier-secret")
        assertEquals("/auth/login-attempts/a1/collect", rec.last.path)
        assertEquals(setOf("verifier"), bodyKeys())
        api.totp("a1", "verifier-secret", "123456")
        assertEquals("/auth/login-attempts/a1/totp", rec.last.path)
        assertEquals(setOf("verifier", "code"), bodyKeys())
        rec.status = HttpStatusCode.NoContent; rec.body = ""
        api.cancel("a1", "verifier-secret")
        assertEquals("/auth/login-attempts/a1/cancel", rec.last.path)
        assertNull(rec.last.headers["Authorization"])
    }

    @Test fun `oidc exchange sends code and verifier`() = runTest {
        rec.status = HttpStatusCode.Accepted; rec.body = res("login_step.json")
        api.exchangeOidc("one-time-code", "verifier-secret")
        assertEquals("/auth/oidc/desktop/exchange", rec.last.path)
        assertEquals(setOf("code", "verifier"), bodyKeys())
    }
}
