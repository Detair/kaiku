package io.wolftown.kaiku.data.auth

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Duration
import java.time.Instant

class RenewalPolicyTest {

    private val cap = Instant.parse("2027-02-01T12:00:00Z")

    @Test fun `renewal prompts within seven days at most daily and never from a call`() {
        val eightDaysBefore = cap.minus(Duration.ofDays(8))
        val sixDaysBefore = cap.minus(Duration.ofDays(6))
        assertFalse(RenewalPolicy.shouldRenew(eightDaysBefore, cap, null, fromCallOrNotification = false))
        assertTrue(RenewalPolicy.shouldRenew(sixDaysBefore, cap, null, fromCallOrNotification = false))
        // At most once per 24 h.
        assertFalse(RenewalPolicy.shouldRenew(sixDaysBefore, cap, sixDaysBefore.minus(Duration.ofHours(23)), false))
        assertTrue(RenewalPolicy.shouldRenew(sixDaysBefore, cap, sixDaysBefore.minus(Duration.ofHours(24)), false))
        // Never from a call or a notification.
        assertFalse(RenewalPolicy.shouldRenew(sixDaysBefore, cap, null, fromCallOrNotification = true))
        // Without a known cap there is nothing to renew.
        assertFalse(RenewalPolicy.shouldRenew(sixDaysBefore, null, null, false))
    }
}
