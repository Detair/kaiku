package io.wolftown.kaiku.data.api

import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test

class CallApiTest {
    private val rec = ApiRecorder()
    private val api = CallApiImpl(rec.client)

    @Test
    fun `no call reads as null`() = runTest {
        rec.body = "null"
        assertNull(api.get("dm-1", ApiRecorder.SESSION))
        assertEquals(HttpMethod.Get, rec.last.method)
        assertEquals("/api/dm/dm-1/call", rec.last.path)
    }

    @Test
    fun `a ringing call carries who started it and who declined`() = runTest {
        rec.body = """{"channel_id":"dm-1","call_id":"c-1","status":"ringing","started_by":"anna",
            "started_at":"2026-11-19T10:00:00Z","declined_by":["bob"],"target_users":["me","bob"],"capabilities":["audio"]}"""
        val call = api.get("dm-1", ApiRecorder.SESSION)!!
        assertEquals(CallStateDto("dm-1", "c-1", "ringing", "anna", listOf("me", "bob"), listOf("bob")), call)
    }

    @Test
    fun `an active call carries its participants`() = runTest {
        rec.body = """{"channel_id":"dm-1","call_id":"c-1","status":"active","started_at":"2026-11-19T10:00:00Z",
            "participants":["anna","me"],"capabilities":["audio"]}"""
        assertEquals(listOf("anna", "me"), api.join("dm-1", ApiRecorder.SESSION).participants)
        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/api/dm/dm-1/call/join", rec.last.path)
    }

    @Test
    fun `a call request of a session that is over is not sent`() = runTest {
        // Another account is signed in by now: the request built for session 1 must not leave with its token.
        rec.session.value = io.wolftown.kaiku.data.local.AuthSession.LoggedIn("bert", 2L)
        for (request in listOf<suspend () -> Any?>(
            { api.get("dm-1", ApiRecorder.SESSION) },
            { api.start("dm-1", ApiRecorder.SESSION) },
            { api.join("dm-1", ApiRecorder.SESSION) },
            { api.decline("dm-1", ApiRecorder.SESSION) },
            { api.leave("dm-1", ApiRecorder.SESSION) },
        )) {
            val failure = runCatching { request() }.exceptionOrNull()
            org.junit.Assert.assertTrue("$failure", failure is SessionChangedException)
        }
        org.junit.Assert.assertTrue(rec.requests.isEmpty())
    }

    @Test
    fun `a refusal carries the code field`() = runTest {
        rec.status = HttpStatusCode.Conflict
        rec.body = """{"error":"Call already exists","code":"CALL_ALREADY_EXISTS"}"""
        try {
            api.start("dm-1", ApiRecorder.SESSION)
            fail("expected ApiException")
        } catch (e: ApiException) {
            assertEquals(CallApi.CALL_ALREADY_EXISTS, e.normalizedCode)
            assertEquals("/api/dm/dm-1/call/start", rec.last.path)
        }
    }
}
