package io.wolftown.kaiku.ui.security

import io.wolftown.kaiku.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

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
}
