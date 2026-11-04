package io.wolftown.kaiku.approval

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class KeySpecTest {

    private val decisions = listOf(
        UvPolicy.choose(30, deviceSecure = true, strongBiometric = true),
        UvPolicy.choose(28, deviceSecure = true, strongBiometric = true),
        UvPolicy.choose(28, deviceSecure = true, strongBiometric = false),
    ).map { it as UvDecision.Use }

    @Test fun `biometric enrolment does not invalidate the key spec`() {
        decisions.forEach { assertFalse(keySpecParams(it).invalidatedByBiometricEnrollment) }
    }

    @Test fun `window keys use ten seconds`() {
        val p = keySpecParams(decisions[2])
        assertFalse(p.perUseAuth)
        assertEquals(10, p.validitySeconds)
        assertNull(p.authTypes)
    }

    @Test fun `api30 keys need auth for every use by biometric or credential`() {
        val p = keySpecParams(decisions[0])
        assertTrue(p.perUseAuth)
        assertEquals(0, p.validitySeconds)
        assertEquals(KeyAuthTypes.BIOMETRIC_STRONG or KeyAuthTypes.DEVICE_CREDENTIAL, p.authTypes)
    }

    @Test fun `api26 to 29 biometric keys need a biometric for every use`() {
        val p = keySpecParams(decisions[1])
        assertTrue(p.perUseAuth)
        assertEquals(-1, p.validitySeconds)
        assertNull(p.authTypes)
    }
}
