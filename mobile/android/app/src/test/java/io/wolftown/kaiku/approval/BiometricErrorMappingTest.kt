package io.wolftown.kaiku.approval

import androidx.biometric.BiometricPrompt
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.UnrecoverableKeyException

class BiometricErrorMappingTest {
    @Test fun `lockouts map to LockedOut and cancels to Cancelled`() {
        assertEquals(UvResult.LockedOut, mapBiometricError(BiometricPrompt.ERROR_LOCKOUT))
        assertEquals(UvResult.LockedOut, mapBiometricError(BiometricPrompt.ERROR_LOCKOUT_PERMANENT))
        assertEquals(UvResult.Cancelled, mapBiometricError(BiometricPrompt.ERROR_USER_CANCELED))
        assertEquals(UvResult.Cancelled, mapBiometricError(BiometricPrompt.ERROR_NEGATIVE_BUTTON))
        assertEquals(UvResult.Cancelled, mapBiometricError(BiometricPrompt.ERROR_CANCELED))
        assertEquals(UvResult.Failed("UV_ERROR_${BiometricPrompt.ERROR_HW_UNAVAILABLE}"), mapBiometricError(BiometricPrompt.ERROR_HW_UNAVAILABLE))
    }

    @Test fun `no screen lock any more means the key is invalidated`() {
        // Android invalidates a user-authenticated key for good when the secure lock screen goes (B.1).
        assertEquals(UvResult.KeyInvalidated, mapBiometricError(BiometricPrompt.ERROR_NO_DEVICE_CREDENTIAL))
    }

    @Test fun `no fingerprints left is not an invalidation`() {
        // setInvalidatedByBiometricEnrollment(false) binds the key to the root SID: it survives the
        // removal of every fingerprint and works again once one is added, so it is never forgotten here.
        assertEquals(UvResult.Failed("UV_ERROR_${BiometricPrompt.ERROR_NO_BIOMETRICS}"), mapBiometricError(BiometricPrompt.ERROR_NO_BIOMETRICS))
    }

    @Test fun `a phone without a screen lock reports the key invalidated before any prompt`() {
        assertEquals(UvResult.KeyInvalidated, uvPreflight(hasKey = true, deviceSecure = false))
        assertEquals(UvResult.KeyInvalidated, uvPreflight(hasKey = false, deviceSecure = true))
        assertNull(uvPreflight(hasKey = true, deviceSecure = true))
    }

    @Test fun `only an invalidated key's lookup failure counts as invalidation`() {
        // keystore2 turns KEY_PERMANENTLY_INVALIDATED into this UnrecoverableKeyException at getKey().
        assertTrue(isInvalidatedKeyFailure(UnrecoverableKeyException("User changed or deleted their auth credentials")))
        assertTrue(isInvalidatedKeyFailure(UnrecoverableKeyException("x").apply { initCause(RuntimeException("Key permanently invalidated")) }))
        assertFalse(isInvalidatedKeyFailure(UnrecoverableKeyException("Failed to obtain information about key").apply { initCause(RuntimeException("System error")) }))
        assertFalse(isInvalidatedKeyFailure(UnrecoverableKeyException(null)))
    }
}
