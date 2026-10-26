package io.wolftown.kaiku.domain

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class TypingNamesTest {

    @Test
    fun `typing shows names not ids`() {
        val names = typingNames(
            ids = linkedSetOf("id-1", "id-unknown"),
            members = mapOf("id-1" to "Alice"),
            someone = "Someone",
        )

        assertEquals(listOf("Alice", "Someone"), names)
        assertFalse(names.any { it.startsWith("id-") })
    }
}
