package io.wolftown.kaiku.ui.shared

import io.wolftown.kaiku.ui.shared.NotificationsOffBannerPolicy.SNOOZE_MS
import io.wolftown.kaiku.ui.shared.NotificationsOffBannerPolicy.shouldShow
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** AND-B-12: after "Not now" or a system denial the hint keeps coming back. */
class NotificationsOffBannerPolicyTest {

    private val now = 1_000_000_000_000L

    @Test
    fun `hidden while notifications are on`() {
        assertFalse(shouldShow(notificationsEnabled = true, dismissedAtMs = null, nowMs = now))
    }

    @Test
    fun `shown while notifications are off and never closed`() {
        assertTrue(shouldShow(notificationsEnabled = false, dismissedAtMs = null, nowMs = now))
    }

    @Test
    fun `closing hides it for a week, then it returns`() {
        assertFalse(shouldShow(false, dismissedAtMs = now, nowMs = now + SNOOZE_MS - 1))
        assertTrue(shouldShow(false, dismissedAtMs = now, nowMs = now + SNOOZE_MS))
    }

    @Test
    fun `a clock set back does not hide it for good`() {
        assertTrue(shouldShow(false, dismissedAtMs = now, nowMs = now - 1))
    }
}
