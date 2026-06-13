package io.wolftown.kaiku.domain

import io.wolftown.kaiku.ui.channel.COMMON_REACTIONS
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ReactionsTest {

    @Test
    fun reactions_areNonEmpty() {
        assertTrue("expected at least one quick reaction", COMMON_REACTIONS.isNotEmpty())
    }

    @Test
    fun reactions_haveNoDuplicates() {
        assertEquals(COMMON_REACTIONS.size, COMMON_REACTIONS.toSet().size)
    }

    @Test
    fun reactions_areAllNonBlank() {
        assertTrue(COMMON_REACTIONS.all { it.isNotBlank() })
    }

    @Test
    fun reactions_fitTheFourColumnGridEvenly() {
        // The picker renders a fixed 4-column grid; a multiple of 4 avoids a
        // ragged last row.
        assertEquals(0, COMMON_REACTIONS.size % 4)
    }
}
