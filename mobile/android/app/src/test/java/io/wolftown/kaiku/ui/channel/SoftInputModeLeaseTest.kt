package io.wolftown.kaiku.ui.channel

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Review W1: switching chat A -> chat B directly (both on screen during the crossfade). */
class SoftInputModeLeaseTest {

    private val pan = 0x20 // SOFT_INPUT_ADJUST_PAN
    private val stateHidden = 0x02
    private val resize = SoftInputModeLease.ADJUST_RESIZE

    @Test
    fun `chat B keeps resize when chat A leaves after B appeared, last one restores`() {
        val lease = SoftInputModeLease()
        val original = pan or stateHidden

        val inA = lease.acquire(original)
        assertEquals(resize or stateHidden, inA)
        val inB = lease.acquire(inA) // B appears while A is still composed
        assertEquals(resize or stateHidden, inB)

        assertNull(lease.release()) // A disposed after the animation: B stays resized
        assertEquals(original, lease.release()) // B leaves: the window's own mode comes back
    }

    @Test
    fun `a single chat restores the mode it found`() {
        val lease = SoftInputModeLease()
        lease.acquire(0)
        assertEquals(0, lease.release())
    }

    @Test
    fun `an extra release changes nothing`() {
        val lease = SoftInputModeLease()
        assertNull(lease.release())
        lease.acquire(pan)
        assertEquals(pan, lease.release())
        assertNull(lease.release())
    }
}
