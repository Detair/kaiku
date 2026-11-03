package io.wolftown.kaiku.data.auth

import androidx.credentials.exceptions.domerrors.AbortError
import androidx.credentials.exceptions.domerrors.InvalidStateError
import androidx.credentials.exceptions.domerrors.NotAllowedError
import androidx.credentials.exceptions.domerrors.SecurityError
import org.junit.Assert.assertEquals
import org.junit.Test

class PasskeyCredentialsTest {
    @Test
    fun `DOM errors map by their type, not by message text`() {
        // Reviews Argon-a M2, Argon minor 4, Fable m7.
        assertEquals(PasskeyOutcome.NotAssociated, mapDomError(SecurityError()))
        assertEquals(PasskeyOutcome.Failed("PASSKEY_ALREADY_REGISTERED"), mapDomError(InvalidStateError()))
        assertEquals(PasskeyOutcome.Cancelled, mapDomError(NotAllowedError()))
        assertEquals(PasskeyOutcome.Failed("PASSKEY_FAILED"), mapDomError(AbortError()))
    }
}
