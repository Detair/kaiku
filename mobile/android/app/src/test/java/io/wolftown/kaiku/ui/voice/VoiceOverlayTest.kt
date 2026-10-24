package io.wolftown.kaiku.ui.voice

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VoiceOverlayTest {

    @Test
    fun `overlay shown while in a call off the voice screen`() {
        assertTrue(shouldShowVoiceOverlay("voice-ch-1", "home"))
        assertTrue(shouldShowVoiceOverlay("voice-ch-1", "channel/{channelId}"))
        assertTrue(shouldShowVoiceOverlay("voice-ch-1", "settings"))
    }

    @Test
    fun `overlay hidden on the voice screen`() {
        assertFalse(shouldShowVoiceOverlay("voice-ch-1", VOICE_ROUTE))
    }

    @Test
    fun `overlay hidden without a call`() {
        assertFalse(shouldShowVoiceOverlay(null, "home"))
        assertFalse(shouldShowVoiceOverlay(null, VOICE_ROUTE))
    }
}
