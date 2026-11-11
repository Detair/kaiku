package io.wolftown.kaiku.data.voice

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * [EglBase][org.webrtc.EglBase] cannot even be loaded on the JVM, so the
 * "one shared instance" rule of [DefaultEglBaseProvider] is tested on the
 * generic holder it delegates to.
 */
class EglBaseProviderTest {

    @Test
    fun `every caller gets the same instance and it is created once`() {
        var created = 0
        val shared = SharedInstance { created++; Any() }

        // The factory's video decoder and the renderers must share one EGL context.
        val first = shared.get()
        val second = shared.get()

        assertSame(first, second)
        assertEquals(1, created)
    }

    @Test
    fun `nothing is created before the first get`() {
        var created = 0
        SharedInstance { created++; Any() }

        assertEquals(0, created)
    }
}
