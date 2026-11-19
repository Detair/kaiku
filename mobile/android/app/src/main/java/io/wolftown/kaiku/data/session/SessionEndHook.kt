package io.wolftown.kaiku.data.session

import java.util.logging.Level
import java.util.logging.Logger

/**
 * C32 "signed-in user": what has to stop *before* a stored session is
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
    private var block: (() -> Unit)? = null

    /** Sets the block (the app does this once, at start); null removes it. */
    fun install(block: (() -> Unit)?) {
        this.block = block
    }

    /** Runs the block, if any; never throws. */
    fun beforeSessionEnds() {
        val run = block ?: return
        try {
            run()
        } catch (e: Exception) {
            logger.log(Level.WARNING, "session end hook failed: ${e::class.simpleName}")
        }
    }
}
