package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.client.request.HttpResponseData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.Golden
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.test.assertFailsWith

class PasskeyApiTest {
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://kaiku.example.com"
    }
    private val authState: AuthState = mockk(relaxed = true)
    private val json = headersOf(HttpHeaders.ContentType, "application/json")
    private val requests = mutableListOf<HttpRequestData>()
    private val bodies = mutableListOf<String>()

    private fun api(handler: suspend MockRequestHandleScope.(HttpRequestData) -> HttpResponseData): PasskeyApi {
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())
            handler(this, request)
        }
        return PasskeyApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient())
    }

    private val credential = JsonObject(mapOf("id" to JsonPrimitive("Y3JlZA"), "type" to JsonPrimitive("public-key")))

    @Test
    fun `every call sends the android client header and the C29 paths`() = runTest {
        val start = Golden.load("passkey_get_start.json")
        val p = api { req ->
            when (req.url.encodedPath) {
                "/auth/passkeys/login/start", "/auth/passkeys/step-up/start", "/auth/passkeys/register/start" ->
                    respond(start, HttpStatusCode.OK, json)
                "/auth/passkeys/login/finish" -> respond(Golden.load("auth_response_device_key.json"), HttpStatusCode.OK, json)
                "/auth/passkeys/step-up/finish" -> respond(Golden.load("step_up_grant.json"), HttpStatusCode.OK, json)
                "/auth/passkeys/register/finish" -> respond(Golden.load("passkey.json"), HttpStatusCode.Created, json)
                "/auth/passkeys" -> respond("""{"rp_id":"chat.example.com","user_handle":"dQ","passkeys":[]}""", HttpStatusCode.OK, json)
                "/auth/reauth/methods" -> respond("""{"passkey":true,"device":false}""", HttpStatusCode.OK, json)
                else -> respond(Golden.load("passkey_removed.json"), HttpStatusCode.OK, json)
            }
        }
        val s = p.loginStart()
        p.loginFinish(s.ceremonyId, credential, " fabian ")
        p.stepUpStart("passkey.remove", "00000000-0000-7000-8000-00000000e001")
        p.stepUpFinish(s.ceremonyId, credential)
        p.registerStart("Pixel 8")
        p.registerFinish(s.ceremonyId, credential)
        p.list()
        p.delete("00000000-0000-7000-8000-00000000e001")
        assertTrue(p.reauthMethods().passkey)
        assertEquals(
            listOf(
                "/auth/passkeys/login/start", "/auth/passkeys/login/finish", "/auth/passkeys/step-up/start",
                "/auth/passkeys/step-up/finish", "/auth/passkeys/register/start", "/auth/passkeys/register/finish",
                "/auth/passkeys", "/auth/passkeys/00000000-0000-7000-8000-00000000e001", "/auth/reauth/methods",
            ),
            requests.map { it.url.encodedPath },
        )
        assertEquals(HttpMethod.Delete, requests[7].method)
        assertTrue(requests.all { it.headers["X-Kaiku-Client"] == "android" })
        val finish = Json.parseToJsonElement(bodies[1]).jsonObject
        assertEquals(s.ceremonyId, (finish["ceremony_id"] as JsonPrimitive).content)
        assertEquals(credential, finish["credential"])
        assertEquals("fabian", (finish["username"] as JsonPrimitive).content)
        assertFalse(bodies.any { it.contains("reauth") })
        assertEquals("Pixel 8", (Json.parseToJsonElement(bodies[4]).jsonObject["label"] as JsonPrimitive).content)
    }

    @Test
    fun `server codes are normalized and ready_at kept`() = runTest {
        val p = api {
            respond("""{"error":"passkey_pending","message":"x","ready_at":"2026-11-05T10:00:00Z"}""", HttpStatusCode.Conflict, json)
        }
        val e = assertFailsWith<PasskeyApiException> { p.loginFinish("c", credential, null) }
        assertEquals("PASSKEY_PENDING", e.normalizedCode)
        assertEquals("2026-11-05T10:00:00Z", e.readyAt)
    }

    @Test
    fun `rename and delete encode the id`() = runTest {
        // Review Argon minor 5.
        val p = api { req ->
            if (req.method == HttpMethod.Patch) respond(Golden.load("passkey.json"), HttpStatusCode.OK, json)
            else respond(Golden.load("passkey_removed.json"), HttpStatusCode.OK, json)
        }
        p.rename("a/b c", "Work")
        p.delete("a/b c")
        assertEquals(HttpMethod.Patch, requests[0].method)
        assertEquals("/auth/passkeys/a%2Fb%20c", requests[0].url.encodedPath)
        assertEquals("/auth/passkeys/a%2Fb%20c", requests[1].url.encodedPath)
        assertEquals("Work", (Json.parseToJsonElement(bodies[0]).jsonObject["label"] as JsonPrimitive).content)
    }
}
