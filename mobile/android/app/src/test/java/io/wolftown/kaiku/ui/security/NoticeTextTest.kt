package io.wolftown.kaiku.ui.security

import io.wolftown.kaiku.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test
import io.wolftown.kaiku.domain.model.SecurityEvent
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

class NoticeTextTest {

    /** Every C28 event type. */
    private val types = listOf(
        "sign_in", "sign_in_not_finished", "second_factor_failed", "guessing", "approval_reported",
        "authenticator_added", "authenticator_removed", "key_invalidated", "recovery_codes_created",
        "password_set", "password_changed", "password_removed", "password_throttled", "password_compromised",
        "totp_throttled", "totp_compromised", "step_up_action", "repudiated",
        "request_started", "request_completed", "request_cancelled", "request_withdrawn",
        "request_ready_soon", "request_ready", "request_expired", "request_void",
        "admin_setup", "admin_role_granted", "admin_role_removed",
        "recovery_contact_named", "recovery_contact_confirmed", "recovery_contact_changed",
        "session_ended_reuse", "session_ended_limit", "bootstrap_reminder",
        // C28 amendment (2026-11-03): admin and CLI recovery actions.
        "sessions_ended", "factor_restored", "throttle_reset", "may_approve_changed",
        // "Set up this PC" (2026-11-08).
        "pc_setup_reported", "pc_setup_failed",
    )

    @Test fun `every event type has notice and push strings and unknown uses generic`() {
        val titles = types.map { NoticeText.title(it) }
        val pushes = types.map { NoticeText.push(it) }
        titles.forEach { assertNotEquals(R.string.security_notice_generic, it) }
        pushes.forEach { assertNotEquals(R.string.security_push_generic, it) }
        assertEquals(types.size, titles.toSet().size)
        assertEquals(types.size, pushes.toSet().size)
        assertEquals(R.string.security_notice_generic, NoticeText.title("time_travel"))
        assertEquals(R.string.security_push_generic, NoticeText.push("time_travel"))
    }

    private fun added(vararg details: Pair<String, String>) = SecurityEvent(
        id = "e1",
        eventType = "authenticator_added",
        createdAt = "2026-11-08T10:00:00Z",
        details = JsonObject(details.associate { (k, v) -> k to JsonPrimitive(v) }),
    )

    @Test fun `a new passkey from Set up this PC names the computer and the phone`() {
        val e = added("via" to "pc_setup", "client_description" to "Firefox on Linux", "approving_device_label" to "Pixel 8")
        assertEquals(R.string.security_notice_authenticator_added_pc_setup, NoticeText.title(e.eventType, e))
        assertEquals("Firefox on Linux", NoticeText.computer(e))
        assertEquals("Pixel 8", NoticeText.phone(e))
    }

    @Test fun `any other new authenticator keeps its usual title`() {
        val plain = added("type" to "passkey")
        assertEquals(R.string.security_notice_authenticator_added, NoticeText.title(plain.eventType, plain))
        // Without both names the PC text would have holes: the usual title.
        val partial = added("via" to "pc_setup", "client_description" to "Firefox on Linux")
        assertEquals(R.string.security_notice_authenticator_added, NoticeText.title(partial.eventType, partial))
        assertNull(NoticeText.phone(partial))
        assertEquals(R.string.security_notice_sign_in, NoticeText.title("sign_in", null))
    }
}
