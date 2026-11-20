package io.wolftown.kaiku.data.api

import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** The removal's calls carry the credentials they are handed, to the server they are told, and nothing stored. */
class RemovalApiTest {

    // The app holds another (stored) session: none of these calls may ever carry it.
    private val rec = ApiRecorder(bearer = "stored-access-token")
    private val api = RemovalApiImpl(rec.client)
    private val origin = "https://key.example.com"
    private val challenge = javaClass.classLoader!!.getResource("contract/approval_challenge_login.json")!!.readText()

    @Test fun `every call names its server and its bearer`() = runTest {
        rec.respond = { r ->
            when {
                r.path == "/auth/me" -> HttpStatusCode.OK to """{"id":"u1","username":"alice","display_name":"Alice"}"""
                r.path == "/api/approval-devices" && r.method == HttpMethod.Get -> HttpStatusCode.OK to """{"devices":[]}"""
                r.path == "/api/approvals/step-up" -> HttpStatusCode.Created to challenge
                r.path.endsWith("/answer") -> HttpStatusCode.OK to """{"state":"consumed"}"""
                else -> HttpStatusCode.OK to javaClass.classLoader!!.getResource("contract/key_device.json")!!.readText()
            }
        }
        api.me(origin, "removal-access")
        api.keyDevices(origin, "removal-access")
        api.stepUp(origin, "removal-access", "key_device.remove", "kd1")
        api.answer(origin, "removal-access", "c1", "approve", "", "MEUCIQ")
        api.removeKeyDevice(origin, "removal-access", "kd1")

        assertEquals(
            listOf(
                "GET https://key.example.com/auth/me", "GET https://key.example.com/api/approval-devices",
                "POST https://key.example.com/api/approvals/step-up", "POST https://key.example.com/api/approvals/c1/answer",
                "DELETE https://key.example.com/api/approval-devices/kd1",
            ),
            rec.requests.map { "${it.method.value} ${it.url}" },
        )
        assertTrue(rec.requests.all { it.headers["Authorization"] == "Bearer removal-access" })
    }

    @Test fun `a refused call is not repeated with a stored or refreshed token`() = runTest {
        rec.status = HttpStatusCode.Unauthorized
        rec.body = """{"error":"INVALID_TOKEN","message":"x"}"""
        try {
            api.removeKeyDevice(origin, "removal-access", "kd1")
            fail("expected ApiException")
        } catch (e: ApiException) {
            assertEquals(401, e.status.value)
            assertEquals("INVALID_TOKEN", e.normalizedCode)
        }
        assertEquals(1, rec.requests.size)
        assertEquals("Bearer removal-access", rec.last.headers["Authorization"])
    }

    @Test fun `only the contract's own success counts`() = runTest {
        rec.status = HttpStatusCode.Accepted
        rec.body = "{}"
        try {
            api.removeKeyDevice(origin, "removal-access", "kd1")
            fail("expected ApiException")
        } catch (e: ApiException) {
            assertEquals(202, e.status.value)
            assertEquals(null, e.code)
        }
    }
}
