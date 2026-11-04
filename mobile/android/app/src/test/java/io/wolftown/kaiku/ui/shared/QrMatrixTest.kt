package io.wolftown.kaiku.ui.shared

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class QrMatrixTest {

    private val payload = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"

    @Test
    fun `qr matrix is square with a four-module quiet zone`() {
        val m = qrMatrix(payload)
        assertEquals(m.width, m.height)
        // Four modules of quiet zone on every side: no module is set there.
        for (i in 0 until m.width) {
            for (q in 0 until 4) {
                assertFalse(m.get(i, q)); assertFalse(m.get(q, i))
                assertFalse(m.get(i, m.height - 1 - q)); assertFalse(m.get(m.width - 1 - q, i))
            }
        }
        // The finder pattern starts right after the quiet zone.
        assertTrue(m.get(4, 4))
    }

    @Test
    fun `same payload gives the same matrix`() {
        assertEquals(qrMatrix(payload), qrMatrix(payload))
    }
}
