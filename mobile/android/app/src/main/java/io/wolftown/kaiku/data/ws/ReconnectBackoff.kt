package io.wolftown.kaiku.data.ws

import kotlin.math.roundToLong
import kotlin.random.Random

/**
 * Exponential backoff with jitter: the base delay starts at [initialMs],
 * doubles on every [next] up to [maxMs], and each returned delay is the base
 * scaled by a random factor in `[1 - jitter, 1 + jitter]`.
 */
class ReconnectBackoff(
    private val initialMs: Long = 1_000,
    private val maxMs: Long = 30_000,
    private val jitter: Double = 0.2,
    private val random: Random = Random.Default,
) {
    init {
        require(initialMs > 0) { "initialMs must be positive" }
        require(maxMs >= initialMs) { "maxMs must be >= initialMs" }
        require(jitter in 0.0..1.0) { "jitter must be within 0..1" }
    }

    private var currentMs = initialMs

    /** Returns the delay for the next attempt and advances the base delay. */
    fun next(): Long {
        val base = currentMs
        currentMs = (currentMs * 2).coerceAtMost(maxMs)
        val factor = 1.0 + jitter * (random.nextDouble() * 2.0 - 1.0)
        return (base * factor).roundToLong().coerceAtLeast(0L)
    }

    /** Starts again from [initialMs]. */
    fun reset() {
        currentMs = initialMs
    }
}
