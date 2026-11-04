package io.wolftown.kaiku.ui.shared

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** FLAG_SECURE and touch filtering survive a transition between two protected screens. */
class SecureScreensTest {

    private class Flag(var on: Boolean)

    private val window = Any()
    private val view = Any()
    private val secure = Flag(false)
    private val filtering = Flag(false)

    private fun enter() = SecureScreens.acquire(window, view, { secure.on }, { secure.on = it }, { filtering.on }, { filtering.on = it })
    private fun leave() = SecureScreens.release(window, view, { secure.on = it }, { filtering.on = it })

    @Test fun `a transition between two secure screens keeps both protections`() {
        enter() // screen A
        enter() // screen B enters while A is still composed (animated navigation)
        leave() // A leaves
        assertTrue(secure.on)
        assertTrue(filtering.on)
        leave() // B leaves: the original flags come back
        assertFalse(secure.on)
        assertFalse(filtering.on)
        assertEquals(0, SecureScreens.activeCount(window))
    }

    @Test fun `a window that was secure before stays secure`() {
        secure.on = true
        enter()
        leave()
        assertTrue(secure.on)
        assertFalse(filtering.on)
    }

    @Test fun `an extra release changes nothing`() {
        enter()
        leave()
        secure.on = true // set by someone else afterwards
        leave()
        assertTrue(secure.on)
    }
}
