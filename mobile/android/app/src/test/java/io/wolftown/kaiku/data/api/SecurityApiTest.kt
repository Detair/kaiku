package io.wolftown.kaiku.data.api

import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SecurityApiTest {

    private val rec = ApiRecorder()
    private val api = SecurityApiImpl(rec.client)
    private fun res(name: String) = javaClass.classLoader!!.getResource("contract/$name")!!.readText()
    private fun body() = Json.parseToJsonElement(rec.last.body).jsonObject

    @Test fun `me security and the log`() = runTest {
        rec.body = res("me_security.json")
        val me = api.meSecurity()
        assertEquals("/api/me/security", rec.last.path)
        assertEquals(9, me.recoveryCodesLeft)
        rec.body = """{"items":[${res("security_event.json")}],"has_more":true,"next_cursor":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"}"""
        val page = api.events(before = "x1", limit = 50)
        assertEquals("/api/me/security-events", rec.last.path)
        assertTrue(rec.last.query.contains("before=x1"))
        assertTrue(rec.last.query.contains("limit=50"))
        assertTrue(page.hasMore)
        api.events(before = null, limit = 50)
        assertTrue(!rec.last.query.contains("before"))
    }

    @Test fun `repudiate sends part2 null or the factor`() = runTest {
        rec.body = """{"event":${res("security_event.json")},"ended_session_ids":[],"revoked_authenticator_ids":[],"compromised":["password"],"voided_request_ids":[]}"""
        val r = api.repudiate("ev1", null)
        assertEquals("/api/me/security-events/ev1/repudiate", rec.last.path)
        assertEquals(JsonNull, body()["part2"])
        assertEquals(listOf("password"), r.compromised)
        api.repudiate("ev1", Part2Factor.recoveryCode("ABCD-EFGH"))
        assertEquals(
            """{"factor":{"type":"recovery_code","code":"ABCD-EFGH"}}""",
            body()["part2"].toString(),
        )
    }

    @Test fun `cancel request and my requests`() = runTest {
        rec.body = res("account_request_summary.json")
        api.cancelRequest("rq1", Part2Factor.totp("123456"))
        assertEquals("/api/account-requests/rq1/cancel", rec.last.path)
        assertEquals(setOf("reason", "part2"), body().keys)
        rec.body = """{"requests":[${res("account_request_summary.json")}]}"""
        assertEquals("oma", api.myRequests().single().subject.username)
        assertEquals("/api/me/account-requests", rec.last.path)
    }

    @Test fun `sessions routes`() = runTest {
        rec.body = """{"sessions":[{"id":"s1","device":"Kaiku desktop","created_at":"2026-11-03T12:00:00Z","expires_at":"2026-11-10T12:00:00Z","is_current":false,"assurance":"probation","client_class":"desktop","auth_methods":["qr"],"authenticator":null,"last_used_at":"2026-11-03T12:00:00Z","absolute_expires_at":"2027-02-01T12:00:00Z"}]}"""
        assertEquals("probation", api.sessions().single().assurance)
        assertEquals("/auth/sessions", rec.last.path)
        rec.status = HttpStatusCode.NoContent; rec.body = ""
        api.endSession("s1")
        assertEquals(HttpMethod.Delete, rec.last.method)
        assertEquals("/auth/sessions/s1", rec.last.path)
        api.endOtherSessions()
        assertEquals(HttpMethod.Delete, rec.last.method)
        assertEquals("/auth/sessions", rec.last.path)
    }

    @Test fun `recovery codes and contact`() = runTest {
        rec.body = """{"left":7}"""
        assertEquals(7, api.recoveryCodesLeft())
        assertEquals("/auth/recovery-codes/count", rec.last.path)
        rec.body = """{"recovery_codes":["AAAA-BBBB-CCCC-D"]}"""
        assertEquals(1, api.createRecoveryCodes().size)
        assertEquals(HttpMethod.Post, rec.last.method)
        rec.body = """{"user_id":"u2","username":"oma","display_name":"Oma","named_at":"2026-11-03T12:00:00Z","confirmed_at":null,"counts_from":null}"""
        api.setRecoveryContact("u2")
        assertEquals(HttpMethod.Put, rec.last.method)
        assertEquals("/api/me/recovery-contact", rec.last.path)
        assertEquals(setOf("user_id"), body().keys)
        rec.status = HttpStatusCode.NoContent; rec.body = ""
        api.removeRecoveryContact()
        assertEquals(HttpMethod.Delete, rec.last.method)
        rec.status = HttpStatusCode.OK
        rec.body = """{"accounts":[{"user_id":"u3","username":"opa","display_name":"Opa","named_at":"2026-11-03T12:00:00Z","confirmed_at":null}]}"""
        assertEquals("opa", api.contactFor().single().username)
        assertEquals("/api/me/recovery-contact-for", rec.last.path)
        rec.status = HttpStatusCode.NoContent; rec.body = ""
        api.confirmContactFor("u3")
        assertEquals("/api/me/recovery-contact-for/u3/confirm", rec.last.path)
    }
}
