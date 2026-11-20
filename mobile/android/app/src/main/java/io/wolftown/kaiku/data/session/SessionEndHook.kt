package io.wolftown.kaiku.data.session

import java.util.logging.Level
import java.util.logging.Logger

/**
 * Signed-in user: what has to stop *before* a stored session is
 * removed (sign-out, session expiry, a rejected refresh, a change of the
 * signed-in user): voice, a DM call and the microphone. The token store runs
 * it synchronously right before it removes a stored session, so nothing of
 * the old account is still capturing when the account state is cleared.
 * Before any of that it advances the session generation
 * ([SessionGeneration]), so nothing of the ending session runs on.
 *
 * Dependency-free on purpose: the token store knows nothing about voice. The
 * app installs the blocks once at start. A block runs on the caller's thread
 * (which may hold the token store's lock): it must not call into WebRTC
 * there and may wait for another thread only for a short, bounded time
 * (voice waits at most 2 s for the microphone to be released). A failing block never stops the sign-out: its
 * exception is logged and the session is removed all the same. The regular
 * teardown ([SessionTeardownRunner]) still runs afterwards.
 */
object SessionEndHook {
    private val logger = Logger.getLogger("SessionEndHook")

    @Volatile
    private var blocks: List<() -> Unit> = emptyList()

    @Volatile
    private var first: (() -> Unit)? = null

    /** Sets the blocks, run in this order (the app does this once, at start); none removes them. */
    fun install(vararg blocks: () -> Unit) {
        this.blocks = blocks.toList()
    }

    /**
     * Sets what runs before every block, whatever was installed when: the
     * advance of the session generation ([SessionGeneration.advance]). So a
     * block, and everything it stops, already sees the old generation as over.
     */
    fun installFirst(advance: (() -> Unit)?) {
        first = advance
    }

    /** Runs the advance, then every block, each on its own (a failing one does not skip the next); never throws. */
    fun beforeSessionEnds() {
        for (block in listOfNotNull(first) + blocks) {
            try {
                block()
            } catch (e: Exception) {
                logger.log(Level.WARNING, "session end hook failed: ${e::class.simpleName}")
            }
        }
    }
}
