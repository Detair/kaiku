package io.wolftown.kaiku.ui.call

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class IncomingCallPresentationTest {

    @Test
    fun `below Android 14 full-screen is allowed without asking`() {
        var asked = false
        assertTrue(IncomingCallPresentation.canUseFullScreenIntent(33) { asked = true; false })
        assertFalse(asked)
    }

    @Test
    fun `from Android 14 the system decides`() {
        assertTrue(IncomingCallPresentation.canUseFullScreenIntent(34) { true })
        assertFalse(IncomingCallPresentation.canUseFullScreenIntent(35) { false })
    }

    @Test
    fun `without full-screen the call rings as a heads-up with actions`() {
        assertEquals(IncomingCallStyle.CallStyleFullScreen, IncomingCallPresentation.choose(true))
        assertEquals(IncomingCallStyle.HeadsUpWithActions, IncomingCallPresentation.choose(false))
    }
}
