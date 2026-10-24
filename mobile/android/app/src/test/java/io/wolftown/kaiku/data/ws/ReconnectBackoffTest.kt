package io.wolftown.kaiku.data.ws

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.roundToLong
import kotlin.random.Random

class ReconnectBackoffTest {

    /** Always returns [value] from nextDouble(), so the jitter factor is fixed. */
    private class FixedRandom(private val value: Double) : Random() {
        override fun nextBits(bitCount: Int): Int = 0
        override fun nextDouble(): Double = value
    }

    @Test
    fun `backoff doubles to cap and resets`() {
        // nextDouble() = 0.5 maps to a jitter factor of exactly 1.0.
        val backoff = ReconnectBackoff(random = FixedRandom(0.5))

        val delays = List(7) { backoff.next() }
        assertEquals(listOf(1_000L, 2_000L, 4_000L, 8_000L, 16_000L, 30_000L, 30_000L), delays)

        backoff.reset()
        assertEquals(1_000L, backoff.next())
    }

    @Test
    fun `jitter stays within 20 percent`() {
        val backoff = ReconnectBackoff(random = Random(42))
        var base = 1_000L
        val delays = mutableListOf<Long>()
        repeat(200) {
            val delay = backoff.next()
            delays += delay
            val low = (base * 0.8).roundToLong()
            val high = (base * 1.2).roundToLong()
            assertTrue("delay $delay outside [$low, $high] for base $base", delay in low..high)
            base = (base * 2).coerceAtMost(30_000L)
        }
        // Jitter is actually applied: capped delays are not all identical.
        assertTrue(delays.drop(10).toSet().size > 1)
    }

    @Test
    fun `extreme random values hit the jitter bounds`() {
        assertEquals(800L, ReconnectBackoff(random = FixedRandom(0.0)).next())
        assertEquals(1_200L, ReconnectBackoff(random = FixedRandom(1.0)).next())
    }
}
