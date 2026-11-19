package io.wolftown.kaiku.data.session

import java.util.logging.Level
import java.util.logging.Logger

/**
 * Signed-in user: what has to stop *before* a stored session is
 * removed (sign-out, session expiry, a rejected refresh, a change of the
 * signed-in user): voice, a DM call and the microphone. The token store runs
 * it synchronously right before it removes a stored session, so nothing of
 * the old account is still capturing when the account state is cleared.
 *
 * Dependency-free on purpose: the token store knows nothing about voice. The
 * app installs the block once at start. The block must be quick and must not
 * wait for other threads. A failing block never stops the sign-out: its
 * exception is logged and the session is removed all the same. The regular
 * teardown ([SessionTeardownRunner]) still runs afterwards.
 */
object SessionEndHook {
    private val logger = Logger.getLogger("SessionEndHook")

    @Volatile
    private var blocks: List<() -> Unit> = emptyList()

    /** Sets the blocks, run in this order (the app does this once, at start); none removes them. */
    fun install(vararg blocks: () -> Unit) {
        this.blocks = blocks.toList()
    }

    /** Runs every block, each on its own (a failing one does not skip the next); never throws. */
    fun beforeSessionEnds() {
        for (block in blocks) {
            try {
                block()
            } catch (e: Exception) {
                logger.log(Level.WARNING, "session end hook failed: ${e::class.simpleName}")
            }
        }
    }
}
