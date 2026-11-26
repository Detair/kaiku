package io.wolftown.kaiku.ui.auth

import java.util.concurrent.atomic.AtomicInteger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Registrations on their way (from creating the account to the setup code it gets): while one is,
 * the app's server address does not change ([io.wolftown.kaiku.ui.auth.setup.ServerAddressRule]).
 */
@Singleton
class Registrations @Inject constructor() {
    private val count = AtomicInteger()

    fun running(): Boolean = count.get() > 0

    /** Runs [block] as a registration on its way. */
    suspend fun <T> during(block: suspend () -> T): T {
        count.incrementAndGet()
        try {
            return block()
        } finally {
            count.decrementAndGet()
        }
    }
}
