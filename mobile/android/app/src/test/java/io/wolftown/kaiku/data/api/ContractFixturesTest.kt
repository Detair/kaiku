package io.wolftown.kaiku.data.api

import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.AttemptQr
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.ClaimResult
import io.wolftown.kaiku.domain.model.Enrollment
import io.wolftown.kaiku.domain.model.EnrollmentClaim
import io.wolftown.kaiku.domain.model.EnrollmentIssued
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.domain.model.KeyDevice
import io.wolftown.kaiku.domain.model.KeyDeviceStatus
import io.wolftown.kaiku.domain.model.LoginAttempt
import io.wolftown.kaiku.domain.model.LoginStep
import io.wolftown.kaiku.domain.model.MeSecurity
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.NoticeAction
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.SecurityEvent
import io.wolftown.kaiku.testutil.ApiRecorder
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Hand-written fixtures from the C26–C28 shapes; they decode, and unknown values stay tolerated (C5). */
class ContractFixturesTest {

    private fun res(name: String) = javaClass.classLoader!!.getResource("contract/$name")!!.readText()

    @Test fun `every fixture decodes`() {
        KaikuJson.decodeFromString<ApprovalChallenge>(res("approval_challenge_login.json"))
        assertEquals(EnrollmentState.CLAIMED, KaikuJson.decodeFromString<Enrollment>(res("enrollment_claimed.json")).state)
        KaikuJson.decodeFromString<EnrollmentIssued>(res("enrollment_issued.json"))
        KaikuJson.decodeFromString<LoginStep>(res("login_step.json"))
        KaikuJson.decodeFromString<LoginAttempt>(res("login_attempt_open.json"))
        val me = KaikuJson.decodeFromString<MeSecurity>(res("me_security.json"))
        assertEquals(listOf("add_passkey", "name_recovery_contact", "some_future_prompt"), me.prompts)
        val ev = KaikuJson.decodeFromString<SecurityEvent>(res("security_event.json"))
        assertEquals(listOf(NoticeAction.REPUDIATE, NoticeAction.UNKNOWN), ev.actions.map { it.action })
        assertEquals("desktop", ev.client!!.clientClass)
        val msg = KaikuJson.decodeFromString<Message>(res("security_notice_message.json"))
        assertEquals("security", msg.messageType)
        assertTrue(msg.author.isSystem)
        assertEquals(2, msg.securityNotice!!.count)
        KaikuJson.decodeFromString<AccountRequestSummary>(res("account_request_summary.json"))
        assertEquals(KeyDeviceStatus.ACTIVE, KaikuJson.decodeFromString<KeyDevice>(res("key_device.json")).state)
        val auth = KaikuJson.decodeFromString<AuthResponse>(res("auth_response_full.json"))
        assertEquals("full", auth.assurance)
        assertEquals("2027-02-01T12:00:00Z", auth.absoluteExpiresAt)
    }

    @Test fun `old auth responses still decode`() {
        val auth = KaikuJson.decodeFromString<AuthResponse>("""{"access_token":"a","expires_in":900,"token_type":"Bearer"}""")
        assertEquals(null, auth.sessionId)
        assertEquals(null, auth.assurance)
    }

    @Test fun `dm item with is_system and read_only sets isSystem and readOnly`() = runTest {
        val rec = ApiRecorder(body = res("dm_list_kaiku.json"))
        val dms = DmApiImpl(rec.client).getDms()
        assertTrue(dms[0].isSystem)
        assertTrue(dms[0].readOnly)
        // Recognised by flags only: a DM with the system account's id or name but no flags is not the Kaiku DM.
        assertFalse(dms[1].isSystem)
        assertFalse(dms[1].readOnly)
    }

    @Test fun `toString never contains secrets`() {
        val secrets = listOf("setup-token-secret", "sig-secret", "claim-secret", "123456", "refresh-secret-value", "eyJ.access.token", "oKGio6Slpq")
        val values = listOf(
            EnrollmentClaim("setup-token-secret", "c", "sig-secret", "pk", listOf("a"), "Pixel", "per_use", 35),
            ClaimResult(enrollmentId = "e", state = "claimed", code = "123456", claimSecret = "claim-secret"),
            KaikuJson.decodeFromString<EnrollmentIssued>(res("enrollment_issued.json").replace("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8", "setup-token-secret")),
            KaikuJson.decodeFromString<AuthResponse>(res("auth_response_full.json")),
            AttemptQr(payload = "kaiku://login/v1?o=x&t=oKGio6Slpq", expiresAt = "t"),
            Part2Factor.totp("123456"),
        )
        for (v in values) for (s in secrets) assertFalse("$v leaks $s", v.toString().contains(s))
    }
}
