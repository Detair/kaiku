package io.wolftown.kaiku.approval

import androidx.biometric.BiometricPrompt
import org.junit.Assert.assertEquals
import org.junit.Test

class BiometricErrorMappingTest {
    @Test fun `lockouts map to LockedOut and cancels to Cancelled`() {
        assertEquals(UvResult.LockedOut, mapBiometricError(BiometricPrompt.ERROR_LOCKOUT))
        assertEquals(UvResult.LockedOut, mapBiometricError(BiometricPrompt.ERROR_LOCKOUT_PERMANENT))
        assertEquals(UvResult.Cancelled, mapBiometricError(BiometricPrompt.ERROR_USER_CANCELED))
        assertEquals(UvResult.Cancelled, mapBiometricError(BiometricPrompt.ERROR_NEGATIVE_BUTTON))
        assertEquals(UvResult.Cancelled, mapBiometricError(BiometricPrompt.ERROR_CANCELED))
        assertEquals(UvResult.Failed("UV_ERROR_${BiometricPrompt.ERROR_HW_UNAVAILABLE}"), mapBiometricError(BiometricPrompt.ERROR_HW_UNAVAILABLE))
    }
}
