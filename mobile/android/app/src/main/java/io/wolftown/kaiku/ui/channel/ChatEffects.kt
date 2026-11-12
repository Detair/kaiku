package io.wolftown.kaiku.ui.channel

import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import kotlin.coroutines.coroutineContext

/**
 * Shows a snackbar with a dismiss button until the user answers it or
 * [obsolete] returns. Then exactly this snackbar is withdrawn: taken down
 * when it is showing, or removed from the queue when another one is showing
 * first. Returns null when it was withdrawn.
 */
internal suspend fun SnackbarHostState.showSnackbarUntil(
    message: String,
    actionLabel: String?,
    obsolete: suspend () -> Unit,
): SnackbarResult? = coroutineScope {
    val shown = async { showSnackbar(message = message, actionLabel = actionLabel, withDismissAction = true) }
    val watcher = launch {
        obsolete()
        shown.cancel()
    }
    try {
        shown.await()
    } catch (e: CancellationException) {
        ensureActive() // the caller itself was cancelled: pass it on
        null
    } finally {
        watcher.cancel()
    }
}

/**
 * Runs [block] (a scroll) and ignores an interruption of it, for example by a
 * touch or another scroll, as long as the caller itself is still active. A
 * real cancellation of the caller is passed on.
 */
internal suspend fun ignoringInterruption(block: suspend () -> Unit) {
    try {
        block()
    } catch (e: CancellationException) {
        coroutineContext.ensureActive()
    }
}
