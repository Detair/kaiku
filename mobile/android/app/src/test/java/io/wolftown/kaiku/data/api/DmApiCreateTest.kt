package io.wolftown.kaiku.data.api

import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class DmApiCreateTest {
    private val rec = ApiRecorder()
    private val api = DmApiImpl(rec.client)

    @Test
    fun `create sends one participant and reads the flattened channel`() = runTest {
        rec.status = HttpStatusCode.Created
        rec.body = """{"id":"dm-1","name":"anna, me","channel_type":"dm","created_at":"2026-11-19T10:00:00Z",
            "participants":[{"user_id":"anna","username":"anna","display_name":"Anna","joined_at":"2026-11-19T10:00:00Z","is_system":false}]}"""

        val dm = api.createDm("anna")

        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/api/dm", rec.last.path)
        val body = Json.parseToJsonElement(rec.last.body).jsonObject
        assertEquals(setOf("participant_ids"), body.keys)
        assertEquals("anna", body["participant_ids"]!!.jsonArray.single().jsonPrimitive.content)
        assertEquals("dm-1", dm.id)
        assertEquals("2026-11-19T10:00:00Z", dm.createdAt)
        assertEquals("Anna", dm.participants.single().displayName)
    }

    @Test
    fun `a refused create carries the server code`() = runTest {
        rec.status = HttpStatusCode.BadRequest
        rec.body = """{"error":"VALIDATION_ERROR","message":"Cannot create DM with this user"}"""
        try {
            api.createDm("blocked")
            fail("expected ApiException")
        } catch (e: ApiException) {
            assertEquals("VALIDATION_ERROR", e.normalizedCode)
        }
    }

    @Test
    fun `blocked list is read as user ids`() = runTest {
        rec.body = """[{"user_id":"x","username":"troll","display_name":"T","friendship_id":"f","friendship_status":"blocked","is_online":false,"created_at":"2026-01-01T00:00:00Z"}]"""
        assertEquals(setOf("x"), api.getBlocked())
        assertEquals("/api/friends/blocked", rec.last.path)
    }

    @Test
    fun `requests made for a session are not sent once another session is current`() = runTest {
        // Built for session 1; another account (session 2) is signed in by the time they would go out.
        rec.session.value = io.wolftown.kaiku.data.local.AuthSession.LoggedIn("bert", 2L)
        for (request in listOf<suspend () -> Any?>(
            { api.getDms() },
            { api.createDm("anna") },
            { api.getBlocked() },
            { api.markAsRead("dm-1", "m1") },
        )) {
            val failure = runCatching { withRequestSession(ApiRecorder.SESSION) { request() } }.exceptionOrNull()
            org.junit.Assert.assertTrue("$failure", failure is SessionChangedException)
        }
        org.junit.Assert.assertTrue(rec.requests.isEmpty())
    }

    @Test
    fun `a request made for the current session goes out`() = runTest {
        rec.body = "[]"
        withRequestSession(ApiRecorder.SESSION) { api.getDms() }
        assertEquals("/api/dm", rec.last.path)
    }
}
