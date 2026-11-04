package io.wolftown.kaiku.ui.scan

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Leaving the scanner releases the camera, also when it is bound only after the view left. */
class CameraBindingTest {
    @Test fun `dispose unbinds what was bound, once`() {
        var unbound = 0
        val b = CameraBinding()
        b.set { unbound++ }
        b.dispose()
        b.dispose()
        assertEquals(1, unbound)
        assertTrue(b.disposed)
    }

    @Test fun `a bind that lands after dispose is undone at once`() {
        var unbound = 0
        val b = CameraBinding()
        b.dispose()
        b.set { unbound++ }
        assertEquals(1, unbound)
    }
}
