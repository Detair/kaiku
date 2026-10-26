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
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class PushApiTest {

    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://kaiku.example.com"
    }
    private val authState: AuthState = mockk(relaxed = true)
    private val jsonHeaders = headersOf(HttpHeaders.ContentType, "application/json")

    private val requests = mutableListOf<Triple<HttpMethod, String, String>>()

    private fun api(
        handler: suspend MockRequestHandleScope.(HttpRequestData) -> HttpResponseData,
    ): PushApi {
        val engine = MockEngine { request ->
            requests += Triple(request.method, request.url.encodedPath, String(request.body.toByteArray()))
            handler(this, request)
        }
        return PushApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient())
    }

    @Test
    fun `register sends snake case body`() = runTest {
        val api = api { respond(SUBSCRIPTION_JSON, HttpStatusCode.Created, jsonHeaders) }

        api.register(PushRegistration("fcm", "fcm-token-1", DEVICE_ID, "Pixel 8"))

        val (method, path, body) = requests.single()
        assertEquals(HttpMethod.Post, method)
        assertEquals("/api/me/push-subscriptions", path)
        val json = Json.parseToJsonElement(body).jsonObject
        assertEquals(setOf("provider", "endpoint", "device_id", "device_label"), json.keys)
        assertEquals("fcm", json["provider"]?.jsonPrimitive?.content)
        assertEquals("fcm-token-1", json["endpoint"]?.jsonPrimitive?.content)
        assertEquals(DEVICE_ID, json["device_id"]?.jsonPrimitive?.content)
        assertEquals("Pixel 8", json["device_label"]?.jsonPrimitive?.content)
    }

    @Test
    fun `config decodes without fcm`() = runTest {
        val api = api { respond("""{"providers":["unifiedpush"],"fcm":null}""", HttpStatusCode.OK, jsonHeaders) }

        val config = api.getConfig()

        assertEquals(listOf("unifiedpush"), config.providers)
        assertNull(config.fcm)
        assertEquals("/api/me/push-subscriptions/config", requests.single().second)
        assertEquals(HttpMethod.Get, requests.single().first)
    }

    @Test
    fun `config decodes fcm project`() = runTest {
        val api = api {
            respond(
                """{"providers":["fcm","unifiedpush"],"fcm":{"project_id":"kaiku-prod"}}""",
                HttpStatusCode.OK,
                jsonHeaders,
            )
        }

        val config = api.getConfig()

        assertEquals(listOf("fcm", "unifiedpush"), config.providers)
        assertEquals("kaiku-prod", config.fcm?.projectId)
    }

    @Test
    fun `unregister device uses device path`() = runTest {
        val api = api { respond("", HttpStatusCode.NoContent) }

        api.unregisterDevice(DEVICE_ID)

        val (method, path, _) = requests.single()
        assertEquals(HttpMethod.Delete, method)
        assertEquals("/api/me/push-subscriptions/device/$DEVICE_ID", path)
    }

    @Test
    fun `rejected registration surfaces status and code only`() = runTest {
        val api = api {
            respond(
                """{"error":"VALIDATION_ERROR","message":"endpoint is too long"}""",
                HttpStatusCode.BadRequest,
                jsonHeaders,
            )
        }
        val endpoint = "x".repeat(2049)

        try {
            api.register(PushRegistration("fcm", endpoint, DEVICE_ID, null))
            fail("expected ApiException")
        } catch (e: ApiException) {
            assertEquals(HttpStatusCode.BadRequest, e.status)
            assertEquals("VALIDATION_ERROR", e.code)
            assertFalse(e.message.contains(endpoint))
        }
    }

    @Test
    fun `registration without label sends null label`() = runTest {
        val api = api { respond(SUBSCRIPTION_JSON, HttpStatusCode.Created, jsonHeaders) }

        api.register(PushRegistration("fcm", "fcm-token-1", DEVICE_ID, null))

        val json = Json.parseToJsonElement(requests.single().third) as JsonObject
        assertTrue(json.containsKey("device_label"))
        assertEquals("null", json["device_label"].toString())
    }

    private companion object {
        const val DEVICE_ID = "6f1c2b9e-3d4a-4e5f-8a7b-9c0d1e2f3a4b"
        const val SUBSCRIPTION_JSON =
            """{"id":"00000000-0000-0000-0000-000000000001","provider":"fcm","endpoint":"fcm-token-1","device_label":"Pixel 8","created_at":"2026-10-24T10:00:00Z","last_used_at":null,"device_id":"6f1c2b9e-3d4a-4e5f-8a7b-9c0d1e2f3a4b"}"""
    }
}
