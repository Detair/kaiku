package io.wolftown.kaiku.data.auth

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Duration
import java.time.Instant

class RenewalGateTest {
    private val cap = Instant.parse("2027-02-01T12:00:00Z")
    private val now = cap.minus(Duration.ofDays(3))

    @Test fun `an open from a notification never prompts and is forgotten afterwards`() {
        val gate = RenewalGate()
        gate.onIntent(fromNotification = true)
        assertFalse(gate.shouldPrompt(now, cap, null))
        // The next normal open prompts.
        assertTrue(gate.shouldPrompt(now, cap, null))
    }
}
