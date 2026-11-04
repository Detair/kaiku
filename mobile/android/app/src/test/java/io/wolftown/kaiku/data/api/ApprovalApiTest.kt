package io.wolftown.kaiku.data.api

import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class ApprovalApiTest {

    private val rec = ApiRecorder()
    private val api = ApprovalApiImpl(rec.client)
    private val challenge = javaClass.classLoader!!.getResource("contract/approval_challenge_login.json")!!.readText()

    private fun bodyKeys() = Json.parseToJsonElement(rec.last.body).jsonObject

    @Test fun `claim posts the token`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = challenge
        val ch = api.claim("oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8")
        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/api/approvals/claim", rec.last.path)
        assertEquals("oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8", bodyKeys()["token"]!!.toString().trim('"'))
        assertEquals("login", ch.kind)
    }

    @Test fun `step up posts action and target`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = challenge
        api.stepUp("key_device.remove", "99999999-9999-4999-8999-999999999999")
        assertEquals("/api/approvals/step-up", rec.last.path)
        assertEquals(setOf("action", "target"), bodyKeys().keys)
    }

    @Test fun `enrollment challenge posts to the enrollment`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = challenge
        api.enrollmentChallenge("55555555-5555-4555-8555-555555555555")
        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/auth/enrollments/55555555-5555-4555-8555-555555555555/challenge", rec.last.path)
    }

    @Test fun `answer sends decision response and signature`() = runTest {
        rec.body = """{"state":"consumed","grant":{"action":"key_device.remove","target":"t","via":"device","expires_at":"2026-11-03T12:05:00Z"}}"""
        val r = api.answer("33333333-3333-4333-8333-333333333304", "approve", "", "MEUCIQ")
        assertEquals("/api/approvals/33333333-3333-4333-8333-333333333304/answer", rec.last.path)
        assertEquals(setOf("decision", "response", "signature"), bodyKeys().keys)
        assertEquals("consumed", r.state)
        assertEquals("device", r.grant!!.via)
        assertNull(r.enrollment)
    }

    @Test fun `deny and release send no body`() = runTest {
        rec.status = HttpStatusCode.NoContent; rec.body = ""
        api.deny("c1")
        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/api/approvals/c1/deny", rec.last.path)
        assertEquals("", rec.last.body)
        api.release("c1")
        assertEquals("/api/approvals/c1/release", rec.last.path)
        assertEquals("", rec.last.body)
    }

    @Test fun `device login uses public routes without a bearer`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = challenge
        api.deviceLoginChallenge("11111111-1111-4111-8111-111111111111", "22222222-2222-4222-8222-222222222222")
        assertEquals("/auth/device-login/challenge", rec.last.path)
        assertEquals(setOf("user_id", "device_id"), bodyKeys().keys)
        assertNull(rec.last.headers["Authorization"])
        rec.status = HttpStatusCode.OK
        rec.body = javaClass.classLoader!!.getResource("contract/auth_response_full.json")!!.readText()
        val auth = api.deviceLogin("c3", "MEUCIQ")
        assertEquals("/auth/device-login", rec.last.path)
        assertEquals(setOf("challenge_id", "signature"), bodyKeys().keys)
        assertNull(rec.last.headers["Authorization"])
        assertEquals("full", auth.assurance)
        assertEquals("77777777-7777-4777-8777-777777777777", auth.sessionId)
    }

    @Test fun `key devices list remove and report invalidated`() = runTest {
        val kd = javaClass.classLoader!!.getResource("contract/key_device.json")!!.readText()
        rec.body = """{"devices":[$kd]}"""
        val list = api.keyDevices()
        assertEquals(HttpMethod.Get, rec.last.method)
        assertEquals("/api/approval-devices", rec.last.path)
        assertTrue(list.single().isCurrent)
        rec.body = kd
        api.removeKeyDevice("99999999-9999-4999-8999-999999999999")
        assertEquals(HttpMethod.Delete, rec.last.method)
        assertEquals("/api/approval-devices/99999999-9999-4999-8999-999999999999", rec.last.path)
        api.reportInvalidated("99999999-9999-4999-8999-999999999999")
        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/api/approval-devices/99999999-9999-4999-8999-999999999999/invalidated", rec.last.path)
    }

    @Test fun `error codes map through normalizeErrorCode with their fields`() = runTest {
        rec.status = HttpStatusCode.Conflict
        rec.body = """{"error":"key_device_pending","message":"x","ready_at":"2026-11-04T12:00:00Z"}"""
        try {
            api.deviceLoginChallenge("u", "d")
            fail("expected ApiException")
        } catch (e: ApiException) {
            assertEquals("KEY_DEVICE_PENDING", e.normalizedCode)
            assertEquals("2026-11-04T12:00:00Z", e.fields.readyAt)
        }
    }

    @Test fun `the key-device probe goes to the key's own server, not the saved one`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = challenge
        // The saved server changed meanwhile; the key belongs to chat.example.com.
        api.deviceLoginChallenge("u", "d", origin = "https://key.example.com")
        assertTrue(rec.last.url.startsWith("https://key.example.com/"))
        assertEquals("/auth/device-login/challenge", rec.last.path)
        assertNull(rec.last.headers["Authorization"])
    }
}
