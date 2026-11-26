package io.wolftown.kaiku.ui.auth.setup

import io.wolftown.kaiku.R
import org.junit.Assert.assertEquals
import org.junit.Test

class ScreenLockGuideTest {
    @Test fun `only a phone with a fingerprint reader is asked to add a fingerprint`() {
        assertEquals(R.string.setup_fingerprint_open, biometricEnrollLabel(hasFingerprint = true))
        assertEquals(R.string.setup_biometric_open, biometricEnrollLabel(hasFingerprint = false))
    }
}
