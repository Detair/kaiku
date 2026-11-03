package io.wolftown.kaiku.data.api

import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AdminApiTest {
    private val rec = ApiRecorder()
    private val api = AdminApiImpl(rec.client)
    private fun body() = Json.parseToJsonElement(rec.last.body).jsonObject

    @Test fun `status and elevate`() = runTest {
        rec.body = """{"is_admin":true,"is_elevated":false,"elevation_expires_at":null}"""
        assertTrue(api.status())
        assertEquals("/api/admin/status", rec.last.path)
        rec.body = "{}"
        api.elevate(null)
        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/api/admin/elevate", rec.last.path)
        assertEquals(JsonNull, body()["reason"])
    }

    @Test fun `new account sends no password`() = runTest {
        rec.status = HttpStatusCode.Created
        rec.body = """{"id":"u2","username":"oma","display_name":"Oma","created_at":"2026-11-03T12:00:00Z","is_banned":false}"""
        val u = api.createUser("oma", "Oma", recoveryContactId = "u1")
        assertEquals("/api/admin/users", rec.last.path)
        assertEquals(setOf("username", "display_name", "recovery_contact_id"), body().keys)
        assertTrue("password" !in rec.last.body)
        assertEquals("u2", u.id)
    }

    @Test fun `users user and admin actions`() = runTest {
        rec.body = """{"items":[{"id":"u2","username":"oma","display_name":"Oma","is_banned":false}],"total":1,"limit":50,"offset":0}"""
        assertEquals("oma", api.users("om").single().username)
        assertEquals("/api/admin/users", rec.last.path)
        assertTrue(rec.last.query.contains("search=om"))
        rec.body = """{"id":"u2","username":"oma","display_name":"Oma","may_approve":false,"key_devices":[{"id":"kd1","label":"Pixel 6","state":"active"}],"passkeys":[],"open_request":null,"bootstrap_open":true}"""
        val d = api.user("u2")
        assertEquals("/api/admin/users/u2/details", rec.last.path)
        assertEquals("Pixel 6", d.keyDevices.single().label)
        rec.status = HttpStatusCode.Created
        rec.body = """{"enrollment_id":"e1","setup_token":"t","setup_url":"https://chat.example.com/setup#v=1&t=t","expires_at":"2026-11-03T12:10:00Z"}"""
        api.issueEnrollment("u2", "recovery", "called_back", "Called her on the landline", listOf("kd1"))
        assertEquals("/api/admin/users/u2/enrollments", rec.last.path)
        assertEquals("recovery", body()["mode"]!!.jsonPrimitive.content)
        assertEquals("called_back", body()["claimant_check"]!!.jsonObject["method"]!!.jsonPrimitive.content)
        assertEquals("kd1", body()["lost_device_ids"]!!.jsonArray.single().jsonPrimitive.content)
        rec.status = HttpStatusCode.OK; rec.body = "{}"
        api.setMayApprove("u2", true)
        assertEquals(HttpMethod.Put, rec.last.method)
        assertEquals("/api/admin/users/u2/may-approve", rec.last.path)
        assertEquals("true", body()["may_approve"].toString())
        rec.status = HttpStatusCode.NoContent; rec.body = ""
        api.removeAuthenticator("u2", "kd1")
        assertEquals("/api/admin/users/u2/authenticators/kd1/remove", rec.last.path)
    }
}
