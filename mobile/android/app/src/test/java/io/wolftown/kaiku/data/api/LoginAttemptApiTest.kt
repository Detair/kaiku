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
import org.junit.Assert.assertTrue
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

    /**
     * Codex v1 integration check, major 2: the native SSO start is fetched with
     * `X-Kaiku-Client: android` (a Custom Tab cannot send it) and answers JSON
     * `{url}`, the provider page the Custom Tab then opens.
     */
    @Test fun `oidc start sends the android header and reads the provider url`() = runTest {
        rec.body = """{"url":"https://idp.example.com/authorize?client_id=kaiku&state=s1"}"""
        val url = api.oidcAuthorizeUrl("my provider", "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
        assertEquals("https://idp.example.com/authorize?client_id=kaiku&state=s1", url)
        assertEquals(io.ktor.http.HttpMethod.Get, rec.last.method)
        assertEquals("/auth/oidc/authorize/my%20provider", rec.last.path)
        assertEquals("client_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM", rec.last.query)
        assertEquals("android", rec.last.headers["X-Kaiku-Client"])
        assertNull(rec.last.headers["Authorization"])
    }

    @Test fun `oidc start refusals keep the server code`() = runTest {
        rec.status = HttpStatusCode.BadRequest
        rec.body = """{"error":"OIDC_CLIENT_OUTDATED","message":"update"}"""
        val e = runCatching { api.oidcAuthorizeUrl("google", "c") }.exceptionOrNull() as ApiException
        assertEquals("OIDC_CLIENT_OUTDATED", e.code)
    }

    @Test fun `oidc start without a provider url fails`() = runTest {
        rec.body = """{"url":"kaiku://auth/callback?code=x"}"""
        assertTrue(runCatching { api.oidcAuthorizeUrl("google", "c") }.isFailure)
        rec.body = "{}"
        assertTrue(runCatching { api.oidcAuthorizeUrl("google", "c") }.isFailure)
    }

    @Test fun `oidc exchange sends code and verifier`() = runTest {
        rec.status = HttpStatusCode.Accepted; rec.body = res("login_step.json")
        api.exchangeOidc("one-time-code", "verifier-secret")
        assertEquals("/auth/oidc/desktop/exchange", rec.last.path)
        assertEquals(setOf("code", "verifier"), bodyKeys())
    }
}
