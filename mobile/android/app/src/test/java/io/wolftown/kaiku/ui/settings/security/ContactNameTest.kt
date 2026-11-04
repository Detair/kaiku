package io.wolftown.kaiku.ui.settings.security

import org.junit.Assert.assertEquals
import org.junit.Test

class ContactNameTest {
    @Test fun `the unique username is always shown`() {
        assertEquals("Oma (@oma)", contactName("Oma", "oma"))
        assertEquals("@oma", contactName("", "oma"))
        assertEquals("@oma", contactName("oma", "oma"))
        // Two people called "Oma" stay apart.
        assertEquals("Oma (@mallory)", contactName("Oma", "mallory"))
    }
}
