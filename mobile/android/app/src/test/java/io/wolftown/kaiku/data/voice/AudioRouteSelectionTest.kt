package io.wolftown.kaiku.data.voice

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class AudioRouteSelectionTest {

    @Test
    fun `default route prefers bluetooth then wired then speaker`() {
        val all = setOf(
            AudioRoute.Speaker, AudioRoute.Earpiece, AudioRoute.WiredHeadset, AudioRoute.Bluetooth
        )
        assertEquals(AudioRoute.Bluetooth, selectDefaultRoute(all))
        assertEquals(AudioRoute.WiredHeadset, selectDefaultRoute(all - AudioRoute.Bluetooth))
        assertEquals(
            AudioRoute.Speaker,
            selectDefaultRoute(setOf(AudioRoute.Speaker, AudioRoute.Earpiece))
        )
    }

    @Test
    fun `earpiece is never the default`() {
        assertNotEquals(
            AudioRoute.Earpiece,
            selectDefaultRoute(setOf(AudioRoute.Earpiece, AudioRoute.Speaker))
        )
        assertEquals(AudioRoute.Speaker, selectDefaultRoute(setOf(AudioRoute.Earpiece)))
        assertEquals(AudioRoute.Speaker, selectDefaultRoute(emptySet()))
    }
}
