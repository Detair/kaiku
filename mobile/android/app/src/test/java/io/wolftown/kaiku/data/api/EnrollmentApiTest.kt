package io.wolftown.kaiku.data.api

import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.domain.model.EnrollmentClaim
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class EnrollmentApiTest {

    private val rec = ApiRecorder()
    private val api = EnrollmentApiImpl(rec.client)
    private fun res(name: String) = javaClass.classLoader!!.getResource("contract/$name")!!.readText()
    private fun bodyKeys() = Json.parseToJsonElement(rec.last.body).jsonObject.keys

    private val claim = EnrollmentClaim(
        setupToken = "setup-token-secret", challengeId = "c5", signature = "sig-secret", publicKey = "pk",
        attestationChain = listOf("leaf", "root"), label = "Google Pixel 8", uvClass = "per_use", apiLevel = 35,
    )

    @Test fun `issue own posts an empty object`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = res("enrollment_issued.json")
        val issued = api.issueOwn()
        assertEquals(HttpMethod.Post, rec.last.method)
        assertEquals("/auth/enrollments", rec.last.path)
        assertEquals("{}", rec.last.body)
        assertTrue(issued.setupUrl.startsWith("https://chat.example.com/setup#v=1&t="))
    }

    @Test fun `issue with ticket uses the registration issuer and the ticket header`() = runTest {
        rec.status = HttpStatusCode.Created; rec.body = res("enrollment_issued.json")
        api.issueWithTicket("ticket-secret")
        assertEquals("/auth/registration/enrollments", rec.last.path)
        assertEquals("ticket-secret", rec.last.headers["X-Kaiku-Ticket"])
    }

    @Test fun `get and cancel`() = runTest {
        rec.body = res("enrollment_claimed.json")
        val e = api.get("55555555-5555-4555-8555-555555555555")
        assertEquals(HttpMethod.Get, rec.last.method)
        assertEquals("/auth/enrollments/55555555-5555-4555-8555-555555555555", rec.last.path)
        assertEquals(EnrollmentState.CLAIMED, e.state)
        assertEquals("Pixel 8", e.claimed!!.label)
        rec.status = HttpStatusCode.NoContent; rec.body = ""
        api.cancel("55555555-5555-4555-8555-555555555555")
        assertEquals(HttpMethod.Delete, rec.last.method)
    }

    @Test fun `new device routes go to the given origin without a bearer`() = runTest {
        rec.body = """{"server_name":"Familie Müller","enrollment_nonce":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","expires_at":"2026-11-03T12:10:00Z"}"""
        val open = api.open("setup-token-secret", origin = "https://other.example.org")
        assertEquals("https://other.example.org/auth/enrollments/open", rec.last.url)
        assertEquals(setOf("setup_token"), bodyKeys())
        assertNull(rec.last.headers["Authorization"])
        assertEquals("Familie Müller", open.serverName)

        rec.status = HttpStatusCode.Created
        rec.body = """{"challenge":${res("approval_challenge_login.json")},"subject":{"user_id":"u","username":"oma","display_name":"Oma"}}"""
        val pop = api.popChallenge("setup-token-secret", "d", "pk")
        assertEquals("/auth/enrollments/pop-challenge", rec.last.path)
        assertEquals(setOf("setup_token", "device_id", "public_key"), bodyKeys())
        assertNull(rec.last.headers["Authorization"])
        assertEquals("oma", pop.subject.username)
    }

    @Test fun `claim sends every field and a ticket header when given`() = runTest {
        rec.status = HttpStatusCode.Created
        rec.body = """{"enrollment_id":"e","state":"claimed","code":"123456","code_expires_at":"2026-11-03T12:05:00Z","claim_secret":"claim-secret"}"""
        val r = api.claim(claim)
        assertEquals("/auth/enrollments/claim", rec.last.path)
        assertEquals(
            setOf("setup_token", "challenge_id", "signature", "public_key", "attestation_chain", "label", "uv_class", "api_level"),
            bodyKeys(),
        )
        assertNull(rec.last.headers["X-Kaiku-Ticket"])
        assertNull(rec.last.headers["Authorization"])
        assertEquals("123456", r.code)
        rec.body = """{"enrollment_id":"e","state":"done","authenticator_state":"active","ready_at":null}"""
        val done = api.claim(claim, ticket = "ticket-secret")
        assertEquals("ticket-secret", rec.last.headers["X-Kaiku-Ticket"])
        assertEquals("done", done.state)
        assertNull(done.code)
    }

    @Test fun `status long polls with the claim secret`() = runTest {
        rec.body = """{"state":"done","authenticator_state":"pending","ready_at":"2026-11-04T12:00:00Z"}"""
        val s = api.status("e", "claim-secret", 25)
        assertEquals("/auth/enrollments/status", rec.last.path)
        assertEquals(setOf("enrollment_id", "claim_secret", "wait"), bodyKeys())
        assertEquals(EnrollmentState.DONE, s.state)
        assertEquals("pending", s.authenticatorState)
    }

    @Test fun `wrong code carries tries left`() = runTest {
        rec.status = HttpStatusCode.BadRequest
        rec.body = """{"error":"ENROLL_CODE_WRONG","message":"x","tries_left":2}"""
        try {
            api.get("e"); fail("expected ApiException")
        } catch (e: ApiException) {
            assertEquals("ENROLL_CODE_WRONG", e.normalizedCode)
            assertEquals(2, e.fields.triesLeft)
        }
    }
}
